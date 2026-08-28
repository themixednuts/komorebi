use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use wayfinder_protocol_recovery_prototype::domain::InvocationId;
use wayfinder_protocol_recovery_prototype::report::{self, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() -> Result<()> {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    match arguments.next().as_deref() {
        Some(command) if command == "crash-worker" => {
            let database = PathBuf::from(required(&mut arguments, "database path")?);
            let boundary = required(&mut arguments, "crash boundary")?
                .into_string()
                .map_err(|_| anyhow!("crash boundary is not Unicode"))?;
            let id = required(&mut arguments, "invocation id")?
                .into_string()
                .map_err(|_| anyhow!("invocation id is not Unicode"))?
                .parse::<u64>()
                .context("parse invocation id")?;
            report::run_crash_worker(&database, &boundary, InvocationId::new(id))
        }
        Some(command) if command == "pipe-client" => {
            let name = required(&mut arguments, "pipe name")?;
            report::run_pipe_client(&name)
        }
        Some(command) if command == "run" => {
            let output = arguments.next().map_or_else(
                || PathBuf::from("../measurements/latest.json"),
                PathBuf::from,
            );
            write_report(&output)
        }
        Some(_) => Err(anyhow!("unknown command")),
        None => write_report(Path::new("../measurements/latest.json")),
    }
}

fn required(arguments: &mut impl Iterator<Item = OsString>, name: &str) -> Result<OsString> {
    arguments.next().ok_or_else(|| anyhow!("missing {name}"))
}

fn write_report(output: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("resolve current executable")?;
    let report = report::run(&executable)?;
    let parent = output
        .parent()
        .ok_or_else(|| anyhow!("report path has no parent"))?;
    std::fs::create_dir_all(parent).context("create report directory")?;
    let bytes = serde_json::to_vec_pretty(&report).context("encode evidence report")?;
    std::fs::write(output, bytes).context("write evidence report")?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
