# Exclusive notification presentation probe

This disposable Windows 11 probe answers whether documented Windows APIs let the manager become the sole presenter for arbitrary ordinary notifications. It is a native Rust/MSIX experiment, not an interactive HTML mock-up and not production code.

The answer is **no**. `UserNotificationListener` is a consented observation and dismissal API. It is not a pre-display interception API, does not expose an original notification action to a listener, and does not raise an access-revoked event. A producing app can suppress its own popup, but a listener cannot set that policy for another app. Focus mutation is a Limited Access Feature rather than an ordinary recovery contract.

The safe product route is therefore Windows-owned popups plus an optional consented manager-owned private history. The manager never shows a second popup for an observed notification.

## Reproduce

Run from the repository root in PowerShell:

```powershell
./docs/wayfinder/prototypes/exclusive-notification-presentation/run.ps1 -Action Register
./docs/wayfinder/prototypes/exclusive-notification-presentation/run.ps1 -Action Status
./docs/wayfinder/prototypes/exclusive-notification-presentation/run.ps1 -Action Measure
./docs/wayfinder/prototypes/exclusive-notification-presentation/run.ps1 -Action Unregister
```

`Register` builds and audits the Rust crate, creates a temporary code-signing certificate, packages the full-trust probe with the `userNotificationListener` capability, and asks for one UAC approval to trust/install it. `Unregister` removes the exact package and certificate thumbprint. It does not enable Developer Mode or change sideloading policy.

The probe subscribes to `NotificationChanged` and waits for exact native events with one operation deadline. It does not poll notification state. Result paths remain `PathBuf`; WinRT text is retained as UTF-16 code units with a separate lossy display projection.

See [CONTRACT.md](CONTRACT.md) for the call stacks and [RESULTS.md](RESULTS.md) for measured evidence.
