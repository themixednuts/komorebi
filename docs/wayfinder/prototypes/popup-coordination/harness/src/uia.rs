use std::ffi::OsString;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::process::Command;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::System::Com::CoCreateInstance;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::CoUninitialize;
use windows::Win32::UI::Accessibility::CUIAutomation;
use windows::Win32::UI::Accessibility::IUIAutomation;
use windows::Win32::UI::Accessibility::IUIAutomationWindowPattern;
use windows::Win32::UI::Accessibility::UIA_WindowPatternId;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiaTopology {
    SacrificialProcess,
    ReplaceableThreadInSacrificialProcess,
}

#[derive(Clone, Copy, Debug)]
pub struct UiaRequest {
    pub window: isize,
    pub generation: u64,
    pub deadline: Duration,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UiaFacts {
    pub generation: u64,
    pub control_type: i32,
    pub window_pattern: bool,
    pub is_modal: Option<bool>,
    pub call_elapsed_ns: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum UiaOutcome {
    Available {
        topology: UiaTopology,
        total_elapsed_ns: u64,
        facts: UiaFacts,
    },
    TimedOut {
        topology: UiaTopology,
        deadline_ms: u64,
        child_terminated_and_reaped: bool,
    },
    Failed {
        topology: UiaTopology,
        message: String,
    },
}

pub async fn probe_process(executable: &std::path::Path, request: UiaRequest) -> UiaOutcome {
    probe_supervised(executable, request, UiaTopology::SacrificialProcess).await
}

pub async fn probe_thread_victim(executable: &std::path::Path, request: UiaRequest) -> UiaOutcome {
    probe_supervised(
        executable,
        request,
        UiaTopology::ReplaceableThreadInSacrificialProcess,
    )
    .await
}

async fn probe_supervised(
    executable: &std::path::Path,
    request: UiaRequest,
    topology: UiaTopology,
) -> UiaOutcome {
    match probe_supervised_inner(executable, request, topology).await {
        Ok(outcome) => outcome,
        Err(error) => UiaOutcome::Failed {
            topology,
            message: format!("{error:#}"),
        },
    }
}

async fn probe_supervised_inner(
    executable: &std::path::Path,
    request: UiaRequest,
    topology: UiaTopology,
) -> Result<UiaOutcome> {
    let command = match topology {
        UiaTopology::SacrificialProcess => "uia-worker",
        UiaTopology::ReplaceableThreadInSacrificialProcess => "uia-thread-victim",
    };
    let started = Instant::now();
    let mut process = Command::new(executable);
    process
        .arg(command)
        .arg(request.window.to_string())
        .arg(request.generation.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    process.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    let mut child = ChildGuard::new(process.spawn()?);
    let wait_result = tokio::time::timeout(request.deadline, child.child_mut()?.wait()).await;
    let status = if let Ok(status) = wait_result {
        status?
    } else {
        child.terminate_and_reap().await?;
        return Ok(UiaOutcome::TimedOut {
            topology,
            deadline_ms: u64::try_from(request.deadline.as_millis()).unwrap_or(u64::MAX),
            child_terminated_and_reaped: true,
        });
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.child_mut()?.stdout.take() {
        pipe.read_to_end(&mut stdout).await?;
    }
    if let Some(mut pipe) = child.child_mut()?.stderr.take() {
        pipe.read_to_end(&mut stderr).await?;
    }
    child.disarm();
    if !status.success() {
        bail!("UIA child exited {status}; raw stderr bytes={stderr:?}");
    }
    let facts: UiaFacts = serde_json::from_slice(&stdout)?;
    if facts.generation != request.generation {
        bail!("UIA result belongs to a stale generation");
    }
    Ok(UiaOutcome::Available {
        topology,
        total_elapsed_ns: u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        facts,
    })
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    const fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn child_mut(&mut self) -> Result<&mut Child> {
        self.child.as_mut().context("UIA child was already reaped")
    }

    async fn terminate_and_reap(&mut self) -> Result<()> {
        let child = self.child_mut()?;
        child.start_kill()?;
        child.wait().await?;
        self.child = None;
        Ok(())
    }

    fn disarm(&mut self) {
        self.child = None;
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                // Destructors cannot return errors. kill_on_drop is the fallback if either
                // best-effort cleanup operation fails during runtime teardown.
                let _ = child.start_kill();
                let _ = child.wait().await;
            });
        } else {
            // Destructors cannot return errors; kill_on_drop remains enabled on the child.
            let _ = child.start_kill();
        }
    }
}

pub fn run_worker(mut arguments: impl Iterator<Item = OsString>) -> Result<()> {
    let (window, generation) = parse_request(&mut arguments)?;
    write_facts(&probe_on_mta(window, generation)?)
}

pub fn run_thread_victim(mut arguments: impl Iterator<Item = OsString>) -> Result<()> {
    let (window, generation) = parse_request(&mut arguments)?;
    let probe = std::thread::Builder::new()
        .name("uia-replaceable-thread-candidate".to_owned())
        .spawn(move || probe_on_mta(window, generation))?;
    let facts = probe
        .join()
        .map_err(|_| anyhow::anyhow!("UIA candidate thread panicked"))??;
    write_facts(&facts)
}

fn parse_request(arguments: &mut impl Iterator<Item = OsString>) -> Result<(isize, u64)> {
    let window = arguments
        .next()
        .context("missing UIA window argument")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("UIA window argument is not ASCII"))?
        .parse::<isize>()?;
    let generation = arguments
        .next()
        .context("missing UIA generation argument")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("UIA generation argument is not ASCII"))?
        .parse::<u64>()?;
    Ok((window, generation))
}

fn probe_on_mta(window: isize, generation: u64) -> Result<UiaFacts> {
    let started = Instant::now();
    // SAFETY: This worker owns the current thread and balances successful initialization in Drop.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    initialized.ok()?;
    let _guard = ComGuard;
    // SAFETY: COM is initialized MTA and the CLSID/interface pair is defined by UI Automation.
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
    let address = usize::try_from(window).context("negative UIA window address")?;
    let hwnd = HWND(std::ptr::with_exposed_provenance_mut(address));
    // SAFETY: The raw value came from the parent Win32 census; UIA validates stale HWND values.
    let element = unsafe { automation.ElementFromHandle(hwnd) }?;
    // SAFETY: `element` is a live COM interface on its owning MTA.
    let control_type = unsafe { element.CurrentControlType() }?.0;
    // SAFETY: The requested pattern identifier and interface type are the documented pair.
    let pattern =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationWindowPattern>(UIA_WindowPatternId) };
    let (window_pattern, is_modal) = match pattern {
        Ok(pattern) => {
            // SAFETY: `pattern` is a live COM interface on its owning MTA.
            (true, Some(unsafe { pattern.CurrentIsModal() }?.as_bool()))
        }
        Err(_) => (false, None),
    };
    Ok(UiaFacts {
        generation,
        control_type,
        window_pattern,
        is_modal,
        call_elapsed_ns: u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
    })
}

fn write_facts(facts: &UiaFacts) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &facts)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: The guard exists only after successful CoInitializeEx on this same thread.
        unsafe { CoUninitialize() };
    }
}
