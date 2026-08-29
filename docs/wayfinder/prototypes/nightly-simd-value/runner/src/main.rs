mod matrix;
mod process;
mod report;

use std::{ffi::OsString, path::Path};

use anyhow::{Context as _, Result, bail};

use crate::{
    matrix::{CompilerArm, Scope, operations},
    process::{release_binary_size, run, rustc_version, tail},
    report::{CommandMeasurement, DiagnosticMeasurement, ReportContext, write_report},
};

#[tokio::main]
async fn main() -> Result<()> {
    let trials = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>())
        .transpose()
        .context("parse trial count")?
        .unwrap_or(2);
    if trials == 0 {
        bail!("trial count must be non-zero");
    }

    let prototype = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("runner manifest directory has no parent")?
        .to_path_buf();
    let repository = prototype
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
        .context("find repository root")?
        .to_path_buf();
    let targets = prototype.join("targets");
    tokio::fs::create_dir_all(&targets)
        .await
        .context("create measurement target root")?;

    let report_context = ReportContext {
        source_revision: process::command_text("git", ["rev-parse", "HEAD"], &repository)
            .await?
            .trim()
            .to_owned(),
        stable_version: rustc_version(CompilerArm::Stable, &prototype).await?,
        nightly_version: rustc_version(CompilerArm::NightlyDefault, &prototype).await?,
        trials,
    };
    let mut measurements = Vec::new();
    let mut diagnostics = Vec::new();
    let report_path = prototype.join("compiler-measurements.json");

    for trial in 0..trials {
        let arm_offset = trial % CompilerArm::ALL.len();
        for arm_index in 0..CompilerArm::ALL.len() {
            let arm = CompilerArm::ALL[(arm_index + arm_offset) % CompilerArm::ALL.len()];
            let scope_offset = trial % Scope::ALL.len();
            for scope_index in 0..Scope::ALL.len() {
                let scope = Scope::ALL[(scope_index + scope_offset) % Scope::ALL.len()];
                measure_scope(
                    trial,
                    arm,
                    scope,
                    &prototype,
                    &repository,
                    &targets,
                    &mut measurements,
                )
                .await?;
                write_report(
                    &report_path,
                    false,
                    &report_context,
                    &measurements,
                    &diagnostics,
                )
                .await?;
            }
        }
    }

    for arm in CompilerArm::ALL {
        diagnostics.push(measure_diagnostic(arm, &prototype).await?);
        write_report(
            &report_path,
            false,
            &report_context,
            &measurements,
            &diagnostics,
        )
        .await?;
    }

    write_report(
        &report_path,
        true,
        &report_context,
        &measurements,
        &diagnostics,
    )
    .await?;
    println!("compiler measurement matrix complete");
    Ok(())
}

async fn measure_scope(
    trial: usize,
    arm: CompilerArm,
    scope: Scope,
    prototype: &Path,
    repository: &Path,
    targets: &Path,
    measurements: &mut Vec<CommandMeasurement>,
) -> Result<()> {
    let current_dir = match scope {
        Scope::Repository => repository,
        Scope::PlannedStackFixture => prototype,
    };
    let target = targets.join(format!("{}-{}", arm.name(), scope.name()));
    let clean_args = vec![
        OsString::from(format!("+{}", arm.toolchain())),
        OsString::from("clean"),
        OsString::from("--target-dir"),
        target.as_os_str().to_owned(),
    ];
    let clean = run("cargo", clean_args, current_dir, arm, None).await?;
    if !clean.success {
        bail!("cargo clean failed for {} {}", arm.name(), scope.name());
    }

    for (operation, cargo_args) in operations(scope) {
        let mut args = Vec::with_capacity(cargo_args.len() + 1);
        args.push(OsString::from(format!("+{}", arm.toolchain())));
        args.extend(cargo_args.iter().map(OsString::from));
        let output = run("cargo", args, current_dir, arm, Some(&target)).await?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let release_binary_bytes = if operation == "build-release" && output.success {
            Some(release_binary_size(&target.join("release")).await?)
        } else {
            None
        };
        measurements.push(CommandMeasurement {
            trial,
            arm,
            scope,
            operation,
            elapsed_ms: output.elapsed_ms,
            success: output.success,
            exit_code: output.exit_code,
            warning_lines: stderr
                .lines()
                .filter(|line| line.contains("warning"))
                .count(),
            error_lines: stderr.lines().filter(|line| line.contains("error")).count(),
            release_binary_bytes,
            stderr_tail: tail(&stderr, 4_000),
        });
    }
    tokio::fs::remove_dir_all(&target)
        .await
        .with_context(|| format!("remove completed measurement target {}", target.display()))?;
    Ok(())
}

async fn measure_diagnostic(arm: CompilerArm, prototype: &Path) -> Result<DiagnosticMeasurement> {
    let mut args = vec![
        OsString::from(format!("+{}", arm.toolchain())),
        prototype
            .join("diagnostics")
            .join("trait_error.rs")
            .into_os_string(),
        OsString::from("--error-format=json"),
    ];
    if matches!(arm, CompilerArm::NightlyNextSolver) {
        args.push(OsString::from("-Znext-solver"));
    }
    let output = run("rustc", args, prototype, arm, None).await?;
    let path = prototype
        .join("diagnostics")
        .join(format!("{}.jsonl", arm.name()));
    tokio::fs::write(&path, &output.stderr)
        .await
        .context("write compiler diagnostic")?;
    let text = String::from_utf8_lossy(&output.stderr);
    Ok(DiagnosticMeasurement {
        arm,
        success: output.success,
        exit_code: output.exit_code,
        json_diagnostic_lines: text.lines().filter(|line| line.starts_with('{')).count(),
        output_path: path
            .strip_prefix(prototype)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned(),
    })
}
