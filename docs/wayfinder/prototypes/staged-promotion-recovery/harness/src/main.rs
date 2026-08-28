use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use wayfinder_promotion_prototype::candidate;
use wayfinder_promotion_prototype::domain::{Boundary, FaultProfile};
use wayfinder_promotion_prototype::installation::Layout;
use wayfinder_promotion_prototype::promotion::{self, CrashAfter, PromotionError};
use wayfinder_promotion_prototype::scenarios;

const PROCESS_DEATH_EXIT_CODE: u8 = 86;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error)
            if error
                .downcast_ref::<PromotionError>()
                .is_some_and(|source| {
                    matches!(source, PromotionError::InjectedProcessDeath(_))
                }) =>
        {
            ExitCode::from(PROCESS_DEATH_EXIT_CODE)
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args_os();
    let _executable = arguments.next().ok_or_else(|| anyhow!("missing argv[0]"))?;
    let command = strict_text(arguments.next(), "command")?;
    match command.as_str() {
        "candidate" => {
            let root = required_path(&mut arguments, "candidate root")?;
            let fault = parse_text::<FaultProfile>(&mut arguments, "fault profile")?;
            reject_extra(arguments)?;
            candidate::serve(&Layout::new(root), fault).context("serve candidate")
        }
        "attempt" => {
            let root = required_path(&mut arguments, "promotion root")?;
            let fault = parse_text::<FaultProfile>(&mut arguments, "fault profile")?;
            let deadline_ms = parse_text::<u64>(&mut arguments, "health deadline milliseconds")?;
            let crash_after = optional_text::<Boundary>(&mut arguments, "crash boundary")?
                .map_or_else(CrashAfter::never, CrashAfter::boundary);
            reject_extra(arguments)?;
            let executable = std::env::current_exe().context("resolve current executable")?;
            let layout = Layout::new(root);
            let identity = layout
                .initialize(fault)
                .context("initialize promotion fixture")?;
            let outcome = promotion::attempt(
                &executable,
                &layout,
                &identity,
                Duration::from_millis(deadline_ms),
                crash_after,
            )?;
            print_json(&outcome)
        }
        "recover" => {
            let root = required_path(&mut arguments, "promotion root")?;
            reject_extra(arguments)?;
            let outcome = promotion::recover(&Layout::new(root), CrashAfter::never())?;
            print_json(&outcome)
        }
        "run" => {
            let output = required_path(&mut arguments, "measurement output")?;
            let deadline_ms = parse_text::<u64>(&mut arguments, "health deadline milliseconds")?;
            reject_extra(arguments)?;
            let executable = std::env::current_exe().context("resolve current executable")?;
            let report =
                scenarios::run_all(&executable, &output, Duration::from_millis(deadline_ms))?;
            print_json(&report)
        }
        _ => bail!("unknown command"),
    }
}

fn required_path(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &'static str,
) -> Result<PathBuf> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("missing {name}"))
}

fn parse_text<T>(arguments: &mut impl Iterator<Item = OsString>, name: &'static str) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let value = strict_text(arguments.next(), name)?;
    value.parse().with_context(|| format!("parse {name}"))
}

fn optional_text<T>(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &'static str,
) -> Result<Option<T>>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    arguments
        .next()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| anyhow!("{name} is not Unicode"))?
                .parse()
                .with_context(|| format!("parse {name}"))
        })
        .transpose()
}

fn strict_text(value: Option<OsString>, name: &'static str) -> Result<String> {
    value
        .ok_or_else(|| anyhow!("missing {name}"))?
        .into_string()
        .map_err(|_| anyhow!("{name} is not Unicode"))
}

fn reject_extra(mut arguments: impl Iterator<Item = OsString>) -> Result<()> {
    if arguments.next().is_some() {
        bail!("unexpected extra argument");
    }
    Ok(())
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer(std::io::stdout().lock(), value).context("write JSON output")?;
    println!();
    Ok(())
}
