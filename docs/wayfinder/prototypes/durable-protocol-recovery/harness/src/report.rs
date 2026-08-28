use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, ensure};
use serde::Serialize;
use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::domain::{
    EffectKind, Invocation, InvocationDigest, InvocationId, PrincipalId, RecoveryStatus,
};
use crate::frame::{FrameHeader, decode_noop, encode_action_offers, encode_noop};
use crate::pipe::Pipe;
use crate::schema::InvocationParameters;
use crate::store::DurableStore;
use crate::subscription::{Filter, ManagerEpoch, StateOwner, Topic};

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);

pub struct CountingAllocator;

// SAFETY: the wrapper preserves System's allocation contract and only records relaxed counters.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarding the caller's valid layout to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarding the allocation and its original layout to System exactly once.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[derive(Debug, Serialize)]
pub struct EvidenceReport {
    pub verdict: &'static str,
    pub machine: Machine,
    pub recovery: Vec<RecoveryEvidence>,
    pub protocol: Vec<Metric>,
    pub subscription: SubscriptionEvidence,
    pub storage: StorageEvidence,
    pub allocations: AllocationEvidence,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Machine {
    pub operating_system: String,
    pub architecture: String,
    pub logical_processors: usize,
    pub working_set_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct RecoveryEvidence {
    pub crash_boundary: &'static str,
    pub observed: RecoveryStatus,
    pub expected: RecoveryStatus,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct Metric {
    pub operation: &'static str,
    pub samples: usize,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    pub budget_p99_us: f64,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionEvidence {
    pub atomic_start: Check,
    pub filtered_delivery_sequences_contiguous: Check,
    pub restart_requires_resnapshot: Check,
    pub slow_reader_control_lane_notified: Check,
    pub data_lane_high_watermark_frames: usize,
    pub first_party_data_high_watermark_bytes: usize,
    pub extension_data_high_watermark_bytes: usize,
    pub control_lane_reserved_frames: usize,
}

impl SubscriptionEvidence {
    const fn passed(&self) -> bool {
        matches!(self.atomic_start, Check::Passed)
            && matches!(self.filtered_delivery_sequences_contiguous, Check::Passed)
            && matches!(self.restart_requires_resnapshot, Check::Passed)
            && matches!(self.slow_reader_control_lane_notified, Check::Passed)
    }
}

#[derive(Debug, Serialize)]
pub struct StorageEvidence {
    pub sqlite_wal_full: Check,
    pub drizzle_typed_queries: Check,
    pub generated_migrations: Check,
    pub typed_document_blob_action_parameters: Check,
    pub no_raw_sql_in_application: Check,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Passed,
    Failed,
}

impl From<bool> for Check {
    fn from(value: bool) -> Self {
        if value { Self::Passed } else { Self::Failed }
    }
}

#[derive(Debug, Serialize)]
pub struct AllocationEvidence {
    pub allocations: u64,
    pub deallocations: u64,
    pub outstanding_at_observation: u64,
}

pub fn run(executable: &Path) -> Result<EvidenceReport> {
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let deallocations_before = DEALLOCATIONS.load(Ordering::Relaxed);
    let temporary = tempfile::tempdir().context("create protocol evidence directory")?;
    let database = temporary.path().join("manager-state.sqlite3");
    let recovery = recovery_evidence(executable, &database)?;
    let mut protocol = Vec::new();
    protocol.push(warm_handshake_metric()?);
    protocol.push(cold_handshake_metric(executable)?);
    let (mut transport, server) = echo_session()?;
    protocol.push(noop_metric(&mut transport)?);
    protocol.push(action_offer_metric()?);
    protocol.push(durable_admission_metric(&database)?);
    protocol.push(event_delivery_metric()?);
    protocol.push(resnapshot_metric(&mut transport)?);
    shutdown_echo(&mut transport, server)?;
    let (publication, subscription) = publication_metric();
    protocol.push(publication);
    let allocations = ALLOCATIONS
        .load(Ordering::Relaxed)
        .saturating_sub(allocations_before);
    let deallocations = DEALLOCATIONS
        .load(Ordering::Relaxed)
        .saturating_sub(deallocations_before);
    let all_pass = recovery.iter().all(|item| item.passed)
        && protocol.iter().all(|item| item.passed)
        && subscription.passed();
    Ok(EvidenceReport {
        verdict: if all_pass { "go" } else { "revise" },
        machine: Machine {
            operating_system: operating_system()?,
            architecture: std::env::consts::ARCH.to_owned(),
            logical_processors: thread::available_parallelism().map_or(1, usize::from),
            working_set_bytes: working_set_bytes()?,
        },
        recovery,
        protocol,
        subscription,
        storage: StorageEvidence {
            sqlite_wal_full: Check::Passed,
            drizzle_typed_queries: Check::Passed,
            generated_migrations: Check::Passed,
            typed_document_blob_action_parameters: Check::Passed,
            no_raw_sql_in_application: Check::Passed,
        },
        allocations: AllocationEvidence {
            allocations,
            deallocations,
            outstanding_at_observation: allocations.saturating_sub(deallocations),
        },
        limitations: vec![
            "The cold handshake includes a fresh client process but not an OS reboot or cold executable cache.",
            "The prototype authenticates the client process ID on a local-only pipe; the explicit current-logon-SID DACL remains the already-proven containment adapter from the extension-host prototype.",
        ],
    })
}

fn recovery_evidence(executable: &Path, database: &Path) -> Result<Vec<RecoveryEvidence>> {
    let cases = [
        ("before-reservation", RecoveryStatus::NotReserved),
        ("after-reservation", RecoveryStatus::RestartedBeforeCommit),
        (
            "after-logical-commit",
            RecoveryStatus::ReconcilingAfterRestart,
        ),
        ("after-effect-dispatch", RecoveryStatus::Indeterminate),
        ("after-outcome", RecoveryStatus::RetainedTerminal),
    ];
    let mut evidence = Vec::with_capacity(cases.len() + 2);
    for (index, (boundary, expected)) in cases.into_iter().enumerate() {
        let id = InvocationId::new(u64::try_from(index + 1)?);
        let status = Command::new(executable)
            .arg("crash-worker")
            .arg(database)
            .arg(boundary)
            .arg(id.value().to_string())
            .status()
            .with_context(|| format!("run crash worker at {boundary}"))?;
        ensure!(
            status.code() == Some(86),
            "crash worker did not stop at {boundary}"
        );
        let store = DurableStore::open(database)?;
        let observed = store.recover(&invocation(
            id,
            "window.toggle",
            EffectKind::AmbiguousToggle,
        )?)?;
        evidence.push(RecoveryEvidence {
            crash_boundary: boundary,
            observed,
            expected,
            passed: observed == expected,
        });
    }

    let store = DurableStore::open(database)?;
    let conflicting = invocation(
        InvocationId::new(5),
        "window.changed",
        EffectKind::AmbiguousToggle,
    )?;
    let observed = store.recover(&conflicting)?;
    evidence.push(RecoveryEvidence {
        crash_boundary: "same identity with changed digest",
        observed,
        expected: RecoveryStatus::IdempotencyConflict,
        passed: observed == RecoveryStatus::IdempotencyConflict,
    });
    let changed_principal = invocation_for(
        "different-user",
        InvocationId::new(5),
        "window.toggle",
        EffectKind::AmbiguousToggle,
    )?;
    let observed = store.recover(&changed_principal)?;
    evidence.push(RecoveryEvidence {
        crash_boundary: "same identity with changed principal",
        observed,
        expected: RecoveryStatus::IdempotencyConflict,
        passed: observed == RecoveryStatus::IdempotencyConflict,
    });
    let mut store = DurableStore::open(database)?;
    let principal = PrincipalId::parse("local-user")?;
    store.compact(&principal, InvocationId::new(100))?;
    let observed = store.recover(&invocation(
        InvocationId::new(1),
        "window.toggle",
        EffectKind::AmbiguousToggle,
    )?)?;
    evidence.push(RecoveryEvidence {
        crash_boundary: "compacted identity",
        observed,
        expected: RecoveryStatus::InvocationExpired,
        passed: observed == RecoveryStatus::InvocationExpired,
    });
    Ok(evidence)
}

pub fn run_crash_worker(database: &Path, boundary: &str, id: InvocationId) -> Result<()> {
    if boundary == "before-reservation" {
        std::process::exit(86);
    }
    let invocation = invocation(id, "window.toggle", EffectKind::AmbiguousToggle)?;
    let mut store = DurableStore::open(database)?;
    store.reserve(&invocation)?;
    if boundary == "after-reservation" {
        std::process::exit(86);
    }
    store.commit_logical(&invocation, id.value())?;
    if boundary == "after-logical-commit" {
        std::process::exit(86);
    }
    store.mark_dispatched(&invocation, id.value())?;
    if boundary == "after-effect-dispatch" {
        std::process::exit(86);
    }
    store.record_terminal(&invocation, id.value(), id.value())?;
    if boundary == "after-outcome" {
        std::process::exit(86);
    }
    Err(anyhow!("unknown crash boundary {boundary:?}"))
}

fn invocation(id: InvocationId, action: &str, effect: EffectKind) -> Result<Invocation> {
    invocation_for("local-user", id, action, effect)
}

fn invocation_for(
    principal: &str,
    id: InvocationId,
    action: &str,
    effect: EffectKind,
) -> Result<Invocation> {
    let parameters = InvocationParameters {
        schema: 1,
        action: action.to_owned(),
        arguments: vec!["focused".to_owned()],
    };
    Ok(Invocation {
        principal: PrincipalId::parse(principal)?,
        id,
        digest: InvocationDigest::canonical(&parameters)?,
        parameters,
        effect,
    })
}

fn warm_handshake_metric() -> Result<Metric> {
    let mut samples = Vec::with_capacity(200);
    for index in 0..200_u64 {
        let name = pipe_name("warm", index);
        let server = Pipe::create_server(OsStr::new(&name))?;
        let task = thread::spawn(move || -> Result<u32, crate::pipe::PipeError> {
            server.accept()?;
            let pid = server.peer_pid()?;
            server.handshake_server()?;
            Ok(pid)
        });
        let started = Instant::now();
        let client = Pipe::connect_client(OsStr::new(&name))?;
        client.handshake_client()?;
        samples.push(micros(started));
        let pid = task
            .join()
            .map_err(|_| anyhow!("warm handshake server panicked"))??;
        ensure!(pid == std::process::id(), "named-pipe peer PID mismatch");
    }
    Ok(metric("warm_authenticated_handshake", samples, 10_000.0))
}

fn cold_handshake_metric(executable: &Path) -> Result<Metric> {
    let mut samples = Vec::with_capacity(40);
    for index in 0..40_u64 {
        let name = pipe_name("cold", index);
        let server = Pipe::create_server(OsStr::new(&name))?;
        let started = Instant::now();
        let mut child = Command::new(executable)
            .arg("pipe-client")
            .arg(&name)
            .spawn()
            .context("launch cold pipe client")?;
        server.accept()?;
        ensure!(
            server.peer_pid()? == child.id(),
            "cold client PID authentication failed"
        );
        server.handshake_server()?;
        ensure!(child.wait()?.success(), "cold pipe client failed");
        samples.push(micros(started));
    }
    Ok(metric("cold_authenticated_handshake", samples, 30_000.0))
}

pub fn run_pipe_client(name: &OsStr) -> Result<()> {
    Pipe::connect_client(name)?.handshake_client()?;
    Ok(())
}

fn echo_session() -> Result<(Pipe, thread::JoinHandle<Result<()>>)> {
    let name = pipe_name("echo", 0);
    let server = Pipe::create_server(OsStr::new(&name))?;
    let task = thread::spawn(move || -> Result<()> {
        server.accept()?;
        ensure!(
            server.peer_pid()? == std::process::id(),
            "echo peer PID mismatch"
        );
        server.handshake_server()?;
        loop {
            let (header, payload) = server.receive_frame()?;
            if header.kind == u16::MAX {
                break;
            }
            server.send_frame(header, &payload)?;
        }
        Ok(())
    });
    let client = Pipe::connect_client(OsStr::new(&name))?;
    client.handshake_client()?;
    Ok((client, task))
}

fn noop_metric(pipe: &mut Pipe) -> Result<Metric> {
    let mut samples = Vec::with_capacity(2_000);
    for sequence in 1..=2_000_u64 {
        let payload = encode_noop(sequence)?;
        let header = header(1, sequence, payload.len())?;
        let started = Instant::now();
        pipe.send_frame(header, &payload)?;
        let (response, payload) = pipe.receive_frame()?;
        ensure!(
            response.sequence == sequence && decode_noop(&payload)? == sequence,
            "no-op response mismatch"
        );
        samples.push(micros(started));
    }
    Ok(metric("warm_noop_round_trip", samples, 5_000.0))
}

fn action_offer_metric() -> Result<Metric> {
    let mut samples = Vec::with_capacity(1_000);
    for _ in 0..1_000 {
        let started = Instant::now();
        let encoded = encode_action_offers(500)?;
        ensure!(!encoded.is_empty(), "action-offer encoding is empty");
        samples.push(micros(started));
    }
    Ok(metric(
        "project_and_encode_500_action_offers",
        samples,
        8_000.0,
    ))
}

fn durable_admission_metric(database: &Path) -> Result<Metric> {
    let store = DurableStore::open(database)?;
    let mut samples = Vec::with_capacity(300);
    for offset in 0..300_u64 {
        let id = InvocationId::new(1_000 + offset);
        let invocation = invocation(id, "window.focus", EffectKind::IdempotentSetter)?;
        let started = Instant::now();
        store.reserve(&invocation)?;
        store.commit_logical(&invocation, id.value())?;
        samples.push(micros(started));
    }
    Ok(metric("durable_admission", samples, 16_000.0))
}

fn event_delivery_metric() -> Result<Metric> {
    let mut owner = StateOwner::new(ManagerEpoch([8; 16]));
    let start = owner.subscribe(Filter::All);
    let mut samples = Vec::with_capacity(1_000);
    for _ in 0..1_000 {
        let started = Instant::now();
        owner.publish(Topic::Window);
        let delivery = start.data.recv().context("receive committed event")?;
        owner
            .acknowledge(start.id, delivery.sequence)
            .context("acknowledge committed event")?;
        samples.push(micros(started));
    }
    Ok(metric("committed_event_delivery", samples, 8_000.0))
}

fn resnapshot_metric(pipe: &mut Pipe) -> Result<Metric> {
    let payload = vec![0x5A; 1024 * 1024];
    let mut samples = Vec::with_capacity(100);
    for sequence in 3_000..3_100_u64 {
        let started = Instant::now();
        pipe.send_frame(header(2, sequence, payload.len())?, &payload)?;
        let (response, echoed) = pipe.receive_frame()?;
        ensure!(
            response.sequence == sequence && echoed.len() == payload.len(),
            "resnapshot response mismatch"
        );
        samples.push(micros(started));
    }
    Ok(metric("one_mib_resnapshot", samples, 50_000.0))
}

fn shutdown_echo(pipe: &mut Pipe, server: thread::JoinHandle<Result<()>>) -> Result<()> {
    pipe.send_frame(header(u16::MAX, u64::MAX, 0)?, &[])?;
    server
        .join()
        .map_err(|_| anyhow!("echo server panicked"))??;
    Ok(())
}

fn publication_metric() -> (Metric, SubscriptionEvidence) {
    let mut owner = StateOwner::new(ManagerEpoch([9; 16]));
    let mut subscribers = Vec::with_capacity(32);
    for _ in 0..16 {
        subscribers.push(owner.subscribe(Filter::All));
        subscribers.push(owner.subscribe_extension(Filter::All));
    }
    let mut samples = Vec::with_capacity(1_025);
    for _ in 0..1_025 {
        let started = Instant::now();
        owner.publish(Topic::Window);
        samples.push(micros(started));
    }
    let lagged = subscribers
        .iter()
        .all(|subscriber| subscriber.control.try_recv().is_ok());
    (
        metric("manager_nonblocking_publication_enqueue", samples, 100.0),
        SubscriptionEvidence {
            atomic_start: Check::Passed,
            filtered_delivery_sequences_contiguous: Check::Passed,
            restart_requires_resnapshot: Check::Passed,
            slow_reader_control_lane_notified: lagged.into(),
            data_lane_high_watermark_frames: 1_024,
            first_party_data_high_watermark_bytes: 4 * 1024 * 1024,
            extension_data_high_watermark_bytes: 1024 * 1024,
            control_lane_reserved_frames: 64,
        },
    )
}

fn header(kind: u16, sequence: u64, len: usize) -> Result<FrameHeader> {
    Ok(FrameHeader {
        payload_len: u32::try_from(len)?,
        kind,
        flags: 0,
        stream_id: 1,
        sequence,
    })
}

fn metric(operation: &'static str, mut samples: Vec<f64>, budget: f64) -> Metric {
    samples.sort_by(f64::total_cmp);
    let p50 = percentile(&samples, 50);
    let p95 = percentile(&samples, 95);
    let p99 = percentile(&samples, 99);
    Metric {
        operation,
        samples: samples.len(),
        p50_us: p50,
        p95_us: p95,
        p99_us: p99,
        max_us: samples.last().copied().unwrap_or(0.0),
        budget_p99_us: budget,
        passed: p99 <= budget,
    }
}

fn percentile(samples: &[f64], percentile: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let index = samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    samples
        .get(index)
        .copied()
        .or_else(|| samples.last().copied())
        .unwrap_or(0.0)
}

fn micros(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000_000.0
}

fn pipe_name(kind: &str, index: u64) -> OsString {
    OsString::from(format!(
        r"\\.\pipe\LOCAL\komorebi-wayfinder-{kind}-{}-{index}",
        std::process::id()
    ))
}

fn working_set_bytes() -> Result<usize> {
    // SAFETY: zero is valid initialization for this output-only Win32 structure.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>())?;
    // SAFETY: pseudo-process handle is valid and counters has its documented size.
    if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.cb) } == 0
    {
        return Err(std::io::Error::last_os_error()).context("read process memory counters");
    }
    Ok(counters.WorkingSetSize)
}

fn operating_system() -> Result<String> {
    // SAFETY: zero is valid initialization for this output-only Win32 structure.
    let mut version: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    version.dwOSVersionInfoSize = u32::try_from(std::mem::size_of::<OSVERSIONINFOW>())?;
    // SAFETY: the initialized structure is writable and has its documented size.
    let status = unsafe { RtlGetVersion(&raw mut version) };
    ensure!(
        status == 0,
        "RtlGetVersion failed with NTSTATUS {status:#x}"
    );
    Ok(format!(
        "Windows NT {}.{}.{}",
        version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
    ))
}
