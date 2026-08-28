#![deny(unsafe_op_in_unsafe_fn)]

mod model;
mod native;
mod surface;

use std::path::PathBuf;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

use crate::model::{Inventory, MatrixReport, Scenario};

#[derive(Debug, Parser)]
#[command(about = "Disposable cover-gated motion measurement probe")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inventory {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Smoke {
        #[arg(long, default_value_t = 20)]
        windows: usize,
        #[arg(long)]
        live_limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = Scenario::Normal)]
        scenario: Scenario,
    },
    Matrix {
        #[arg(long, default_value_t = 20)]
        repetitions: usize,
        #[arg(long, value_delimiter = ',', default_value = "60,120,144,240")]
        refresh: Vec<u32>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        live_limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = Scenario::Normal)]
        scenario: Scenario,
    },
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Inventory { output } => write_inventory(output)?,
        Command::Smoke {
            windows,
            live_limit,
            scenario,
        } => surface::smoke(windows, live_limit.unwrap_or(windows), scenario)?,
        Command::Matrix {
            repetitions,
            refresh,
            output,
            live_limit,
            scenario,
        } => run_matrix(repetitions, refresh, output, live_limit, scenario)?,
    }
    Ok(())
}

fn run_matrix(
    repetitions: usize,
    refresh: Vec<u32>,
    output: PathBuf,
    live_limit: Option<usize>,
    scenario: Scenario,
) -> anyhow::Result<()> {
    let current_mode = native::current_display_mode()?;
    let available_modes = native::available_display_modes()?;
    let available_refresh = available_modes
        .iter()
        .map(|mode| mode.refresh_hz)
        .collect::<std::collections::BTreeSet<_>>();
    let unavailable_refresh_hz = refresh
        .iter()
        .copied()
        .filter(|rate| !available_refresh.contains(rate))
        .collect::<Vec<_>>();
    let mut trials = Vec::new();
    for refresh_hz in refresh
        .iter()
        .copied()
        .filter(|rate| available_refresh.contains(rate))
    {
        let lease = native::DisplayModeLease::switch_to(refresh_hz)?;
        for window_count in [20, 50] {
            for _ in 0..repetitions {
                trials.push(surface::measure_once(
                    window_count,
                    live_limit.unwrap_or(window_count),
                    scenario,
                )?);
            }
        }
        drop(lease);
    }
    let report = MatrixReport {
        current_mode,
        available_modes,
        requested_refresh_hz: refresh,
        unavailable_refresh_hz,
        repetitions,
        trials,
    };
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&output, format!("{json}\n"))
        .with_context(|| format!("write matrix {}", output.display()))?;
    println!(
        "wrote {} trials to {}",
        report.trials.len(),
        output.display()
    );
    Ok(())
}

fn write_inventory(output: Option<PathBuf>) -> anyhow::Result<()> {
    let inventory = Inventory {
        current_mode: native::current_display_mode()?,
        available_modes: native::available_display_modes()?,
        windows: native::visible_windows()?,
    };
    let json = serde_json::to_string_pretty(&inventory)?;
    if let Some(path) = output {
        std::fs::write(&path, format!("{json}\n"))
            .with_context(|| format!("write inventory {}", path.display()))?;
    } else {
        println!("{json}");
    }
    Ok(())
}
