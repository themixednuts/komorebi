use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OVERLAPPED;
use windows_sys::Win32::System::Pipes::{
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::WindowsProgramming::{
    PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT, PROCESS_CREATION_CHILD_PROCESS_RESTRICTED,
};

use super::WIN32K_DISABLE_ALWAYS_ON;
use super::policy::ContainmentPolicy;
use super::report::{BinaryEvidence, BoundaryEvidence, WindowsPathEvidence};

pub(super) fn boundary(policy: &ContainmentPolicy) -> BoundaryEvidence {
    let job = policy.job();
    let pipe = policy.pipe();
    let process = policy.process();
    BoundaryEvidence {
        security_identity: "LPAC AppContainer identity with only configured compatibility capabilities",
        compatibility_capabilities: policy.compatibility_capabilities().to_vec(),
        all_application_packages_policy: if process.opt_out_all_application_packages() {
            PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT
        } else {
            0
        },
        process_mitigation_policy: [
            if process.disable_win32k() {
                WIN32K_DISABLE_ALWAYS_ON
            } else {
                0
            },
            0,
        ],
        child_process_policy: if process.restrict_child_processes() {
            PROCESS_CREATION_CHILD_PROCESS_RESTRICTED
        } else {
            0
        },
        resource_lifetime: "configured no-breakaway Job Object limits",
        job_active_process_limit: job.active_process_limit(),
        job_memory_limit_bytes: job.memory_limit_bytes(),
        job_cpu_hard_cap_basis_points: job.cpu_hard_cap_basis_points(),
        ipc: "host-owned named pipe with exact principal DACL and kernel peer verification",
        pipe_flags: PIPE_TYPE_BYTE
            | PIPE_READMODE_BYTE
            | PIPE_WAIT
            | PIPE_REJECT_REMOTE_CLIENTS
            | FILE_FLAG_OVERLAPPED,
        pipe_buffer_bytes: pipe.buffer_bytes(),
        maximum_frame_bytes: pipe.maximum_frame_bytes(),
        dll_search: "application directory, System32, and explicit user directories only",
        experimental_api_used: false,
        inherit_handles: false,
        create_no_window: true,
    }
}

pub(super) fn binary(role: &'static str, path: &Path) -> Result<BinaryEvidence> {
    let bytes = fs::read(path).with_context(|| format!("read binary {}", path.display()))?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let mut command = Command::new("llvm-readobj");
    command.arg("--coff-imports").arg(path);
    let imports = checked_output(command, "llvm-readobj")?;
    let mut pe_imports: Vec<_> = imports
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Name: "))
        .map(str::to_ascii_lowercase)
        .collect();
    pe_imports.sort_unstable();
    pe_imports.dedup();
    Ok(BinaryEvidence {
        role,
        path: WindowsPathEvidence::from(path),
        bytes: u64::try_from(bytes.len())?,
        sha256,
        pe_imports,
    })
}

pub(super) fn command_output(program: &str, arguments: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command.args(arguments);
    checked_output(command, program)
}

fn checked_output(mut command: Command, program: &str) -> Result<String> {
    let output = command.output().with_context(|| format!("run {program}"))?;
    let stderr = String::from_utf8(output.stderr)
        .unwrap_or_else(|error| format!("non-UTF-8 stderr bytes: {:02x?}", error.into_bytes()));
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        stderr.trim()
    );
    String::from_utf8(output.stdout)
        .with_context(|| format!("decode {program} output"))
        .map(|value| value.trim().to_owned())
}
