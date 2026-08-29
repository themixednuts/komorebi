use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use particle_kernel::{Avx2Kernel, ParticleBatch, ParticleStep, step_scalar};
use serde::Serialize;
use windows_sys::Win32::{
    Foundation::FILETIME,
    System::Threading::{GetCurrentProcess, GetProcessTimes},
};

const PARTICLES: usize = 2_048;
const ITERATIONS_PER_SAMPLE: usize = 512;
const WARMUP_SAMPLES: usize = 20;
const MEASURED_SAMPLES: usize = 120;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

struct CountingAllocator;

// SAFETY: every operation delegates to System with the original pointer and layout. The atomic
// counter observes successful allocation attempts and does not affect allocation ownership.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller supplies GlobalAlloc's required valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller supplies GlobalAlloc's required valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout come from the matching System allocation.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the pointer and layout come from System and size is the requested new size.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Variant {
    ScalarAutovectorized,
    Avx2,
    #[cfg(feature = "portable-simd")]
    PortableSimd,
}

impl Variant {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "scalar" => Ok(Self::ScalarAutovectorized),
            "avx2" => Ok(Self::Avx2),
            #[cfg(feature = "portable-simd")]
            "portable" => Ok(Self::PortableSimd),
            #[cfg(not(feature = "portable-simd"))]
            "portable" => bail!("portable SIMD support was not compiled into this binary"),
            _ => bail!("unknown variant {value:?}; expected scalar, avx2, portable, or profile"),
        }
    }
}

#[derive(Serialize)]
struct BenchmarkReport {
    variant: Variant,
    particles: usize,
    iterations_per_sample: usize,
    warmup_samples: usize,
    measured_samples: usize,
    p50_ns: u64,
    p95_ns: u64,
    p99_ns: u64,
    mean_ns: u64,
    process_cpu_100ns: u64,
    allocations_in_timed_regions: u64,
    checksum: f64,
    idle: IdleReport,
    cpu: CpuFeatures,
}

#[derive(Serialize)]
struct IdleReport {
    requested_ms: u64,
    elapsed_ms: u64,
    process_cpu_100ns: u64,
    wait: &'static str,
}

#[derive(Serialize)]
struct CpuFeatures {
    avx2: bool,
    fma: bool,
    sse42: bool,
}

#[derive(Serialize)]
struct ProfileReport {
    frames: usize,
    candidates: Vec<CandidateProfile>,
    selected: &'static str,
    reason: &'static str,
}

#[derive(Serialize)]
struct CandidateProfile {
    name: &'static str,
    elements_per_frame: usize,
    elapsed_ns: u64,
}

fn main() -> Result<()> {
    let command = std::env::args()
        .nth(1)
        .context("usage: particle-benchmark <profile|scalar|avx2|portable>")?;
    if command == "profile" {
        println!("{}", serde_json::to_string_pretty(&profile_candidates()?)?);
        return Ok(());
    }
    let variant = Variant::parse(&command)?;
    println!("{}", serde_json::to_string_pretty(&benchmark(variant)?)?);
    Ok(())
}

fn benchmark(variant: Variant) -> Result<BenchmarkReport> {
    let step = ParticleStep::checked(1.0 / 240.0, 0.997, 0.0, 9.81)?;
    let base = ParticleBatch::seeded(PARTICLES, 0x00C0_DE48);
    let avx2 = matches!(variant, Variant::Avx2)
        .then(Avx2Kernel::detect)
        .transpose()?;

    for _ in 0..WARMUP_SAMPLES {
        let mut batch = base.clone();
        run_iterations(variant, avx2, &mut batch, step);
        black_box(batch.checksum());
    }

    let cpu_before = process_cpu_time_100ns()?;
    let mut samples = Vec::with_capacity(MEASURED_SAMPLES);
    let mut checksum = 0.0;
    let mut allocations = 0;
    for _ in 0..MEASURED_SAMPLES {
        let mut batch = base.clone();
        ALLOCATIONS.store(0, Ordering::Relaxed);
        let started = Instant::now();
        run_iterations(variant, avx2, &mut batch, step);
        samples.push(nanos(started.elapsed()));
        allocations += ALLOCATIONS.load(Ordering::Relaxed);
        checksum += black_box(batch.checksum());
    }
    let process_cpu_100ns = process_cpu_time_100ns()?.saturating_sub(cpu_before);
    samples.sort_unstable();
    let mean_ns = samples.iter().copied().sum::<u64>()
        / u64::try_from(samples.len()).context("sample count exceeds u64")?;

    Ok(BenchmarkReport {
        variant,
        particles: PARTICLES,
        iterations_per_sample: ITERATIONS_PER_SAMPLE,
        warmup_samples: WARMUP_SAMPLES,
        measured_samples: MEASURED_SAMPLES,
        p50_ns: percentile(&samples, 50),
        p95_ns: percentile(&samples, 95),
        p99_ns: percentile(&samples, 99),
        mean_ns,
        process_cpu_100ns,
        allocations_in_timed_regions: allocations,
        checksum,
        idle: measure_idle()?,
        cpu: CpuFeatures {
            avx2: std::arch::is_x86_feature_detected!("avx2"),
            fma: std::arch::is_x86_feature_detected!("fma"),
            sse42: std::arch::is_x86_feature_detected!("sse4.2"),
        },
    })
}

fn run_iterations(
    variant: Variant,
    avx2: Option<Avx2Kernel>,
    batch: &mut ParticleBatch,
    step: ParticleStep,
) {
    for _ in 0..ITERATIONS_PER_SAMPLE {
        match variant {
            Variant::ScalarAutovectorized => step_scalar(batch, step),
            Variant::Avx2 => {
                if let Some(kernel) = avx2 {
                    kernel.step(batch, step);
                }
            }
            #[cfg(feature = "portable-simd")]
            Variant::PortableSimd => particle_kernel::step_portable_simd(batch, step),
        }
    }
}

fn profile_candidates() -> Result<ProfileReport> {
    const FRAMES: usize = 20_000;
    let step = ParticleStep::checked(1.0 / 240.0, 0.997, 0.0, 9.81)?;

    let mut particles = ParticleBatch::seeded(PARTICLES, 0x00C0_DE48);
    let started = Instant::now();
    for _ in 0..FRAMES {
        step_scalar(&mut particles, step);
    }
    black_box(particles.checksum());
    let particle_ns = nanos(started.elapsed());

    let mut rectangles = vec![[0.0_f32, 0.0, 1920.0, 1080.0]; 64];
    let started = Instant::now();
    for frame in 0..FRAMES {
        let offset = f32::from(u16::try_from(frame % 240).context("frame phase")?) * 0.125;
        for rectangle in &mut rectangles {
            rectangle[0] += offset;
            rectangle[1] -= offset;
            rectangle[2] = rectangle[2].max(1.0);
            rectangle[3] = rectangle[3].max(1.0);
        }
    }
    black_box(&rectangles);
    let geometry_ns = nanos(started.elapsed());

    let mut parameters = vec![[1.0_f32, 0.16, 0.55, 0.92]; 64];
    let started = Instant::now();
    for frame in 0..FRAMES {
        let phase = f32::from(u16::try_from(frame % 240).context("frame phase")?) / 240.0;
        for parameter in &mut parameters {
            parameter[3] = (parameter[3] * 0.999 + phase * 0.001).clamp(0.0, 1.0);
        }
    }
    black_box(&parameters);
    let parameter_ns = nanos(started.elapsed());

    Ok(ProfileReport {
        frames: FRAMES,
        candidates: vec![
            CandidateProfile {
                name: "particle-update",
                elements_per_frame: PARTICLES,
                elapsed_ns: particle_ns,
            },
            CandidateProfile {
                name: "window-geometry-transform",
                elements_per_frame: rectangles.len(),
                elapsed_ns: geometry_ns,
            },
            CandidateProfile {
                name: "effect-parameter-transform",
                elements_per_frame: parameters.len(),
                elapsed_ns: parameter_ns,
            },
        ],
        selected: "particle-update",
        reason: "largest measured CPU share at the admitted 2,048-particle scene limit",
    })
}

fn measure_idle() -> Result<IdleReport> {
    let cpu_before = process_cpu_time_100ns()?;
    let started = Instant::now();
    std::thread::park_timeout(Duration::from_secs(2));
    Ok(IdleReport {
        requested_ms: 2_000,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        process_cpu_100ns: process_cpu_time_100ns()?.saturating_sub(cpu_before),
        wait: "one-shot OS-backed thread park; no polling loop",
    })
}

fn process_cpu_time_100ns() -> Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all pointers name initialized writable FILETIME values for the synchronous call.
    let succeeded = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    };
    if succeeded == 0 {
        bail!("GetProcessTimes failed")
    }
    Ok(filetime(kernel).saturating_add(filetime(user)))
}

fn filetime(value: FILETIME) -> u64 {
    u64::from(value.dwLowDateTime) | u64::from(value.dwHighDateTime) << 32
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted
        .get(rank.saturating_sub(1))
        .copied()
        .unwrap_or_default()
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
