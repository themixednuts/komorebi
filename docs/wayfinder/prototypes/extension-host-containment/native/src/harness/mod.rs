use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::time::{Duration, Instant};

use crate::protocol::{ExpectedOutcome, ObservedOutcome, RuntimeKind};
use crate::windows::{OwnedHandle, windows_version};
use anyhow::{Context, Result, bail, ensure};
use mlua::{ChunkMode, Lua, LuaOptions, StdLib};
use uuid::Uuid;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_DESKTOP,
    JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
    JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};

mod af_unix;
mod backpressure;
mod environment;
mod evidence;
mod fault;
mod ipc;
mod launch;
mod lifetime;
mod policy;
mod recovery;
mod report;
mod responsiveness;
mod session;
mod windows_boundary;

use crate::protocol::ParentExitMode;
use af_unix::run_comparison as run_af_unix_comparison;
use backpressure::run as run_backpressure;
use evidence::{binary as binary_evidence, boundary as boundary_evidence, command_output};
use fault::run as run_faults;
use launch::{ExtensionBehavior, launch as launch_extension};
use lifetime::run_suite as run_parent_lifetime;
use policy::ContainmentPolicy;
use recovery::run as run_restart_recovery;
use report::{
    CleanupEvidence, HarnessReport, InvocationEvidence, PlatformEvidence, RunReport, ScaleReport,
    SharedHostControl, ToolchainEvidence, Verification, WindowsPathEvidence,
};
use responsiveness::run as run_host_responsiveness;
use session::serve as serve_session;
use windows_boundary::{delete_profile, profile_cleanup_succeeded};

pub(super) const WIN32K_DISABLE_ALWAYS_ON: u64 = 0x0000_0000_1000_0000;
/// Runs the complete extension-containment evidence suite or an explicit diagnostic command.
///
/// # Errors
///
/// Returns an error when a required Windows control, probe, measurement, or cleanup step fails.
pub fn run() -> Result<()> {
    let policy_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("containment-policy.json");
    let policy = ContainmentPolicy::load(&policy_path)?;
    if run_diagnostic(&policy)? {
        return Ok(());
    }

    run_evidence(&policy)
}

fn run_diagnostic(policy: &ContainmentPolicy) -> Result<bool> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => Ok(false),
        [flag, profile_name] if flag == OsStr::new("--delete-profile") => {
            delete_profile(
                profile_name
                    .to_str()
                    .context("profile name is not valid Unicode")?,
                policy,
            )?;
            Ok(true)
        }
        [flag, mode, _nonce] if flag == OsStr::new("--lifetime-parent") => {
            let mode = mode
                .to_str()
                .context("parent exit mode is not valid Unicode")?
                .parse::<ParentExitMode>()?;
            lifetime::run_parent(mode, policy)?;
            Ok(true)
        }
        [flag, path, samples, timeout] if flag == OsStr::new("--af-unix-client") => {
            let samples = samples
                .to_str()
                .context("AF_UNIX sample count is not valid Unicode")?
                .parse::<usize>()?;
            let timeout = timeout
                .to_str()
                .context("AF_UNIX timeout is not valid Unicode")?
                .parse::<u64>()?;
            af_unix::run_client(Path::new(path), samples, Duration::from_millis(timeout))?;
            Ok(true)
        }
        _ => bail!("unexpected command-line arguments"),
    }
}

fn run_evidence(policy: &ContainmentPolicy) -> Result<()> {
    let started = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    let executable_dir = std::env::current_exe()?
        .parent()
        .context("host executable has no parent")?
        .to_path_buf();
    let prototype_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("manifest has no prototype root")?
        .to_path_buf();
    let results_dir = prototype_root.join("results");
    fs::create_dir_all(&results_dir)?;
    let private_file = results_dir.join(format!("host-private-{}.txt", Uuid::new_v4()));
    fs::write(&private_file, b"must not be readable from LPAC")?;

    let rust = if std::env::var_os("WAYFINDER_DIAGNOSTIC_MINIMAL").is_some() {
        executable_dir.join("containment-minimal-child.exe")
    } else {
        executable_dir.join("containment-rust-child.exe")
    };
    let lua = executable_dir.join("containment-lua-child.exe");
    let fault = executable_dir.join("containment-fault-child.exe");
    ensure!(
        rust.is_file() && lua.is_file() && fault.is_file(),
        "build all release binaries first: cargo build --release --bins"
    );
    let version = windows_version()?;
    let binaries = collect_binary_evidence(&executable_dir, &lua, &fault)?;

    let mut runs = Vec::new();
    for (runtime, executable) in [(RuntimeKind::Rust, rust), (RuntimeKind::LuaJit, lua)] {
        runs.push(run_extension(runtime, &executable, &private_file, policy)?);
    }
    let af_unix = run_af_unix_comparison(&std::env::current_exe()?, policy, &runs)?;
    let faults = run_faults(&fault, &private_file, policy)?;
    let host_responsiveness = run_host_responsiveness(&fault, &private_file, policy)?;
    let backpressure = run_backpressure(&fault, &private_file, policy)?;
    let parent_lifetime = run_parent_lifetime(&executable_dir, &private_file, policy)?;
    let restart_recovery = run_restart_recovery(
        &executable_dir.join("containment-rust-child.exe"),
        &private_file,
        policy,
    )?;
    let scale = run_scale_cohorts(
        &executable_dir.join("containment-rust-child.exe"),
        &private_file,
        policy,
    )?;
    let shared_host_control = run_shared_host_control(policy)?;
    fs::remove_file(&private_file)?;
    let report = HarnessReport {
        generated_at_unix_ms: started,
        platform: PlatformEvidence {
            windows_major: version.major,
            windows_minor: version.minor,
            windows_build: version.build,
            architecture: std::env::consts::ARCH,
        },
        toolchain: ToolchainEvidence {
            rustc_verbose_version: command_output("rustc", &["-Vv"])?,
            cargo_version: command_output("cargo", &["-V"])?,
            cargo_dependency_tree: command_output(
                "cargo",
                &["tree", "--locked", "--prefix", "none", "--edges", "normal"],
            )?,
        },
        invocation: InvocationEvidence {
            working_directory: WindowsPathEvidence::from(prototype_root.as_path()),
            command: r".\native\run.ps1",
            rustflags: "-C target-feature=+crt-static",
        },
        binaries,
        boundary: boundary_evidence(policy),
        runs,
        af_unix,
        faults,
        host_responsiveness,
        backpressure,
        parent_lifetime,
        restart_recovery,
        scale,
        shared_host_control,
        cleanup: CleanupEvidence {
            profiles_deleted: profile_cleanup_succeeded(),
            private_file_deleted: !private_file.exists(),
            pipe_handles_closed: true,
        },
    };
    let output = serde_json::to_vec_pretty(&report)?;
    fs::write(results_dir.join("latest.json"), &output)?;
    println!("{}", String::from_utf8(output)?);
    Ok(())
}

fn collect_binary_evidence(
    executable_dir: &Path,
    lua: &Path,
    fault: &Path,
) -> Result<Vec<report::BinaryEvidence>> {
    [
        ("host", std::env::current_exe()?),
        (
            "rust_child",
            executable_dir.join("containment-rust-child.exe"),
        ),
        ("lua_jit_child", lua.to_path_buf()),
        ("fault_child", fault.to_path_buf()),
    ]
    .into_iter()
    .map(|(role, path)| binary_evidence(role, &path))
    .collect()
}

fn run_extension(
    runtime: RuntimeKind,
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<RunReport> {
    let generation = policy.workload().generation();
    let mut extension = launch_extension(
        runtime,
        executable,
        private_file,
        policy,
        ExtensionBehavior::Normal,
        generation,
    )?;

    let session = serve_session(
        &mut extension.channel,
        generation,
        extension.process.raw(),
        &extension.error_file,
        policy,
    )?;
    let pipe_policy = policy.pipe();
    // SAFETY: process handle is valid and timeout is bounded.
    let exit_observed = unsafe {
        WaitForSingleObject(
            extension.process.raw(),
            u32::try_from(pipe_policy.operation_timeout().as_millis())?,
        )
    } == WAIT_OBJECT_0;
    Ok(RunReport {
        runtime,
        profile_name: extension.profile_name.clone(),
        expected_pid: extension.process_id,
        pipe_reported_pid: extension.pipe_pid,
        pipe_acl_sddl: extension.pipe_acl_sddl.clone(),
        foreign_profile_sid: extension.foreign_profile_sid.clone(),
        reparse_link_created: Verification::from(extension.reparse_link_created),
        startup_ms: extension.startup_ms,
        private_commit_bytes: extension.private_commit_bytes,
        in_expected_job: Verification::from(extension.in_expected_job),
        facts: extension.facts.clone(),
        probes: session.probes,
        echo_rtt_us: session.echo_rtt_us,
        broker_service_us: session.broker_service_us,
        storage_cas_roundtrip: session.storage_cas_roundtrip,
        brokered_http_status: session.brokered_http_status,
        stale_generation_rejected: session.stale_generation_rejected,
        exit_observed: Verification::from(exit_observed),
    })
}

fn trace(stage: &str) {
    if std::env::var_os("WAYFINDER_TRACE").as_deref() == Some(std::ffi::OsStr::new("1")) {
        eprintln!("{stage}");
    }
}

fn child_error_detail(
    process: windows_sys::Win32::Foundation::HANDLE,
    error_file: &Path,
) -> String {
    let mut exit_code = 0_u32;
    // SAFETY: process remains valid while the authenticated extension is active.
    let status = if unsafe { GetExitCodeProcess(process, &raw mut exit_code) } == 0 {
        "exit code unavailable".to_owned()
    } else {
        format!("exit code {exit_code:#x}")
    };
    let trace = fs::read_to_string(error_file)
        .unwrap_or_else(|_| "child emitted no diagnostic trace".to_owned());
    format!("{status}\n{trace}")
}

fn create_restricted_job(policy: policy::JobPolicy) -> Result<OwnedHandle> {
    // SAFETY: null attributes/name create a private job object.
    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | if policy.kill_on_close() {
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        } else {
            0
        };
    limits.BasicLimitInformation.ActiveProcessLimit = policy.active_process_limit();
    limits.JobMemoryLimit = policy.memory_limit_bytes();
    set_job(job.raw(), JobObjectExtendedLimitInformation, &limits)?;
    let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
        ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        ..Default::default()
    };
    // SAFETY: CpuRate is the active union field for hard-cap mode.
    cpu.Anonymous.CpuRate = policy.cpu_hard_cap_basis_points();
    set_job(job.raw(), JobObjectCpuRateControlInformation, &cpu)?;
    if policy.ui_restrictions() {
        let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
        };
        set_job(job.raw(), JobObjectBasicUIRestrictions, &ui)?;
    }
    Ok(job)
}

fn set_job<T>(job: windows_sys::Win32::Foundation::HANDLE, class: i32, value: &T) -> Result<()> {
    // SAFETY: value matches class and is readable for its advertised size.
    if unsafe {
        SetInformationJobObject(
            job,
            class,
            std::ptr::from_ref(value).cast(),
            u32::try_from(size_of::<T>())?,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("set Job Object policy");
    }
    Ok(())
}

fn private_commit(process: windows_sys::Win32::Foundation::HANDLE) -> Result<usize> {
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX>())?,
        ..Default::default()
    };
    // SAFETY: memory is writable for cb bytes and process is queryable.
    if unsafe { K32GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb) } == 0 {
        return Err(std::io::Error::last_os_error()).context("query child private commit");
    }
    Ok(memory.PrivateUsage)
}

fn broker_http(url: &str) -> Result<(u16, usize)> {
    ensure!(
        url == "http://example.com/",
        "URL is outside the prototype allowlist"
    );
    let address = ("example.com", 80)
        .to_socket_addrs()?
        .next()
        .context("host DNS returned no example.com address")?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(
        b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\nUser-Agent: komorebi-wayfinder/0\r\n\r\n",
    )?;
    let mut response = Vec::new();
    stream.take(1024 * 1024).read_to_end(&mut response)?;
    let first_line = response
        .split(|byte| *byte == b'\n')
        .next()
        .context("HTTP response has no status line")?;
    let status = std::str::from_utf8(first_line)?
        .split_whitespace()
        .nth(1)
        .context("HTTP response has no status")?
        .parse()?;
    Ok((status, response.len()))
}

fn run_scale_cohorts(
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<Vec<ScaleReport>> {
    let mut reports = Vec::new();
    for process_count in policy.workload().cohort_sizes() {
        let cohort_started = Instant::now();
        let mut workers = Vec::with_capacity(process_count);
        for _ in 0..process_count {
            let executable = executable.to_path_buf();
            let private_file = private_file.to_path_buf();
            let policy = policy.clone();
            workers.push(std::thread::spawn(move || {
                run_extension(RuntimeKind::Rust, &executable, &private_file, &policy)
            }));
        }
        let mut runs = Vec::with_capacity(process_count);
        for worker in workers {
            runs.push(
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("scale worker panicked"))??,
            );
        }
        let mut ready: Vec<_> = runs.iter().map(|run| run.startup_ms).collect();
        let mut rtt: Vec<_> = runs
            .iter()
            .flat_map(|run| run.echo_rtt_us.iter().copied())
            .collect();
        ready.sort_by(f64::total_cmp);
        rtt.sort_by(f64::total_cmp);
        reports.push(ScaleReport {
            process_count,
            cohort_wall_ms: cohort_started.elapsed().as_secs_f64() * 1_000.0,
            authenticated_ready_p50_ms: percentile(&ready, 50, 100),
            authenticated_ready_p99_ms: percentile(&ready, 99, 100),
            aggregate_private_commit_bytes: runs.iter().map(|run| run.private_commit_bytes).sum(),
            echo_rtt_p99_us: percentile(&rtt, 99, 100),
            forbidden_probes_allowed: runs
                .iter()
                .flat_map(|run| &run.probes)
                .filter(|probe| {
                    matches!(probe.expected, ExpectedOutcome::Denied)
                        && matches!(probe.observed, ObservedOutcome::Allowed)
                })
                .count(),
            all_exited: runs.iter().all(|run| run.exit_observed.passed()),
        });
    }
    Ok(reports)
}

fn run_shared_host_control(policy: &ContainmentPolicy) -> Result<SharedHostControl> {
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle for the current process.
    let process = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() };
    let before = private_commit(process)?;
    let started = Instant::now();
    let workload = policy.workload();
    let mut contexts = Vec::with_capacity(workload.shared_host_contexts());
    for _ in 0..workload.shared_host_contexts() {
        let lua = Lua::new_with(StdLib::NONE, LuaOptions::default())
            .map_err(|error| anyhow::anyhow!("create shared-host LuaJIT control: {error}"))?;
        lua.set_memory_limit(64 * 1024 * 1024)
            .map_err(|error| anyhow::anyhow!("limit shared-host LuaJIT control: {error}"))?;
        let marker: u64 = lua
            .load("return 1")
            .set_mode(ChunkMode::Text)
            .eval()
            .map_err(|error| anyhow::anyhow!("activate shared-host LuaJIT control: {error}"))?;
        ensure!(marker == 1, "shared-host LuaJIT activation mismatch");
        contexts.push(lua);
    }
    let cohort_startup_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after = private_commit(process)?;
    let mut noops = Vec::new();
    for lua in &contexts {
        let function: mlua::Function = lua
            .load("return function(value) return value end")
            .set_mode(ChunkMode::Text)
            .eval()
            .map_err(|error| anyhow::anyhow!("create shared-host no-op: {error}"))?;
        for value in 0..u64::try_from(workload.shared_host_noop_samples())? {
            let call_started = Instant::now();
            let returned: u64 = function
                .call(value)
                .map_err(|error| anyhow::anyhow!("call shared-host no-op: {error}"))?;
            std::hint::black_box(returned);
            noops.push(call_started.elapsed().as_secs_f64() * 1_000_000.0);
        }
    }
    noops.sort_by(f64::total_cmp);
    Ok(SharedHostControl {
        lua_contexts: contexts.len(),
        cohort_startup_ms,
        incremental_private_commit_bytes: after.saturating_sub(before),
        in_process_noop_p99_us: percentile(&noops, 99, 100),
        blast_radius_extensions: contexts.len(),
        isolation_boundary: "none: one native crash or memory corruption terminates every context",
    })
}

pub(super) fn percentile(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    debug_assert!(denominator > 0 && numerator <= denominator);
    let scaled = (sorted.len() - 1).saturating_mul(numerator);
    let index = scaled.saturating_add(denominator - 1) / denominator;
    sorted[index.min(sorted.len() - 1)]
}
