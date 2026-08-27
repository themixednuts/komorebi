# Active installation baseline

This folder records the work for [Restore a trustworthy active-installation baseline](https://github.com/themixednuts/komorebi/issues/34). It describes this personal machine. It is not a general komorebi installer.

`baseline.spec.json` is the expected active state. `doctor.ps1` reads that specification and the live Windows installation, writes exact paths and SHA-256 hashes, and exits with code 1 when any check fails.

`DesktopSlideshow.cs` is a narrow adapter over the documented `IDesktopWallpaper` API. It sets or reads the live slideshow folder without opening Settings or reapplying the theme. Build it once with `build-slideshow-tool.ps1` before running the doctor.

`recover-ipc.ps1` is a one-purpose recovery path for the observed state where the manager process survives but its AF_UNIX listener no longer accepts clients. It restores cached windows before any fallback termination, validates every marker path before removal, recreates the listener, proves a JSON state query, and starts the AppBar through its Startup shortcut.

On this machine, fresh `uds_windows` sockets fail with error 10022 in direct children of `AppData\Local` but pass under the user profile and through a directory junction. Recovery therefore keeps komorebi's fixed `AppData\Local\komorebi` name as a junction to `C:\Users\jonfo\.local\share\komorebi\runtime`. This avoids changing global environment variables or patching the installed v0.1.41 binaries.

`uds-probe` runs a four-byte request and response through a fresh listener with the same `uds_windows` version and pattern that komorebi uses. It also queries the manager socket. The optional second argument writes its report to a file.

Task Scheduler cannot host this installation's AF_UNIX clients. A probe started by the `Komorebi AppBar` task returned Winsock error 10050 for both a fresh listener and the manager socket. The same probe passed when the interactive shell started it. The `Komorebi` and `Komorebi AppBar` tasks remain disabled as recoverable evidence.

Windows starts both components from the current user's Startup folder. `Komorebi.lnk` starts `komorebic-no-console.exe start --whkd`. `Komorebi AppBar.lnk` starts the stable custom binary. Both executables use the Windows GUI subsystem, so startup does not create a console window.

Run `configure-startup.ps1` to recreate both shortcuts and disable the incompatible scheduled tasks. The script reads the exact targets and arguments from `baseline.spec.json` and is safe to run more than once.

Run the doctor from PowerShell:

```powershell
./build-slideshow-tool.ps1
./doctor.ps1 -OutputPath ./results/doctor.json
```

## Call-stack design

The accepted ownership already exists in `CONTEXT.md`: Doctor is read-only and Repair is an explicit state change. Combining them would let an observation silently mutate the installation, so the baseline keeps them separate.

```text
doctor.ps1: SpecPath, OutputPath
  -> parse baseline.spec.json: validated expected paths, hashes, counts, and URLs
    -> read live files, processes, Startup shortcuts, disabled scheduled tasks, registry values, IDesktopWallpaper, IPC, and Git configuration
      -> build named checks: expected value plus actual value
        -> aggregate report: passing only when every check passes
          -> write JSON report: the only doctor side effect
  <- exit 0 on pass, exit 1 on any mismatch or unavailable boundary
```

Each Windows, process, IPC, filesystem, and Git query stays at the script boundary. The check records are plain values. Aggregation and pass or fail calculation operate only on those values. The report exposes boundary errors instead of repairing around them.

The repair path is intentionally narrower:

```text
explicit baseline repair
  -> copy content to stable paths and verify hashes
    -> replace source-backed references with stable paths
      -> restart only the affected process
        -> run doctor.ps1 against the live installation
  <- keep the change only when the corresponding check turns green
```

Every repair step must be safe to rerun. File identity comes from content hashes. Startup identity comes from each shortcut's exact target and arguments. The doctor requires one manager marker and one AppBar subscriber marker. Stale runtime markers are removed by current liveness, not age alone.
