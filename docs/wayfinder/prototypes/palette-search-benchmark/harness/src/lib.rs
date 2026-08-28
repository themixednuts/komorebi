use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod benchmark;
mod catalog;
mod domain;
mod fff;
mod fixture;
mod job;
mod native;
mod protocol;
mod report;
mod root;
mod sources;
mod watcher;
mod worker;

#[derive(Debug, Parser)]
#[command(about = "Disposable Wayfinder command-palette benchmark")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long = "project-root")]
        project_roots: Vec<PathBuf>,
        #[arg(long)]
        allow_full_drive: bool,
        #[arg(long)]
        output: PathBuf,
    },
    Worker,
    ActivationProbe {
        #[arg(long)]
        event: OsString,
        #[arg(long)]
        profile: String,
    },
}

/// Parses the benchmark command and executes it inside the process's sole Tokio runtime.
///
/// # Errors
///
/// Returns an error when argument-dependent filesystem, Windows, worker, measurement, or report
/// publication work fails.
pub async fn entry() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Run {
            project_roots,
            allow_full_drive,
            output,
        } => {
            let report = benchmark::run(benchmark::BenchmarkPlan {
                project_roots,
                allow_full_drive,
                executable: std::env::current_exe()?,
            })
            .await?;
            tokio::task::spawn_blocking(move || report::publish_json(&output, &report))
                .await
                .map_err(|_| anyhow::anyhow!("report publication task failed"))??;
        }
        Command::Worker => worker::run_worker().await?,
        Command::ActivationProbe { event, profile } => {
            if profile.is_empty() {
                anyhow::bail!("activation profile must not be empty");
            }
            native::activation_probe(&event)?;
        }
    }
    Ok(())
}
