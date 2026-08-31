# Windows shell ownership and renderer adapters

## Decision

Keep `komorebi-shell` renderer-neutral and keep GPUI as the first production
renderer. Add a measured WinUI/`windows-reactor` spike against the same shell
models before migrating accessibility-heavy surfaces. Do not make WinUI, XAML,
or GPUI types part of command, shortcut, palette, plugin, persistence, or native
window-management contracts.

Pursue Explorer replacement only through Microsoft's supported Shell Launcher
v2 contract on a Windows 11 Enterprise, Education, or IoT Enterprise test
system. Do not edit Winlogon's undocumented registry values or kill Explorer to
simulate shell ownership.

The development machine is Windows 11 Home (`EditionID=Core`), 25H2, build
26200.9168. Home has no Microsoft-supported arbitrary shell-registration
mechanism. The documented Custom User Interface policy starts at Pro, while
Shell Launcher v2 and its supervision/recovery contract require Enterprise,
Education, or IoT. Local exclusive-shell testing is therefore unsupported; a
registry `Winlogon\Shell` edit would be the rejected monkeypatch.

## Evidence

- [WinUI is developed in the MIT-licensed microsoft-ui-xaml repository](https://github.com/microsoft/microsoft-ui-xaml).
- [The current source build still documents prebuilt components and local-build limitations](https://github.com/microsoft/microsoft-ui-xaml/blob/winui3/main/GettingStarted.md#current-limitations).
- [`windows-reactor` is Microsoft's Rust declarative layer over native WinUI controls](https://github.com/microsoft/windows-rs/tree/master/crates/libs/reactor).
- [ContentIsland is the supported Windows App SDK composition-hosting seam](https://learn.microsoft.com/en-us/windows/apps/develop/composition/content-island).
- [Shell Launcher is the supported custom-shell supervisor](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/).
- [Shell Launcher configuration and edition requirements are documented by Microsoft](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/configure).
- [The Custom User Interface policy and its Pro-or-higher edition scope are documented by Microsoft](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-admx-winlogon#customshell).
- [DWM remains permanently enabled](https://learn.microsoft.com/en-us/windows/compatibility/desktop-window-manager-is-always-on). A custom shell owns the interactive desktop experience, not the operating-system compositor.

## Runtime ownership

```text
Winlogon
  -> CustomShellHost.exe (Shell Launcher v2)
    -> komorebi-shell-host.exe (stable, unelevated, supervised process)
      -> renderer-neutral shell runtime
        -> GPUI renderer | WinUI/Reactor renderer
        -> native WinEvent/window-management adapters
        -> DirectComposition/GPU decoration adapters
        -> narrow authenticated privileged broker, only where required
```

The shell host must remain alive rather than launch another process and exit.
It owns startup, recovery, child-service lifetime, and renderer selection.
Privileged operations never require the interactive shell to run elevated or
disable UAC.

## What shell replacement does and does not own

The custom shell can own the desktop, bar/taskbar, launcher, workspace UI,
settings surfaces, and application activation. It does not replace Winlogon,
DWM, UAC secure desktop, the lock screen, or protected system processes.

Two roles remain unproven and are hard gates for exclusive shell mode:

1. Windows exposes event-driven notification reading through
   `UserNotificationListener`, but no documented contract registers a custom
   exclusive toast presenter. A spike must prove whether duplicate native UI
   can be avoided without suppression hacks.
2. `Shell_NotifyIcon` documents the sender contract, not a public third-party
   notification-area host contract. Arbitrary legacy tray compatibility may be
   impossible without Explorer or undocumented protocols.

The current `komorebi-bar` tray path is not an exclusive-shell foundation.
`systray-util` creates a `Shell_TrayWnd` spy, intercepts undocumented
`WM_COPYDATA` traffic, and forwards against Explorer-owned tray state. Remove
that dependency from exclusive-shell builds. First-party indicators can use
documented system APIs; arbitrary third-party legacy tray compatibility remains
a non-goal unless Microsoft publishes a supported receiver contract.

## Renderer comparison spike

Render the exact same shortcut and palette models in GPUI, WinUI hosted as an
island, and standalone `windows-reactor`. Measure:

- cold and warm first-present and input-ready latency;
- settled private memory, handles, threads, idle CPU, and GPU allocation;
- frame and input latency under load;
- mixed-DPI/multi-monitor behavior and no-activate focus behavior;
- Narrator, UI Automation, keyboard-only operation, and native theme behavior;
- clean-machine deployment size and packaging requirements; and
- custom Direct3D/HLSL content hosting without mirroring shell state.

The Reactor spike remains a disposable crate outside the production workspace.
Pin the inspected Git revision
`dc720b3674c46ceb82d758ed20959977b32e60a9`; crates.io currently contains only
a `0.0.0` placeholder. This workspace uses `windows-core` 0.62 while Reactor
master uses 0.100, so no WinRT, COM, `IUnknown`, or HWND wrapper crosses the
spike boundary. Preserve authoritative paths as `PathBuf`, `OsString`, or the
shell's opaque WTF-16-safe path identity; Reactor `String` paths are display
text only.

Capability parity precedes performance measurement. The spike must first prove
popup/tool-window kind, exact placement, immovable and non-resizable behavior,
topmost/no-activate semantics, HWND access, focus return, DPI, keyboard,
Narrator, theming, and deployment. Reactor's current public window API does not
expose several of those controls. If parity fails, test one narrow native
window-profile extension or a XAML Island rather than comparing a conventional
opaque window to the production GPUI popup.

GPUI remains selected until those measurements show a material product benefit.
WinUI is favored for opaque accessibility-heavy settings and dialogs if it
improves Windows integration. GPUI/native composition remains selected for the
bar, command-palette popup, transparent borders, particles, and HUD effects.
`SwapChainPanel` can host custom GPU content, but that content still requires an
explicit UI Automation peer.

Do not add a generic renderer trait. Extract pure shell controllers only where
two adapters need the same behavior. For the palette, move activation state and
stale-completion handling out of GPUI into a typed `PaletteController`; the UI
adapters own physical input, focus, windows, and presentation, while the shell
owns selection and activation transitions.

## Shell Launcher proof environment

Use a Windows 11 Enterprise VM and event-driven ETW, process, WinEvent, and
notification observation. Prove sign-in startup, crash recovery, Win32 and
packaged-app activation, Settings, file/protocol associations, optional File
Explorer folder windows, appbars, fullscreen, monitor changes, sleep/resume,
lock/unlock, UAC, RDP, Alt-Tab, virtual desktops, accessibility, notifications,
and tray behavior. Maintain a separate Explorer-backed administrator account
and a tested offline rollback path.
