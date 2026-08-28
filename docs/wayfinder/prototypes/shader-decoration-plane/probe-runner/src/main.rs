use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::Serialize;
use tokio::{process::Command, signal};

#[derive(Serialize)]
struct ChildResult {
    executable: PathBuf,
    success: bool,
    exit_code: Option<i32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let executable = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: decoration-probe-runner <probe.exe>")?;

    let mut child = Command::new(&executable)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start {}", executable.display()))?;

    let status = tokio::select! {
        status = child.wait() => status.context("wait for probe")?,
        result = signal::ctrl_c() => {
            result.context("install Ctrl+C handler")?;
            child.kill().await.context("cancel probe")?;
            child.wait().await.context("reap cancelled probe")?
        }
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&ChildResult {
            executable,
            success: status.success(),
            exit_code: status.code(),
        })?
    );
    Ok(())
}
