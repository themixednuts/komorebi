use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail, ensure};
use clap::Parser;
use serde_json::json;

use crate::model::{Edge, Rect, ShellIdentity};
use crate::protocol::{ChildEvent, NotificationKind, PositionReason, ProbeCase, ProbeReport};
use crate::windows::{pe_subsystem, primary_monitor, restart_explorer};

const CHILD_EVENT_DEADLINE: Duration = Duration::from_secs(15);
const EXPLORER_EVENT_DEADLINE: Duration = Duration::from_secs(45);
const WINDOWS_GUI_SUBSYSTEM: u16 = 2;

#[derive(Parser)]
struct Args {
    /// Create the report at this exact path; existing files are never overwritten.
    #[arg(long)]
    output: Option<PathBuf>,
}

struct Ready {
    shell: ShellIdentity,
    positioned: Rect,
    shown: Rect,
    visible_before_position: bool,
}

struct AppBarChild {
    process: Child,
    input: ChildStdin,
    events: Receiver<Result<ChildEvent, String>>,
    exited: bool,
}

impl AppBarChild {
    fn spawn(executable: &Path, edge: Edge, thickness: i32) -> anyhow::Result<Self> {
        let mut process = Command::new(executable)
            .arg("--edge")
            .arg(edge_argument(edge))
            .arg("--thickness")
            .arg(thickness.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("spawn AppBar child")?;
        let input = process.stdin.take().context("capture AppBar child stdin")?;
        let output = process
            .stdout
            .take()
            .context("capture AppBar child stdout")?;
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let event = match line {
                    Ok(line) => serde_json::from_str(&line)
                        .map_err(|error| format!("decode child event: {error}; input={line:?}")),
                    Err(error) => Err(format!("read child event: {error}")),
                };
                if sender.send(event).is_err() {
                    return;
                }
            }
        });
        Ok(Self {
            process,
            input,
            events,
            exited: false,
        })
    }

    fn command(&mut self, command: &str) -> anyhow::Result<()> {
        self.input
            .write_all(command.as_bytes())
            .context("write AppBar child command")?;
        self.input
            .write_all(b"\n")
            .context("terminate AppBar child command")?;
        self.input.flush().context("flush AppBar child command")
    }

    fn wait_for(
        &self,
        operation: &str,
        deadline: Duration,
        predicate: impl Fn(&ChildEvent) -> bool,
    ) -> anyhow::Result<ChildEvent> {
        let expires = Instant::now()
            .checked_add(deadline)
            .context("compute native event deadline")?;
        loop {
            let remaining = expires
                .checked_duration_since(Instant::now())
                .with_context(|| format!("wait for {operation}"))?;
            let received = self
                .events
                .recv_timeout(remaining)
                .with_context(|| format!("receive {operation}"))?;
            let event = received.map_err(anyhow::Error::msg)?;
            if let ChildEvent::Failure { operation, message } = &event {
                bail!("AppBar child failed during {operation}: {message}");
            }
            if predicate(&event) {
                return Ok(event);
            }
        }
    }

    fn ready(&self) -> anyhow::Result<Ready> {
        let created = self.wait_for("hidden creation", CHILD_EVENT_DEADLINE, |event| {
            matches!(event, ChildEvent::CreatedHidden { .. })
        })?;
        let ChildEvent::CreatedHidden { process_id } = created else {
            bail!("hidden creation event changed shape");
        };
        ensure!(
            process_id == self.process.id(),
            "child PID evidence mismatch"
        );

        let registered = self.wait_for("initial registration", CHILD_EVENT_DEADLINE, |event| {
            matches!(event, ChildEvent::Registered { .. })
        })?;
        let ChildEvent::Registered { shell } = registered else {
            bail!("registration event changed shape");
        };
        let positioned = self.wait_for("initial position", CHILD_EVENT_DEADLINE, |event| {
            matches!(
                event,
                ChildEvent::Positioned {
                    reason: PositionReason::Initial,
                    ..
                }
            )
        })?;
        let ChildEvent::Positioned {
            rect: positioned, ..
        } = positioned
        else {
            bail!("position event changed shape");
        };
        let shown = self.wait_for("first show", CHILD_EVENT_DEADLINE, |event| {
            matches!(event, ChildEvent::Shown { .. })
        })?;
        let ChildEvent::Shown {
            rect: shown,
            visible_before_position,
            ..
        } = shown
        else {
            bail!("show event changed shape");
        };
        Ok(Ready {
            shell,
            positioned,
            shown,
            visible_before_position,
        })
    }

    fn wait_positioned(&self, reason: PositionReason) -> anyhow::Result<Rect> {
        let event = self.wait_for("AppBar position", CHILD_EVENT_DEADLINE, |event| {
            matches!(event, ChildEvent::Positioned { reason: found, .. } if *found == reason)
        })?;
        let ChildEvent::Positioned { rect, .. } = event else {
            bail!("position event changed shape");
        };
        Ok(rect)
    }

    fn graceful_exit(&mut self) -> anyhow::Result<()> {
        self.command("shutdown")?;
        self.wait_for("AppBar release", CHILD_EVENT_DEADLINE, |event| {
            matches!(event, ChildEvent::Released)
        })?;
        let status = self.process.wait().context("wait for AppBar child exit")?;
        self.exited = true;
        ensure!(status.success(), "AppBar child exited with {status}");
        Ok(())
    }

    fn crash(&mut self) -> anyhow::Result<()> {
        self.process.kill().context("terminate AppBar child")?;
        let _status = self
            .process
            .wait()
            .context("wait for terminated AppBar child")?;
        self.exited = true;
        Ok(())
    }
}

impl Drop for AppBarChild {
    fn drop(&mut self) {
        if !self.exited {
            let _kill_result = self.process.kill();
            let _wait_result = self.process.wait();
        }
    }
}

/// Runs the native lifecycle matrix and writes one immutable evidence report.
///
/// # Errors
///
/// Returns an error when a lifecycle invariant, native operation, or report write fails.
pub fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let report = execute()?;
    let output = match args.output {
        Some(output) => output,
        None => default_report_path()?,
    };
    write_report(&output, &report)?;
    println!("AppBar lifecycle report written");
    Ok(())
}

fn execute() -> anyhow::Result<ProbeReport> {
    let executable = std::env::current_exe().context("locate AppBar probe executable")?;
    let child_executable = executable.with_file_name("appbar-child.exe");
    let child_pe_subsystem = pe_subsystem(&child_executable)?;
    ensure!(
        child_pe_subsystem == WINDOWS_GUI_SUBSYSTEM,
        "child is not a Windows GUI-subsystem executable"
    );

    let (monitor, baseline_work_area) = primary_monitor()?;
    let mut cases = vec![passed(
        "gui_subsystem",
        json!({ "pe_subsystem": child_pe_subsystem }),
    )];
    let (mut observer, ready) = verify_startup(&child_executable, &mut cases)?;
    verify_competing_bars(&child_executable, &observer, &mut cases)?;
    verify_crash_cleanup(&child_executable, &observer, &mut cases)?;
    verify_geometry_change(&mut observer, &mut cases)?;
    verify_explorer_restart(&observer, ready.shell, &mut cases)?;
    verify_dpi_path(&mut observer, &mut cases)?;
    verify_final_cleanup(
        &child_executable,
        &mut observer,
        baseline_work_area,
        &mut cases,
    )?;

    Ok(ProbeReport {
        schema: 1,
        monitor,
        baseline_work_area,
        child_pe_subsystem,
        cases,
        limitations: vec![
            "DPI transition was injected through the real position call stack; this machine did not provide a second physical-DPI monitor for a live cross-monitor drag.".to_owned(),
            "Crash cleanup proves Explorer released this reservation after process death; forced machine power loss is outside a user-mode prototype.".to_owned(),
            "The prototype owns reservation lifecycle only. Explorer and DWM continue to own taskbar policy, work-area arbitration, composition, and final recovery semantics.".to_owned(),
        ],
    })
}

fn verify_startup(
    child_executable: &Path,
    cases: &mut Vec<ProbeCase>,
) -> anyhow::Result<(AppBarChild, Ready)> {
    let mut observer = AppBarChild::spawn(child_executable, Edge::Right, 13)?;
    let ready = observer.ready()?;
    ensure!(
        !ready.visible_before_position,
        "AppBar was visible before its first negotiated position"
    );
    ensure!(
        ready.positioned == ready.shown,
        "shown AppBar rectangle differs from negotiated rectangle"
    );
    cases.push(passed(
        "startup_is_hidden_until_positioned",
        json!({ "rect": ready.shown, "visible_before_position": false }),
    ));

    observer.command("register-again")?;
    let suppressed = observer.wait_for(
        "duplicate registration suppression",
        CHILD_EVENT_DEADLINE,
        |event| matches!(event, ChildEvent::RegistrationSuppressed { .. }),
    )?;
    let ChildEvent::RegistrationSuppressed { shell } = suppressed else {
        bail!("registration suppression event changed shape");
    };
    ensure!(
        shell == ready.shell,
        "suppression crossed shell generations"
    );
    cases.push(passed(
        "one_registration_per_shell_generation",
        json!({ "shell": shell }),
    ));
    Ok((observer, ready))
}

fn verify_competing_bars(
    child_executable: &Path,
    observer: &AppBarChild,
    cases: &mut Vec<ProbeCase>,
) -> anyhow::Result<()> {
    let mut contender = AppBarChild::spawn(child_executable, Edge::Right, 17)?;
    let contender_ready = contender.ready()?;
    let observer_stacked = observer
        .wait_positioned(PositionReason::ShellPositionChanged)
        .context("observe competing AppBar registration")?;
    let (_, work_area) = primary_monitor()?;
    ensure!(
        !observer_stacked.overlaps(contender_ready.positioned),
        "competing AppBars overlap"
    );
    ensure!(
        work_area.right == observer_stacked.left.min(contender_ready.positioned.left),
        "work area does not end at the innermost AppBar"
    );
    cases.push(passed(
        "competing_appbars_negotiate_without_overlap",
        json!({ "observer": observer_stacked, "contender": contender_ready.positioned, "work_area": work_area }),
    ));

    contender.graceful_exit()?;
    let observer_rect = observer
        .wait_positioned(PositionReason::ShellPositionChanged)
        .context("observe graceful AppBar removal")?;
    let (_, work_area) = primary_monitor()?;
    ensure!(
        work_area.right == observer_rect.left,
        "ABM_REMOVE left a stale reservation"
    );
    cases.push(passed(
        "graceful_remove_releases_reservation",
        json!({ "observer": observer_rect, "work_area": work_area }),
    ));
    Ok(())
}

fn verify_crash_cleanup(
    child_executable: &Path,
    observer: &AppBarChild,
    cases: &mut Vec<ProbeCase>,
) -> anyhow::Result<()> {
    let mut victim = AppBarChild::spawn(child_executable, Edge::Right, 19)?;
    let victim_ready = victim.ready()?;
    let _observer_with_victim = observer
        .wait_positioned(PositionReason::ShellPositionChanged)
        .context("observe crash-test AppBar registration")?;
    victim.crash()?;
    let observer_rect = observer
        .wait_positioned(PositionReason::ShellPositionChanged)
        .context("observe crashed AppBar cleanup")?;
    let (_, work_area) = primary_monitor()?;
    ensure!(
        work_area.right == observer_rect.left,
        "crashed AppBar left a stale reservation"
    );
    cases.push(passed(
        "crash_cleanup_converges_from_shell_notification",
        json!({ "crashed_rect": victim_ready.positioned, "observer": observer_rect, "work_area": work_area }),
    ));
    Ok(())
}

fn verify_geometry_change(
    observer: &mut AppBarChild,
    cases: &mut Vec<ProbeCase>,
) -> anyhow::Result<()> {
    observer.command("set-thickness 21")?;
    let rect = observer
        .wait_positioned(PositionReason::GeometryChanged)
        .context("observe AppBar geometry change")?;
    let work_area = settled_work_area(observer, rect)?;
    ensure!(
        work_area.right == rect.left && rect.width() == 21,
        "geometry change did not update the reservation: rect={rect:?}, work_area={work_area:?}"
    );
    cases.push(passed(
        "geometry_change_converges_on_shell_callback",
        json!({ "rect": rect, "work_area": work_area }),
    ));
    Ok(())
}

fn settled_work_area(observer: &AppBarChild, positioned: Rect) -> anyhow::Result<Rect> {
    let (_, first_observation) = primary_monitor()?;
    if first_observation.right == positioned.left {
        return Ok(first_observation);
    }
    let publication = observer.wait_for(
        "Shell work-area publication",
        CHILD_EVENT_DEADLINE,
        |event| {
            matches!(
                event,
                ChildEvent::Positioned {
                    reason: PositionReason::ShellPositionChanged,
                    rect,
                    work_area,
                } if work_area.right == rect.left
            )
        },
    )?;
    let ChildEvent::Positioned { work_area, .. } = publication else {
        bail!("work-area publication event changed shape");
    };
    Ok(work_area)
}

fn verify_explorer_restart(
    observer: &AppBarChild,
    previous_shell: ShellIdentity,
    cases: &mut Vec<ProbeCase>,
) -> anyhow::Result<()> {
    let old_process_id = restart_explorer()?;
    observer.wait_for(
        "TaskbarCreated broadcast",
        EXPLORER_EVENT_DEADLINE,
        |event| {
            matches!(
                event,
                ChildEvent::Notification {
                    notification: NotificationKind::TaskbarCreated
                }
            )
        },
    )?;
    let registered = observer.wait_for(
        "replacement shell registration",
        EXPLORER_EVENT_DEADLINE,
        |event| matches!(event, ChildEvent::Registered { shell } if shell.process_id != old_process_id),
    )?;
    let ChildEvent::Registered { shell } = registered else {
        bail!("replacement registration event changed shape");
    };
    ensure!(
        shell != previous_shell,
        "Explorer reused the old shell identity"
    );
    let rect = observer
        .wait_positioned(PositionReason::ShellRecreated)
        .context("observe AppBar recovery after Explorer restart")?;
    let (_, work_area) = primary_monitor()?;
    ensure!(work_area.right == rect.left, "reservation was not restored");
    cases.push(passed(
        "explorer_restart_registers_once_for_new_generation",
        json!({ "previous_shell": previous_shell, "replacement_shell": shell, "rect": rect, "work_area": work_area }),
    ));
    Ok(())
}

fn verify_dpi_path(observer: &mut AppBarChild, cases: &mut Vec<ProbeCase>) -> anyhow::Result<()> {
    observer.command("simulate-dpi 144")?;
    let rect = observer
        .wait_positioned(PositionReason::GeometryChanged)
        .context("observe synthetic DPI transition")?;
    ensure!(
        rect.width() == 32,
        "DPI conversion produced the wrong width"
    );
    cases.push(passed(
        "dpi_transition_path_preserves_dip_contract",
        json!({ "dip": 21, "dpi": 144, "physical_rect": rect }),
    ));
    Ok(())
}

fn verify_final_cleanup(
    child_executable: &Path,
    observer: &mut AppBarChild,
    baseline: Rect,
    cases: &mut Vec<ProbeCase>,
) -> anyhow::Result<()> {
    let mut sentinel = AppBarChild::spawn(child_executable, Edge::Left, 7)?;
    let _sentinel_ready = sentinel.ready()?;
    observer.graceful_exit()?;
    sentinel.wait_for(
        "right-edge reservation release",
        CHILD_EVENT_DEADLINE,
        |event| {
            matches!(
                event,
                ChildEvent::Positioned {
                    reason: PositionReason::ShellPositionChanged,
                    work_area,
                    ..
                } if work_area.right == baseline.right
            )
        },
    )?;
    let (_, final_work_area) = primary_monitor()?;
    ensure!(
        final_work_area.right == baseline.right,
        "prototype left a permanent reservation: baseline={baseline:?}, final={final_work_area:?}"
    );
    cases.push(passed(
        "final_cleanup_restores_baseline_edge",
        json!({ "baseline": baseline, "final": final_work_area }),
    ));
    sentinel.graceful_exit()?;
    Ok(())
}

fn passed(name: &str, evidence: serde_json::Value) -> ProbeCase {
    ProbeCase {
        name: name.to_owned(),
        passed: true,
        evidence,
    }
}

const fn edge_argument(edge: Edge) -> &'static str {
    match edge {
        Edge::Left => "left",
        Edge::Top => "top",
        Edge::Right => "right",
        Edge::Bottom => "bottom",
    }
}

fn default_report_path() -> anyhow::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?
        .as_nanos();
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("results")
        .join(format!(
            "appbar-lifecycle-{}-{stamp}.json",
            std::process::id()
        )))
}

fn write_report(path: &Path, report: &ProbeReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create report directory")?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("create unique report")?;
    serde_json::to_writer_pretty(&mut file, report).context("encode probe report")?;
    file.write_all(b"\n").context("terminate probe report")?;
    file.sync_all().context("persist probe report")
}
