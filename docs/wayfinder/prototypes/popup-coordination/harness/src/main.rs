use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use wayfinder_popup_coordination_prototype::{producer, report, uia};

#[tokio::main(flavor = "multi_thread")]
#[allow(
    clippy::unnecessary_debug_formatting,
    reason = "Debug preserves non-Unicode Windows command and path data in diagnostics"
)]
async fn main() -> Result<()> {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let Some(command) = arguments.next() else {
        bail!("expected run, producer, uia-worker, or uia-thread-victim");
    };
    match command.as_os_str() {
        value if value == OsStr::new("run") => {
            let output = PathBuf::from(
                arguments
                    .next()
                    .unwrap_or_else(|| "../measurements/latest.json".into()),
            );
            let evidence = report::run(std::env::current_exe()?).await?;
            let encoded = serde_json::to_vec_pretty(&evidence)?;
            tokio::fs::create_dir_all(
                output
                    .parent()
                    .context("measurement output must have a parent")?,
            )
            .await?;
            publish_atomically(&output, &encoded)
                .await
                .with_context(|| format!("write measurement report {output:?}"))?;
            let mut stdout = tokio::io::stdout();
            stdout.write_all(&encoded).await?;
            stdout.write_all(b"\n").await?;
        }
        value if value == OsStr::new("producer") => {
            tokio::task::spawn_blocking(producer::run).await??;
        }
        value if value == OsStr::new("uia-worker") => {
            let arguments = arguments.collect::<Vec<_>>();
            tokio::task::spawn_blocking(move || uia::run_worker(arguments.into_iter())).await??;
        }
        value if value == OsStr::new("uia-thread-victim") => {
            let arguments = arguments.collect::<Vec<_>>();
            tokio::task::spawn_blocking(move || uia::run_thread_victim(arguments.into_iter()))
                .await??;
        }
        other => bail!("unknown command {other:?}"),
    }
    Ok(())
}

async fn publish_atomically(output: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut temporary = output.as_os_str().to_os_string();
    temporary.push(format!(".{}.partial", std::process::id()));
    let temporary = PathBuf::from(temporary);
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    let mut file = options.open(&temporary).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&temporary, output).await?;
    Ok(())
}
