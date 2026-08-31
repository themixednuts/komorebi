#![cfg(windows)]

use komorebi_extensions::LpacLaunchError;
use komorebi_extensions::LpacWorkerLauncher;
use komorebi_extensions::PluginId;
use komorebi_extensions::SandboxIdentity;

#[test]
fn native_worker_runs_inside_the_required_lpac_boundary() -> Result<(), Box<dyn std::error::Error>>
{
    let plugin = PluginId::parse("containment-probe")?;
    let identity = SandboxIdentity::for_plugin(&plugin);
    let worker = std::path::Path::new(env!("CARGO_BIN_EXE_komorebi-extension-worker"));

    let report = LpacWorkerLauncher::new(identity).launch_probe(worker)?;

    assert!(report.is_app_container());
    assert!(report.is_less_privileged());
    assert!(report.has_low_integrity());
    assert!(report.has_no_capabilities());
    assert!(report.denies_child_processes());
    assert!(report.disables_win32k());
    assert!(report.prohibits_dynamic_code());
    assert!(report.is_job_contained());
    Ok(())
}

#[test]
fn launcher_rejects_a_relative_worker_path_before_windows_process_creation()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = PluginId::parse("invalid-path-probe")?;
    let launcher = LpacWorkerLauncher::new(SandboxIdentity::for_plugin(&plugin));

    let result = launcher.launch_probe(std::path::Path::new("worker.exe"));

    assert!(matches!(result, Err(LpacLaunchError::InvalidWorkerPath)));
    Ok(())
}
