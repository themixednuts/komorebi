# Windows shell-surface ownership boundary

Research for [Wayfinder issue #28](https://github.com/themixednuts/komorebi/issues/28), performed on 2026-08-27. The target is a Windows 11-first personal Windows manager. The goal is to own as much of the workspace experience as Windows supports without confusing visual similarity with control of DWM or Explorer.

## Decision

Build a manager-owned shell layer alongside Explorer, not inside it:

- Own every manager-created surface: AppBar, command palette, overview, scratchpads, notification mirror, quick controls, and optional OSD replacements. Render and animate these as ordinary manager-owned top-level windows with documented Win32, DWM, DirectComposition, Windows App SDK, and UI Automation contracts.
- Observe and coordinate third-party top-level windows, including dialogs and popups, through out-of-context WinEvents, ownership/style inspection, UI Automation, placement, and z-order. Do not claim semantic ownership and do not suppress an arbitrary modal surface by default.
- Offer a consented notification mirror using `UserNotificationListener`. It can read and remove the current cross-app toast set after the user grants permission, but it cannot veto another application's banner before Windows presents it. A custom presentation mode therefore requires the user to enable Do Not Disturb/Focus Assist so Windows routes banners to Notification Center, then lets our surface mirror them.
- Keep Explorer running and the Windows taskbar auto-hidden while our AppBar is primary. We can add our own tray icon, but Windows exposes no documented provider contract for rehosting every third-party tray icon, Notification Center, Quick Settings, or system flyout.
- Treat policy-based hiding of Notification Center or Quick Settings, consuming hardware keys to avoid duplicate OSDs, and replacing Explorer as opt-in experiments with automatic rollback. They are not prerequisites for the manager-owned shell layer.
- Reject private ShellExperienceHost/Explorer COM, window-class hooks, cross-process subclassing, injected hooks, DWM patching, and UIAccess misuse as production foundations.

DWM still performs final desktop composition. It has been always enabled since Windows 8, and DirectComposition visual trees owned by applications are composed as subtrees of DWM's desktop tree; an application is not given the global compositor scene graph ([DWM overview](https://learn.microsoft.com/en-us/windows/win32/dwm/dwm-overview), [DWM always-on change](https://learn.microsoft.com/en-us/windows/compatibility/desktop-window-manager-is-always-on), [DirectComposition architecture](https://learn.microsoft.com/en-us/windows/win32/directcomp/architecture-and-components)). This does not make our shell layer a hack. AppBars, owned top-level windows, notification listeners, Core Audio, UI Automation, and DirectComposition are native Windows integration points. The boundary is that they do not transfer ownership of Explorer's or another application's surfaces.

## Capability matrix

| Surface | Own | Observe | Suppress or replace | Production boundary |
| --- | --- | --- | --- | --- |
| Manager-created overlays | **Yes.** Create, place, animate, capture-exclude, and expose accessibility for our own `HWND`s. | Full state because we own the process and model. | **Yes**, for our own surfaces. | Supported now. Use no-activate/tool-window/topmost policy deliberately; the previous composition probes already proved a correctly created non-activating overlay preserves focus. |
| Third-party popup and dialog windows | No semantic ownership. Win32 owner/owned relationships control z-order, lifetime, and minimize behavior. | **Yes, best effort.** Out-of-context WinEvents plus UI Automation and `GW_OWNER`/style inspection. | Placement, float/center, z-order, and an adjacent affordance are supported. Hiding, closing, reparenting, or changing ownership is mutation, not replacement. | Coordinate by explicit per-app policy. Never hide an unknown modal dialog, because its disabled owner can become unusable. |
| App notifications and current history | No ownership of the sender or Windows notification database. | **Yes, with explicit user consent and manifest capability.** `UserNotificationListener` returns current toast notifications and add/remove events. | The consented listener can remove one notification or clear notifications. It does not document a pre-display veto. | Supported as an optional, non-elevated, packaged notification service. Permission revocation must degrade to Windows UI immediately. |
| Toast presentation | We own notifications sent by our app, not the global toast presenter. | Listener events can drive a mirror after Windows accepts a notification. | A sender can suppress its own popup. We cannot set that flag for another sender. A user-controlled Do Not Disturb mode can suppress banners globally and route them to Notification Center. | A custom presenter is opt-in and must be described as a mirror, not a transparent system replacement, until latency, payload, action, alarm, lock-screen, and reboot behavior pass a spike. |
| Notification Center | Explorer/Shell owns the system surface. | Current app notifications are observable through the consented listener; the Center's complete UI/model is not exposed. | A documented Group Policy can remove Notifications and Action Center from the taskbar, but Windows says banners still appear and missed notifications cannot be reviewed. | Keep Windows Center as recovery. Policy hiding is owner-enabled only, edition-checked, reversible, and blocked on a working accessible replacement. |
| Quick Settings | Explorer/Shell owns the surface. | Window events/UIA may reveal a transient surface but are not a stable provider API. | Policy can remove Quick Settings and requires reboot. No documented host/replacement API was found. | Build our own quick controls from public per-feature APIs. Hiding the Windows surface is a separate reversible deployment choice, not runtime takeover. |
| Volume, brightness, and media OSDs | We may own replacement overlays and the commands that update a public device API. | WinEvents can observe shell UI best effort. SMTC lets a media app provide metadata and respond to the built-in media UI. | No public API was found that globally disables or rehosts all system OSDs. Consuming a documented low-level hardware-key event before the shell sees it may work for some keyboards and needs a device matrix. | Replacement UI is supported; suppression is spike-only. If a key cannot be exclusively consumed, keep Windows OSD and avoid a duplicate manager OSD. |
| Other shell flyouts | Explorer/Shell owns Start, calendar, network, accessibility, hidden-icons, and related flyouts. | Out-of-context WinEvents/UIA can observe visible UI but event coverage is not an ownership contract. | No general documented replacement or suppression API was found. Some individual system features have settings URIs or feature APIs. | Provide first-party equivalents where useful and let Windows remain the fallback. Never bind production logic to private window class names or private COM. |
| Taskbar | Explorer owns the Windows taskbar. Our bar owns its own window. | AppBar notifications and ordinary window observation provide coordination. | AppBar is the documented cooperative reservation. Shell Launcher is the documented full Explorer-replacement route, but only on Enterprise, Education, and IoT Enterprise editions. | Retain Explorer on this Windows Home installation. Our AppBar is primary; the taskbar remains auto-hidden recovery UI. |
| Tray / notification area | Explorer owns the host. An application owns only the icon identity it submits. | There is no documented enumeration/subscription contract for all third-party tray icons. | `Shell_NotifyIcon` can add, modify, or delete **our** icon. User visibility choices cannot be programmatically controlled. Shell Launcher does not document a tray-host compatibility contract for custom shells. | Ship our own status controls and optional tray icon. Do not promise third-party tray rehosting without an isolated compatibility experiment. |

## Manager-created shell surfaces

The manager can fully own its AppBar, palette, overview, scratchpads, notification mirror, and quick-control/OSD windows. Win32 defines popup, tool, no-activate, layered, and topmost styles; z-order is still shared policy rather than an exclusive plane. A topmost surface is above non-topmost windows but not inherently above every other topmost surface ([window features and z-order](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features)). The AppBar contract is the supported way to reserve desktop work area and requires fullscreen z-order cooperation ([application desktop toolbars](https://learn.microsoft.com/en-us/windows/win32/shell/application-desktop-toolbars)).

GPUI, `gpui-base`, or `gpui-components` can render these surfaces, but choosing a renderer does not satisfy the Windows accessibility contract. Custom controls must expose UI Automation providers, properties, control patterns, navigation, focus, and events; standard controls receive this support from framework proxies, while custom controls do not ([UI Automation providers](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview), [server-side provider requirements](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-serversideprovider), [accessibility best practices](https://learn.microsoft.com/en-us/windows/win32/winauto/accessibility-best-practices)). A GPUI surface cannot replace a system surface until keyboard-only operation, Narrator/UIA, high contrast, text scaling, reduced motion, and per-monitor DPI pass alongside performance.

## Third-party dialogs and popups

Use `SetWinEventHook` with `WINEVENT_OUTOFCONTEXT` to observe the current desktop without injecting code. Windows queues out-of-context callbacks in order, requires a message loop, and warns about callback reentrancy ([`SetWinEventHook`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)). Combine `EVENT_OBJECT_CREATE/SHOW/HIDE/DESTROY/LOCATIONCHANGE`, foreground changes, owner/style inspection, and UI Automation. Do not rely on `EVENT_SYSTEM_DIALOGSTART/END` or menu popup events alone: Microsoft explicitly documents them as inconsistent ([WinEvent constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants)).

Win32 owned windows stay above their owner, hide with it, and are destroyed with it; dialogs and message boxes are owned by default ([owned-window rules](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#owned-windows)). Although `SetWindowLongPtr(GWLP_HWNDPARENT)` exposes an owner mutation for ordinary same-integrity top-level windows, it fails upward across UIPI and changing a foreign relationship couples its lifetime/z-order to our process ([`SetWindowLongPtr`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw)). It must not be used to claim popups.

The production policy is therefore:

1. Associate a popup with its app and owner chain.
2. Preserve application modality and focus intent.
3. Float and center recognized dialogs inside the owner's monitor/workspace, and keep owned sets together during workspace moves.
4. Tile only explicit app-specific utility windows whose behavior is proven.
5. Never hide, clone, close, reparent, or replace an unknown dialog automatically.
6. Treat destroyed/recreated handles as new identities and tolerate inaccessible/elevated controls.

## Notifications and custom presentation

`UserNotificationListener` is the one documented cross-app notification API. It requires a User Notification Listener manifest capability and an explicit permission prompt from a UI thread. Permission can be revoked; Microsoft says operations may then fail silently, including returning an empty list. The listener retrieves the user's **current** toast notifications, signals additions/removals, and can remove or clear them ([notification listener guide](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener), [`UserNotificationListener`](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.management.usernotificationlistener?view=winrt-28000)). It is not a durable historical archive and does not document interception before Windows decides to display a banner.

The sending application controls whether its own toast popup is suppressed through `ToastNotification.SuppressPopup` or the Windows App SDK equivalent `AppNotification.SuppressDisplay`; those values are part of the sender's notification object, not a global listener control ([`ToastNotification`](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification?view=winrt-28000), [`AppNotification`](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.windows.appnotifications.appnotification?view=windows-app-sdk-1.8)). Windows also configures notification attribution and does not allow the sender to override that shell-owned area in toast XML ([app notification content](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-content)).

The supported custom experience is thus a cooperative mode:

1. Explain the data access and request notification-listener permission. Never request it during background startup.
2. Let the owner explicitly enable Do Not Disturb/Focus Assist. Microsoft documents that suppressed notifications go directly to Action Center/Notification Center ([Focus Assist](https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/focus-assist)). Do not silently toggle or emulate this setting through private APIs.
3. Mirror listener additions into an accessible manager overlay and maintain a private local history only when the owner opts in.
4. Remove a system notification only after the mirrored state is durable and the owner dismisses it; never clear all notifications as routine synchronization.
5. If access is denied/revoked, the listener falls behind, the service crashes, or the replacement is unhealthy, stop presenting our mirror and leave Windows notifications untouched.

This needs a packaged, non-elevated UI/service boundary. Current Windows App SDK guidance says app notifications are unsupported for an elevated app and fail silently, reinforcing the existing plan to keep presentation outside any elevated broker ([app notification quickstart](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-quickstart)).

Notification action parity is unproven. The listener exposes app identity, time, ID, and notification visual data, but the documented guide does not promise that every original toast action can be rehosted and invoked. Urgent/alarm notifications, inline replies, progress updates, images, private content, lock-screen delivery, reboot persistence, and listener-event latency must be measured before Windows banners are routinely suppressed.

## Notification Center, Quick Settings, OSDs, and flyouts

Microsoft exposes policy switches, not provider interfaces, for two major Windows 11 surfaces. `Remove Notifications and Action Center` removes the taskbar entry but explicitly leaves toast banners visible and prevents reviewing missed notifications. `Remove Quick Settings` removes the system-tray surface and requires a reboot ([taskbar policy settings](https://learn.microsoft.com/en-us/windows/configuration/taskbar/policy-settings#remove-notifications-and-action-center), [Quick Settings policy](https://learn.microsoft.com/en-us/windows/configuration/taskbar/policy-settings#remove-quick-settings)). These controls prove suppression is a deployment policy, not an API for taking over the existing surface. Their CSP/GPO and Windows-edition applicability must be checked on the actual installation; this Windows Home target cannot assume enterprise policy availability.

We can still provide a better manager-owned quick-control surface using documented feature APIs, for example Core Audio for volume and per-feature Windows APIs or Settings URIs for network, display, Bluetooth, accessibility, and power. That work needs a capability matrix because devices differ: desktop monitors often expose no software brightness control, radios can be policy-managed, and some settings require an elevated or brokered operation. Launching a Settings URI is documented fallback behavior, not ownership of Quick Settings ([launch Windows Settings](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings)).

System media transport controls let a media application integrate metadata and commands with the built-in system UI; they do not expose a global OSD presenter ([`SystemMediaTransportControls`](https://learn.microsoft.com/en-us/uwp/api/windows.media.systemmediatransportcontrols?view=winrt-28000)). No documented public API was found for globally suppressing or rehosting volume, brightness, media, Start, calendar, network, accessibility, or hidden-icons flyouts. This is a qualified public-API finding, not proof that every device path behaves identically.

The useful OSD experiment is at the input boundary. A documented low-level keyboard hook can consume a keystroke by returning a nonzero value, but Windows may time it out, some media/brightness keys arrive through different HID/firmware paths, and global hooks must never stall input ([low-level keyboard procedure](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)). Test whether consuming each actual device key prevents both its system action and OSD; if it does, invoke the public volume/brightness command and render one manager OSD. If not, send no duplicate manager OSD. Private shell class detection may be diagnostic telemetry only.

## Taskbar, tray, and Explorer boundary

Our AppBar is the supported taskbar-like surface. Explorer remains the host for the Windows taskbar, Start, notification area, and related shell UI. `Shell_NotifyIcon` lets each application add, modify, and delete its own icon; user visibility cannot be programmatically controlled, and Microsoft says only the user chooses which icons appear ([notification area](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area), [taskbar extensions](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar-extensions#notification-area)). There is no documented API to enumerate, receive, or rehost every other application's tray icon. An accessible set of manager-owned status indicators is safe; a full tray clone is not yet specifiable.

Shell Launcher is Microsoft's documented Explorer-replacement feature. It launches a custom shell after sign-in, monitors it, and can restart it, restart/shut down the device, or do nothing when it exits. It is intended for purpose-specific devices and is available only on Enterprise, Education, and IoT Enterprise editions, not this Home installation ([Shell Launcher overview](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/), [startup and exit behavior](https://learn.microsoft.com/en-us/windows/configuration/shell-launcher/configure#shell-launcher-startup-and-exit-behavior)). Even Shell Launcher does not document a third-party tray-host protocol or grant access to DWM's global scene. Replacing Explorer would make us responsible for shell startup, recovery UI, app launching, task switching, taskbar/tray equivalents, accessibility, and safe rollback while still leaving DWM, UAC, logon, and secure desktop to Windows.

Therefore:

- **Now:** retain Explorer; auto-hide its taskbar; make our AppBar and control surfaces primary.
- **Later spike:** use a disposable Enterprise/Education VM and Shell Launcher, not registry shell replacement on the daily machine. Establish what system components survive and what must be recreated.
- **Never as production foundation:** kill/restart Explorer to hide UI, emulate undocumented `Shell_TrayWnd` messages, inject into Explorer/ShellExperienceHost, or patch DWM.

Explorer reconstruction is a normal recovery boundary. It broadcasts `TaskbarCreated`; applications must assume their notification icons were removed and add them again ([taskbar creation notification](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar#taskbar-creation-notification)). Our AppBar already survived two measured Explorer restarts, but AppBar registration, our tray icon, shell-surface observations, and work-area state must all reconcile idempotently after that message, DPI/display churn, and process restart.

## Security, integrity, consent, and recovery

- **Integrity:** keep the main manager and UI at medium integrity. UIPI blocks some mutations and messages toward higher-integrity windows; elevated apps receive explicit degraded behavior. Any elevated broker must expose narrow typed operations and contain no renderer, notification listener, generalized UI Automation, or input injection.
- **UIAccess:** do not use it. Microsoft restricts UIAccess to signed assistive technology installed in a secure location and explicitly says it should not be used by applications merely wanting to appear above other Windows UI. It still does not grant access to SYSTEM/secure-desktop surfaces ([UI Automation security](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)).
- **Secure surfaces:** UAC consent, logon, lock, Ctrl+Alt+Delete, and other secure-desktop UI remain Windows-owned. Our overlays must disappear or naturally become unavailable and recover on the interactive desktop afterward.
- **Consent:** listener access, Do Not Disturb, private notification history, policy suppression, and any Explorer-replacement test are separate explicit owner choices. One choice must not imply the others.
- **Accessibility:** every replacement must be at least as keyboard and UIA accessible as the Windows surface it displaces. Failure keeps the Windows surface enabled.
- **Recovery:** split the core manager from optional shell UI processes. Register eligible UI processes for Windows Application Restart, but do not depend on it as the only recovery mechanism ([Application Restart](https://learn.microsoft.com/en-us/windows/win32/recovery/registering-for-application-restart)). Keep an external stable launcher/watchdog, a no-shell-surfaces safe mode, last-known-good policy state, and rollback before applying any policy that hides Windows recovery UI.
- **Observation failure:** WinEvents are asynchronous and can be inconsistent; UIA providers can hang, omit controls, or disappear. Use timeouts, generation identities, and ordinary HWND classification as complementary signals. Observation must never block the window-event loop.
- **Unsupported hooks:** out-of-context WinEvents and a bounded low-level input hook are documented. Cross-process subclassing, DLL-injected global hooks, private COM, UIAccess misuse, and window-class-dependent Shell manipulation are excluded even if a single build appears to accept them.

## Smallest safe experiments

### S1: Consented notification mirror and presentation

Build a disposable packaged, non-elevated listener plus one accessible manager-owned popup. After an explanatory screen, let the owner grant permission and manually toggle Do Not Disturb. Generate at least 100 notifications from packaged/unpackaged Win32, Windows App SDK, browser, chat, progress, image, inline-reply, priority/alarm, and reboot-persistent senders. Measure sender timestamp to listener event and first manager frame, duplicate Windows banners, payload/action coverage, update/removal ordering, revocation, service restart, lock/unlock, reboot, and Notification Center reconciliation. Never clear globally during the test. Pass for custom presentation only if there are no duplicate banners in the opted-in mode, p95 first-frame latency is within a stated interaction budget, urgent Windows-owned alerts are not lost, dismissals converge, and every unsupported action gets a truthful "open in app" fallback.

### S2: Popup/dialog ownership census

Run only out-of-context WinEvents and read-only UIA/Win32 inspection for a week across Win32, WinUI, Chromium, Electron, Java, games, file pickers, UAC attempts, custom menus, tooltips, and transient windows. Record owner chain, styles, automation control type, event sequence, modality, integrity, lifetime, and whether generic centering would have been correct. Then enable placement only for recognized disposable test apps. Pass the generic policy only if it never separates an owned set, steals focus, tiles menus/tooltips, or strands a disabled owner. Keep per-application escape rules regardless.

### S3: Hardware key and OSD matrix

With a watchdog that unhooks on heartbeat loss, test each keyboard/headset/monitor volume, mute, brightness, play, and transport key in three modes: observe only, consume only, then consume plus the equivalent public device command. Record hook visibility, whether the native action occurs, whether the Windows OSD appears, repeat behavior, latency, secure desktop, sleep/resume, Bluetooth reconnect, and hook timeout. A manager OSD may replace Windows only for input routes that are exclusively consumed and exactly reproduced; all others retain Windows presentation.

### S4: Reversible shell-surface policy

In a disposable VM matching the target Windows edition, snapshot policy and Explorer state, enable one documented policy at a time for Notification Center and Quick Settings, reboot when required, and verify our replacement, Settings fallback, Narrator, keyboard-only control, crash recovery, external rollback, upgrade, and exact restoration. Do not run this on the daily machine until the stable launcher can restore policy without the manager UI.

### S5: Explorer replacement and tray compatibility

In an Enterprise/Education VM only, configure Shell Launcher through its documented CSP/WMI route. Inventory application startup, task switching, Win32/UWP launch, Settings, notification delivery/listener behavior, Shell_NotifyIcon calls, notification area absence, system flyouts, UAC, lock/unlock, multi-monitor, crash/exit actions, update, and rollback to Explorer. This spike decides whether a future edition change creates a credible personal shell. It must not attempt undocumented tray-message emulation or DWM modification.

## Newly specifiable tickets

### Build the accessible shell-surface host

**Question:** What process, window-style, z-order, UI Automation, safe-mode, and restart contract lets AppBar, palette, overview, notification, quick-control, and OSD surfaces share one native host without coupling their failure to the manager core?

This can be specified now. Reuse the AppBar lifecycle and manager-owned composition results. Include an egui/GPUI parity harness; GPUI adoption requires UIA and recovery parity, not only rendering performance.

### Define popup and dialog coordination policy

**Question:** How does the manager classify owner chains, modality, menus/tooltips, utility popups, and transient windows so it can center or float useful dialogs without stealing focus, separating owned sets, or suppressing application state?

This can be specified now, with S2 as its acceptance harness rather than a broad research blocker.

### Build a consented notification mirror and private history

**Question:** What permission, packaging, data-retention, action fallback, latency, dismissal, accessibility, and recovery contract lets the owner use a manager notification surface while Windows remains a lossless fallback?

The listener and storage model can be specified now. Making it the primary presenter is gated by S1 and an explicit Do Not Disturb choice.

### Build native quick controls and prove OSD replacement routes

**Question:** Which public APIs and brokers can read and change volume, mute, brightness, media, network, Bluetooth, accessibility, display, and power state on the owner's hardware, and which hardware inputs can be consumed without duplicate or lost system behavior?

Specify the surface and public feature adapters now. Gate per-device OSD suppression on S3; unsupported routes must delegate to Windows.

### Evaluate an opt-in personal shell profile

**Question:** Can documented policy and, on a supported Windows edition, Shell Launcher remove enough duplicate Explorer UI while preserving startup, tray-dependent applications, notifications, Settings, accessibility, secure desktop, updates, and one-step recovery?

This remains a VM-only spike ticket using S4/S5. It must not block the manager-owned shell host or assume a Windows edition upgrade.

No broader shell-ownership research is needed before these specifications. The current production boundary is clear: own our windows, observe and coordinate foreign windows, mirror notifications with consent, retain Explorer as recovery, and experimentally narrow only the documented policy/input/shell-replacement frontiers.
