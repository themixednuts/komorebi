use std::{
    ffi::{OsStr, OsString},
    path::Path,
    process::Stdio,
    time::Instant,
};

use anyhow::{Context as _, Result, bail};
use tokio::{process::Command, signal};

use crate::matrix::CompilerArm;

pub(crate) struct CommandOutput {
    pub(crate) success: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) elapsed_ms: u64,
}

pub(crate) async fn rustc_version(arm: CompilerArm, current_dir: &Path) -> Result<String> {
    let output = run(
        "rustc",
        [
            OsString::from(format!("+{}", arm.toolchain())),
            OsString::from("--version"),
            OsString::from("--verbose"),
        ],
        current_dir,
        arm,
        None,
    )
    .await?;
    if !output.success {
        bail!("read rustc version for {}", arm.name());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) async fn command_text<const N: usize>(
    program: &str,
    args: [&str; N],
    current_dir: &Path,
) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(current_dir)
        .output()
        .await
        .with_context(|| format!("run {program}"))?;
    if !output.status.success() {
        bail!("{program} failed")
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) async fn run(
    program: impl AsRef<OsStr>,
    args: impl IntoIterator<Item = OsString>,
    current_dir: &Path,
    arm: CompilerArm,
    target: Option<&Path>,
) -> Result<CommandOutput> {
    let mut command = Command::new(program.as_ref());
    command
        .args(args)
        .current_dir(current_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("RUSTFLAGS");
    if let Some(flags) = arm.rustflags() {
        command.env("RUSTFLAGS", flags);
    }
    if let Some(target) = target {
        command.env("CARGO_TARGET_DIR", target);
    }
    let child = command
        .spawn()
        .with_context(|| format!("spawn {}", program.as_ref().display()))?;
    let started = Instant::now();
    let output = tokio::select! {
        output = child.wait_with_output() => output.context("wait for measurement command")?,
        interrupt = signal::ctrl_c() => {
            interrupt.context("install Ctrl+C handler")?;
            bail!("measurement cancelled")
        }
    };
    Ok(CommandOutput {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

pub(crate) async fn release_binary_size(directory: &Path) -> Result<u64> {
    let mut total = 0_u64;
    let mut entries = match tokio::fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error).context("read release directory"),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "exe") {
            total = total.saturating_add(entry.metadata().await?.len());
        }
    }
    Ok(total)
}

pub(crate) fn tail(value: &str, characters: usize) -> String {
    let tail = value.chars().rev().take(characters).collect::<String>();
    tail.chars().rev().collect()
}
