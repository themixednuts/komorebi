# Native AppBar lifecycle prototype

This disposable Windows 11 prototype answers issue #26 with real `SHAppBarMessage` behavior. It is not production integration and it does not modify the active installation.

## Decision

Use an event-driven, Shell-generation-fenced AppBar lifecycle in the dedicated shell-role process:

- create the GUI-subsystem window hidden;
- call `ABM_NEW` once for the current Explorer process ID and creation time;
- negotiate with `ABM_QUERYPOS` and `ABM_SETPOS`, move, then show without activation;
- coalesce `ABN_POSCHANGED`, monitor, DPI, and geometry invalidations into one queued position pass;
- allow an invalidation received during a pass to schedule exactly one later pass;
- on `TaskbarCreated`, register once for the replacement Shell generation;
- call `ABM_REMOVE` before graceful destruction;
- let Explorer release a crashed process's reservation, while the manager supervisor uses a native process wait to restart the shell role.

There is no timer, retry interval, equality-settling loop, or work-area polling. A measured geometry change required two effects: the initiating position pass and one `ABN_POSCHANGED`-driven pass after Explorer published the new work area.

## Run

Run `run.ps1` from PowerShell. It builds a release GUI child, exercises competing bars, graceful removal, process death, geometry and DPI transitions, and restarts Explorer once. Explorer restart can close open File Explorer windows.

The report is created with `OpenOptions::create_new(true)` under `native/results`; an existing report is never overwritten.

## Path boundary

Executable and report paths remain `Path`/`PathBuf` values. The Explorer image comes from `WINDIR` as an `OsString` and is passed directly to `Command`. No filesystem or process operation consumes `display()`, `to_str()`, or a lossy UTF-8 conversion. Win32 string adapters must use lossless `OsStrExt::encode_wide`; extended-length path construction should use the focused `verbatim` crate only at the Win32 boundary that requires `\\?\` form.

See [CONTRACT.md](CONTRACT.md) for the typed call stacks and [RESULTS.md](RESULTS.md) for measured evidence.
