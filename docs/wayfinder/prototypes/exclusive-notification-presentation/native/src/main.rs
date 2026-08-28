use std::fs::OpenOptions;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use exclusive_notification_presentation_prototype::model::{ProbeReport, ProducerPresentation};
use exclusive_notification_presentation_prototype::windows_notification::{
    NotificationProbe, ProbeError,
};

#[derive(Debug, Parser)]
#[command(about = "Event-driven Windows notification feasibility probe")]
struct Options {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Status,
    RequestAccess,
    Measure {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        deadline_ms: u64,
        #[arg(long, default_value = "komorebi-notification-probe")]
        marker: String,
    },
}

#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error(transparent)]
    Probe(#[from] ProbeError),
    #[error("report I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("report encoding failed: {0}")]
    Json(#[from] serde_json::Error),
}

fn main() -> Result<(), MainError> {
    let options = Options::parse();
    match options.command {
        Command::Status => status(),
        Command::RequestAccess => request_access(),
        Command::Measure {
            output,
            deadline_ms,
            marker,
        } => measure(output, Duration::from_millis(deadline_ms), &marker),
    }
}

fn status() -> Result<(), MainError> {
    let probe = NotificationProbe::connect(Duration::from_secs(10))?;
    write_stdout_json(&probe.capability_report()?)?;
    Ok(())
}

fn request_access() -> Result<(), MainError> {
    let probe = NotificationProbe::connect(Duration::from_secs(10))?;
    write_stdout_json(&probe.request_access()?)?;
    Ok(())
}

fn measure(output: PathBuf, deadline: Duration, marker: &str) -> Result<(), MainError> {
    let probe = NotificationProbe::connect(deadline)?;
    let capability = probe.capability_report()?;
    let normal = probe.measure(
        &format!("{marker}-windows-popup"),
        ProducerPresentation::WindowsPopup,
    )?;
    let suppressed = probe.measure(
        &format!("{marker}-producer-suppressed"),
        ProducerPresentation::ProducerSuppressedPopup,
    )?;
    let report = ProbeReport::new(capability, normal, suppressed, deadline);

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn write_stdout_json(value: &impl serde::Serialize) -> Result<(), MainError> {
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
