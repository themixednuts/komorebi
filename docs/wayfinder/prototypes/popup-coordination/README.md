# Popup coordination native probe

This disposable Windows 11 probe measures issue #42 without changing komorebi or foreign application windows. It uses real User32 WinEvents, Win32 observations, UI Automation clients, controlled HWND fixtures, and one constrained placement/focus experiment.

Run from `harness`:

```powershell
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
& .\target\release\wayfinder-popup-coordination-prototype.exe run ..\measurements\latest.json
```

The binary enters one Tokio runtime through `#[tokio::main]`. User32 message pumps and COM MTA work remain on their required affine OS threads. It has no timer polling and no interactive HTML.

`measurements/latest.json` is raw, title-redacted evidence. Window class names remain UTF-16 code units; native command/path arguments remain `OsString`/`PathBuf`. Display diagnostics never become identity or matching input.
