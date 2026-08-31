# WinUI and exclusive Windows shell research

Date: 2026-08-31

## Recommendation

Keep GPUI as the production UI renderer and finish removing egui. Run one
standalone WinUI spike for the settings UI and command palette, where native
controls, UI Automation, IME, touch, and high-contrast behavior may justify the
extra runtime. Do not move the bar, borders, particles, or other transparent
shell decorations to WinUI. Keep those on documented Win32 window roles plus a
direct GPU renderer and Windows composition.

Do not fork WinUI. Its open source tree is useful for debugging, but building a
private WinUI product binary still has incomplete tooling and prebuilt
components. Microsoft's newer pure-Rust `windows-reactor` crate is the useful
integration point. It should be tested from a pinned Git revision because the
published crates.io package is still a placeholder.

Only call komorebi an exclusive Windows shell when Windows starts and
supervises it through Shell Launcher v2 on Windows 11 Enterprise, Education, or
IoT Enterprise. Never simulate that by killing Explorer, hiding its windows, or
editing undocumented Winlogon registry values. This development machine is
Windows 11 Home, so the exclusive-shell proof belongs in an Enterprise VM.

Even in that VM, two compatibility gaps are release blockers: Windows does not
publish a third-party legacy notification-area host contract, and it does not
publish a contract that lets another process become the exclusive toast
presenter. Those facts put a real boundary around how much of Explorer's shell
experience we can own without undocumented protocols.

## What open source WinUI changes

[WinUI 3 is available under the MIT license](https://github.com/microsoft/microsoft-ui-xaml),
but source access does not turn it into a small Rust UI library. Microsoft's
[source build instructions still list prebuilt components, an incomplete XAML
compiler release, and unsupported local CI](https://github.com/microsoft/microsoft-ui-xaml/blob/main/GettingStarted.md#current-limitations).
The Windows App SDK runtime, WinRT ABI, XAML UI thread, and deployment model
remain part of an application that uses WinUI.

The practical Rust route is Microsoft's `windows-rs` work. At the inspected
commit
[`dc720b3674c46ceb82d758ed20959977b32e60a9`](https://github.com/microsoft/windows-rs/tree/dc720b3674c46ceb82d758ed20959977b32e60a9/crates/libs/reactor),
`windows-reactor` is a pure-Rust declarative layer that reconciles typed Rust
views to native WinUI controls. It boots the Windows App Runtime, runs WinUI on
an STA UI thread, and supports multiple windows on that thread. No C++/WinRT
component is required for a code-only standalone application. Its source
package reports version 0.100 and Rust 1.95, while
[crates.io currently exposes only version 0.0.0](https://crates.io/crates/windows-reactor).

The current Reactor API includes native controls, automation properties,
`SwapChainPanel`, and a composition host. Its public window controls cover
backdrop, size constraints, icon, title bar, and theme. The backend can obtain
the HWND through `IWindowNative`, but that function remains private at the
inspected revision. Reactor does not yet expose the complete shell-window
contract komorebi needs: no-activate, tool-window, click-through, exact monitor
placement, switcher exclusion, and appbar behavior.

That limitation is fixable without leaking HWND through the application. If
the spike reaches this point, add one typed `WindowRole` seam to a narrow
Reactor fork or upstream contribution. The native adapter obtains the HWND and
applies documented
[`WS_EX_NOACTIVATE`, `WS_EX_TOOLWINDOW`, and `WS_EX_TOPMOST` styles](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles)
plus [`SetWindowPos` with `SWP_NOACTIVATE`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos).
Nothing above that adapter receives an HWND, COM interface, or WinRT object.

```rust
enum WinUiWindowRole {
    InteractivePalette(PalettePlacement),
    PassiveBar(BarPlacement),
    PassiveOverlay(OverlayPlacement),
    OpaqueSettings,
}
```

The variants replace combinations of window-style booleans. Their payloads
contain validated coordinates and monitor identities. The adapter matches each
variant exhaustively and applies one native profile.

## Rendering, effects, accessibility, and hosting

[WinUI uses the retained, GPU-accelerated Windows visual layer](https://learn.microsoft.com/en-us/windows/uwp/composition/visual-layer).
Its animations can continue independently of the application UI thread.
[Composition effects](https://learn.microsoft.com/en-us/windows/apps/develop/composition/composition-effects)
cover supported effect graphs and animated properties, not arbitrary HLSL.
[Win2D `PixelShaderEffect`](https://microsoft.github.io/Win2D/WinUI3/html/T_Microsoft_Graphics_Canvas_Effects_PixelShaderEffect.htm)
can run custom compiled pixel shaders, but Win2D marks that effect as unavailable
to the composition effect system. Arbitrary particles and shader pipelines
therefore need Direct3D or wgpu content in a `SwapChainPanel`, or a separate
composition-hosted surface.

Microsoft documents two composition object graphs in
[`windows-composition`](https://github.com/microsoft/windows-rs/blob/master/docs/crates/windows-composition.md).
`Microsoft.UI.Composition` belongs to Windows App SDK and can be hosted inside
WinUI. `Windows.UI.Composition` is the system stack and can target an HWND
through `DesktopWindowTarget` without WinUI. Objects cannot cross between the
graphs. The system stack is the cleaner base for top-level transparent borders
and overlay windows. The renderer can attach wgpu or Direct3D content without
making XAML responsible for shell decoration.

[XAML Islands](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.hosting?view=windows-app-sdk-2.0)
can put WinUI controls in an existing Win32 HWND, and
[`ContentIsland`](https://learn.microsoft.com/en-us/windows/apps/develop/composition/content-island)
can host composition, Win2D, or Direct3D content inside a WinUI tree. The
[official Islands sample](https://github.com/microsoft/WindowsAppSDK-Samples/blob/main/Samples/Islands/README.md)
also shows the focus and message integration that the host must own. Mixing
GPUI and XAML in one interactive window would add two focus, layout, and input
systems. A standalone Reactor renderer against the same pure shell state is a
better comparison.

WinUI's strongest advantage is native behavior. Its controls already implement
UI Automation peers, keyboard navigation, IME, touch, high contrast, and system
theme behavior. Reactor exposes automation names and IDs over those controls.
Pixels drawn into a swap chain do not acquire those semantics. A shader-backed
control needs a parallel semantic control tree or a custom automation peer.

[AppWindow](https://learn.microsoft.com/en-us/windows/apps/develop/ui/windowing-overview)
is a documented one-to-one abstraction over a top-level HWND and provides
placement, z-order, title bar, and presenter controls. It does not attach UI
content to a window, and it does not replace the User32 styles needed for a
passive shell window.

Windows App SDK deployment also matters for a login shell. An unpackaged app
must [initialize and deploy the runtime](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deploy-unpackaged-apps),
either framework-dependent or self-contained. The stable shell host must not
depend exclusively on WinUI. It needs to remain alive and expose a minimal
recovery UI if Windows App SDK initialization or the child renderer fails.

## Renderer call stack and proof

There should be no generic renderer trait. Share behavior only after the GPUI
and Reactor adapters prove that the same pure controller is useful.

```text
PaletteFrame
  -> WinUiPaletteAdapter::view
    -> Reactor Component/View
      -> native WinUI controls on the STA thread
        -> Microsoft.UI.Composition
          -> DWM

native click, key, focus, or text event
  -> PaletteInput
    -> PaletteController
      -> existing typed broker or service effect
```

The spike renders the same command and shortcut models in GPUI and standalone
Reactor. It must first prove exact placement, no-activate behavior, focus
return, DPI changes, multi-monitor movement, Narrator, keyboard-only input,
high contrast, and clean-machine deployment. After parity, measure cold and
warm time to first interactive frame, p95 input-to-present latency, settled
private memory, threads, handles, idle CPU, GPU allocation, and package size.

Adopt Reactor for an opaque settings surface only if it provides measurably
better Windows behavior with acceptable startup and memory. Adopt it for the
palette only if the narrow native window-role extension passes every focus and
activation test. Keep GPUI for the bar and HUD. Keep a direct GPU backend plus
system composition for borders, particles, and shaders.

The spike must use event callbacks and the native dispatcher. It must not poll
for HWND creation, focus, DPI, composition readiness, or window state.

## What replacing Explorer actually means

[Shell Launcher v2](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/)
is Microsoft's supported Explorer replacement contract. Windows runs
`CustomShellHost.exe`, starts the configured shell, watches the exact process,
and applies a configured action when it exits. Shell Launcher also processes
[`Run` and `RunOnce` entries before starting the shell](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/configure#shell-launcher-startup-and-exit-behavior).
Microsoft limits it to
[Enterprise, Education, and IoT Enterprise editions](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/wesl-usersetting).

The
[Custom User Interface policy](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-admx-winlogon#customshell)
starts at Pro and can substitute another logon UI, but its documentation does
not provide Shell Launcher's process supervision and recovery contract. It is
useful for a controlled proof, not the target architecture. Home has neither a
supported arbitrary-shell policy nor Shell Launcher.

Owning the shell does not mean owning the compositor.
[DWM is always on](https://learn.microsoft.com/en-us/windows/compatibility/desktop-window-manager-is-always-on).
Winlogon, the lock screen, Ctrl+Alt+Delete, and the UAC secure desktop also stay
with Windows. Microsoft documents the separate Default, ScreenSaver, and
[Winlogon desktops](https://learn.microsoft.com/en-us/windows/win32/winstation/desktops),
and [UAC uses the secure desktop](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/user-account-control/how-it-works).
The custom shell must run unelevated. A small authenticated broker can perform
the few operations that need elevation.

The exclusive shell can own the desktop surface, wallpaper, taskbar
replacement, launcher, workspace UI, task switcher UI where reserved shortcuts
permit it, settings, file and protocol activation, window policy, decorations,
and first-party status indicators. Explorer can still run a folder window as a
file-manager application if testing proves that doing so does not recreate its
shell UI.

Three Windows contracts set the boundary:

1. [`Shell_NotifyIcon`](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area)
   documents how an application sends an icon to the taskbar status area. It
   does not document how a third party becomes that area's host. The current
   [`systray-util` dependency describes its own `Shell_TrayWnd` spy and
   `WM_COPYDATA` interception](https://github.com/glzr-io/zebar/blob/main/crates/systray-util/README.md#technical-overview).
   Its source also inspects `TrayNotifyWnd` and `ToolbarWindow32`. That is an
   undocumented Explorer protocol. It cannot be part of exclusive-shell mode.
2. [`UserNotificationListener`](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener)
   provides permission-gated, event-driven access to toast history and actions.
   It does not register a process as the exclusive toast presenter or suppress
   Windows' presenter.
3. Public [`IVirtualDesktopManager`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager)
   supports identifying a window's desktop and
   [moving a window to a known desktop](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ivirtualdesktopmanager-movewindowtodesktop).
   It does not publicly create, enumerate, switch, or subscribe to the native
   desktop set. Komorebi workspaces should remain first-class rather than rely
   on undocumented virtual-desktop COM interfaces.

## Exclusive shell call stack and proof

```text
Winlogon
  -> CustomShellHost.exe
    -> komorebi-shell-host.exe, stable and unelevated
      -> renderer-neutral shell runtime
        -> GPUI renderer or isolated WinUI renderer
        -> WinEvent and documented window-management adapters
        -> system composition and GPU decoration adapters
        -> authenticated privileged broker when required
```

Provisioning is a separate elevated command. It accepts an account SID, an
ACL-protected canonical executable path, and a recovery policy. Its call stack
probes the Windows edition and Shell Launcher feature, validates the executable
and account, records rollback data, writes the documented `WESL_UserSetting`
configuration, reads it back, and then returns. Sign-out remains an explicit
separate operation. Typestate prevents `apply` before every validation passes.

Run the proof in a Windows 11 Enterprise VM with a separate Explorer-backed
administrator account and tested offline rollback. Prove sign-in, crash and
renderer recovery, Win32 and packaged app launch, Settings, file and protocol
activation, optional Explorer folder windows, fullscreen, monitors and DPI,
sleep and resume, lock and unlock, UAC, RDP, accessibility, Alt-Tab, and native
virtual desktops.

Notifications and tray behavior are hard gates. A notification must appear
exactly once in the custom UI, preserve actions and dismissal, and produce no
native duplicate without suppression hooks. Third-party tray applications must
either use a documented host path or be declared unsupported. If either result
requires Explorer window spies, process injection, private COM interfaces, or
shell process killing, the honest product boundary is a custom desktop and
window manager, not a complete Windows shell replacement.
