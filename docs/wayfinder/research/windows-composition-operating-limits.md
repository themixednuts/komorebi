# Windows composition operating limits on the target machine

Research for [Wayfinder issue #22](https://github.com/themixednuts/komorebi/issues/22), performed on 2026-08-27. The target is a Windows 11-first personal Windows manager. All live probes used disposable windows or read-only queries, except for two controlled Explorer restarts. They changed no persistent shell setting and did not restart or reconfigure the installed manager.

## Decision

Use a manager-owned DWM-thumbnail surface for workspace-scale transitions and overview. Apply the real window topology once at a transition boundary, preferably as one `DeferWindowPos` batch. Keep direct real-window interpolation for small intra-workspace changes only after representative applications meet a separate frame and WinEvent budget.

The narrow reliable contract is:

- A DWM thumbnail is a visual representation inside a window we own. It does not move the source `HWND`, redirect source input, or guarantee that content is available. Registration success is not proof of usable pixels.
- A real move changes application geometry and generates application-visible position traffic. It must be treated as state mutation, not merely animation.
- Acrylic, custom shadow, glow, and transition visuals belong to manager-owned windows. A visual that follows another process is a separately positioned top-level window and can lag or be reordered.
- DWM corner preference is a discretionary hint. It is suitable as the default request, but custom foreign-window clipping is not yet a production contract.
- Public virtual-desktop APIs are suitable for passive filtering after a user-driven desktop switch. They do not provide enumeration, switching, ordering, or change notifications.
- The installed AppBar survives Explorer reconstruction on this machine. Recovery must still be idempotent and tested with a second AppBar before PowerToys Dock integration is considered safe.
- Any temporary foreign-window opacity change requires an exact restoration record. An existing layered window whose complete rendering state is unknown must not be modified.
- Protected or unavailable content becomes a labeled placeholder. Capture fallbacks must never be used to bypass display affinity.

## Test environment

| Item | Observed value |
| --- | --- |
| Windows | 11 25H2, build `26200.9168` |
| DWM | `DwmIsCompositionEnabled` returned `S_OK`, enabled |
| GPU | NVIDIA GeForce RTX 3090, driver `32.0.15.9649` |
| Display | 5120×1440 at 239 Hz; nominal refresh interval 4.184 ms |
| Session policy | Local session, transparency enabled, high contrast inactive |
| Windows App Runtime | 1.4 through 1.8 and 2.x packages present; an Acrylic tracking harness was not installed |
| Installed manager | `komorebi.exe` PID 36664 and `komorebi-bar.exe` PID 33472 during probes |

These are single-machine baselines, not portable performance budgets. The probes used PowerShell/P/Invoke and disposable WinForms windows, so their absolute CPU costs include managed interop and message-pump overhead.

## Measured results

| Probe | Result | What it establishes |
| --- | --- | --- |
| Real-window motion | 240 `SetWindowPos` steps: total-step p50 4.368 ms, p95 9.014 ms, max 13.841 ms; `DwmFlush` p50 3.291 ms, p95 6.375 ms, max 7.447 ms; source received 243 `WM_WINDOWPOSCHANGED` and 3 `WM_PAINT` messages | Direct interpolation crossed two refresh intervals at p95 and mutated source geometry every step. The low paint count for a solid WinForms window does not generalize to Chromium, WinUI, terminals, games, or resize-heavy applications. |
| Thumbnail-only motion | 240 `DwmUpdateThumbnailProperties` steps: registration/update `S_OK`; total-step p50 4.176 ms, p95 5.004 ms, max 8.337 ms; `DwmFlush` p50 4.020 ms, p95 4.628 ms, max 6.650 ms; source received 0 position and 0 paint messages | Thumbnail motion was more consistent and isolated the source application. It still missed the one-refresh budget at p95, so the production surface needs composition-thread animation rather than a PowerShell loop. |
| Opacity cancellation | Disposable ordinary window accepted alpha 173. Restoring the original extended style changed it exactly from `0x10100` back to `0x10100` | Exact cancellation is possible when the original style is known and the window did not already use a layered rendering path. |
| DWM corner preference | Get/set/readback of `DWMWA_WINDOW_CORNER_PREFERENCE=DWMWCP_ROUND` all returned `S_OK`; readback was 2; original value 0 was restored | The preference is accepted on this build. It does not prove visible clipping. |
| Passive virtual-desktop query | Normal ChatGPT window: both public queries `S_OK`, current=true, stable non-null GUID; combined query p50 0.1411 ms, p95 0.4048 ms, max 8.3814 ms over 1,000 iterations | Polling a modest tracked-window set at 100 ms is operationally cheap at rest. It does not measure settling after a desktop switch. |
| Pinned/shell-like virtual-desktop query | AppBar: current query `S_OK`, but `GetWindowDesktopId` returned `0x8002802B` (`TYPE_E_ELEMENTNOTFOUND`) and `GUID_NULL` | A missing desktop ID is a normal state that must not be treated as manager failure. |
| Explorer/AppBar recovery, cycle 1 | Explorer PID 12152 became 37476. The bar PID and probed `HWND` remained valid. New Explorer and the 50 px top reservation were observed by 255.0 ms. Eleven 20 ms samples all reported `rcWork.top=50`; bar rectangle remained `(0,0)-(5120,50)` | No visible reservation gap or duplicate work-area shrink was observed at the probe resolution. This is an upper bound from polling, not a timestamp of the `TaskbarCreated` broadcast. |
| Explorer/AppBar recovery, cycle 2 | Explorer PID 37476 became 42240. Bar PID stayed 33472, `rcWork.top` was 50, and the restored state was observed by 139.6 ms | A second registration cycle did not accumulate another 50 px reservation. |
| Non-activating topmost overlay | A disposable `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` window shown without activation and promoted with `HWND_TOPMOST | SWP_NOACTIVATE` preserved foreground `HWND 0x70342`; topmost state was observed and then removed | Correct create/show semantics preserve focus. `SWP_NOACTIVATE` cannot repair focus already stolen by an activating `Show`; the overlay must be non-activating from creation. |
| Capture exclusion | A blue source and its DWM thumbnail were visible to `CopyFromScreen`. After the source set `WDA_EXCLUDEFROMCAPTURE`, thumbnail registration/update still returned `S_OK`, but sampled source and thumbnail pixels were both black. Affinity was then restored to `WDA_NONE` | Success HRESULTs cannot identify protected or unavailable thumbnail content. A black/unavailable-content detector and placeholder path are required. |

`DwmRegisterThumbnail` creates a relationship between top-level windows, and `DwmUpdateThumbnailProperties` changes the destination rectangle, crop, opacity, and visibility ([registration](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail), [updates](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmupdatethumbnailproperties)). In contrast, `SetWindowPos` changes the actual window's placement, z-order, size, and activation state ([`SetWindowPos`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)). `DwmFlush` only waits for composition work queued by the calling process; it is not a whole-session presentation fence ([`DwmFlush`](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmflush)).

## Workspace transition operating contract

The measured result favors thumbnail motion, but it does not yet establish a shippable 20-window transition. The implementation ticket must use this state machine:

1. Snapshot the current workspace model, foreground window, real window rectangles, visibility, cloaking state, z-order intent, and any style mutations.
2. Create one non-activating surface per affected monitor. Register live thumbnails when available and create explicit placeholders for protected, destroyed, or unavailable sources.
3. Present the first complete overlay frame before hiding or moving any real window. Do not expose a desktop-colored intermediate frame.
4. Animate only visuals owned by the overlay. Pointer and keyboard interaction remains owned by the overlay; DWM thumbnails are not interactive child windows.
5. At the commit boundary, apply real positions and visibility as one batch. Ignore or generation-tag WinEvents caused by that commit.
6. Restore foreground state, remove overlays, unregister every thumbnail, and discard the restoration snapshot.
7. On cancellation, source destruction, monitor change, or device loss, restore the snapshot exactly before removing the overlay.

The production harness must repeat at 20 and 50 source windows across Win32, Chromium, WinUI/UWP-hosted, terminal, Java, minimized, cloaked, elevated, destroyed-mid-transition, mixed-DPI, fullscreen, HDR, and capture-excluded cases. Record command-to-first-complete-frame, p50/p95/max frame interval, CPU/GPU, committed geometry error, WinEvent count, black-frame count, focus restoration, and cancellation equality. Pass criteria are no desktop flash, no stale protected pixels, p95 first complete frame within two refresh intervals, no source position traffic before commit, and byte-for-byte equality of the restoration snapshot after cancellation.

Use `BeginDeferWindowPos`/`DeferWindowPos`/`EndDeferWindowPos` for the boundary batch because Windows documents those calls as the way to change several windows' placement together ([window positioning](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#window-position)). This is atomic in intent, not a documented compositor transaction, so the harness still must detect partial application and recover.

## Optional Acrylic tracking

Desktop Acrylic is supported only on a window we own. Its controller owns material policy and can fall back when composition conditions do not permit Acrylic. Microsoft characterizes Acrylic as a live blurred view of content behind a window and documents fallbacks for Remote Desktop or virtualized environments, insufficient hardware, disabled transparency, Battery Saver, and high contrast ([materials policy](https://learn.microsoft.com/en-us/windows/apps/develop/ui/materials), [`DesktopAcrylicController`](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.composition.systembackdrops.desktopacryliccontroller)). Microsoft publishes no maximum delay for an Acrylic window externally tracking another `HWND`.

No Acrylic tracking result is claimed yet. Run it only if third-party-following blur remains desirable:

- Use a packaged, disposable Windows App SDK 1.8 harness with one `DesktopAcrylicController` target and a separately owned checkerboard target window.
- Obtain the target's `DWMWA_EXTENDED_FRAME_BOUNDS` on each location event, schedule the overlay position on one dedicated UI thread, and log QPC timestamps for the source frame, requested overlay frame, and captured result.
- Move and resize at 60, 120, and 239 Hz under idle, CPU load, and GPU load. Repeat during maximize, snap, fullscreen, DPI crossing, monitor crossing, target destruction, and Explorer restart.
- Repeat with transparency disabled, Battery Saver, high contrast, RDP, and a forced fallback color. Those policy variants are observational; the harness must restore settings it changes or instruct the owner to toggle them manually.
- Pass only if p95 tracking error is at most one refresh interval, the overlay never appears above an unrelated topmost window, and every fallback is opaque and deterministic. Otherwise retain Acrylic for stationary manager-owned UI only.

## DWM corners and hard clipping

Windows explicitly describes corner preference as a hint. DWM does not round maximized or snapped windows and may not round windows in VM/AVD/WDAG scenarios. Per-pixel-alpha layered windows and region-shaped windows are not eligible for system rounding ([rounded-corner guidance](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners), [`DWM_WINDOW_CORNER_PREFERENCE`](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_window_corner_preference)). `SetWindowRgn` provides hard geometric clipping, but it changes the window region Windows uses for drawing and hit testing ([`SetWindowRgn`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowrgn)).

The set/readback probe succeeded, but two pixel-classification attempts were invalidated because the active manager retiled or reordered the disposable windows between geometry capture and screen sampling. This is evidence that the test harness itself must be manager-owned or explicitly unmanaged; those samples are not clipping evidence.

Before enabling any foreign-window clipping, build an in-process unmanaged harness that records both `GetWindowRect` and `DWMWA_EXTENDED_FRAME_BOUNDS`, with a known owned backing window. Classify corner pixels and hit tests for ordinary framed, custom-frame, Chromium, WinUI, Java, existing layered, region-shaped, snapped, maximized, and borderless windows at every active DPI. Restore the original DWM preference and original region after every case. Default behavior may request DWM rounding; `SetWindowRgn` must remain per-application opt-in unless the full visual/input/restore matrix passes.

## Passive Windows virtual-desktop settling

The public `IVirtualDesktopManager` exposes only current-membership query, desktop-ID query, and move-to-known-desktop methods ([interface](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager), [membership query](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ivirtualdesktopmanager-iswindowoncurrentvirtualdesktop), [desktop ID](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ivirtualdesktopmanager-getwindowdesktopid)). It exposes no enumeration, current-desktop GUID, ordering, switching, or public change notification.

At-rest query latency is low enough for passive polling, but switch settling remains intentionally unmeasured because this ticket did not synthesize a desktop-switch command or use undocumented COM. The reproducible test is:

1. Track 30 known normal, pinned, minimized, cloaked, UWP-hosted, and elevated top-level windows.
2. Poll both public queries at 16, 100, and 500 ms.
3. Have the owner switch desktops with the normal Windows gesture ten times in each direction. Do not switch from the manager or test harness.
4. Log HRESULT, ID, current membership, cloak state, foreground window, CPU time, and the time from the user input marker until all results remain unchanged for three polls.
5. Repeat once after Explorer restart.

The manager may use the result to debounce events from windows outside the current Windows desktop. It must accept `GUID_NULL` and `TYPE_E_ELEMENTNOTFOUND` as unclassified/pinned states and must never couple its workspace identity to a Windows desktop GUID.

## AppBar recovery and PowerToys Dock coexistence

An AppBar registers and negotiates its rectangle using `ABM_NEW`, `ABM_QUERYPOS`, and `ABM_SETPOS`; the Shell maintains the AppBar list and notifies bars when positions change ([AppBar contract](https://learn.microsoft.com/en-us/windows/win32/shell/application-desktop-toolbars), [`SHAppBarMessage`](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shappbarmessage)). Explorer broadcasts the registered `TaskbarCreated` message after reconstructing the taskbar ([taskbar lifecycle](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar)). Re-registering an AppBar on that message is a measured recovery strategy, not an explicit Microsoft guarantee.

Two live cycles preserved the 50 px reservation without restarting the bar or accumulating a second reservation. The test did not observe `TaskbarCreated` directly and sampled at 20 ms, so 255.0 ms and 139.6 ms are recovery upper bounds.

PowerToys Command Palette was running after Explorer restart, but only its centered palette window was visible. Dock was not enabled, and this ticket did not change that persistent setting. Microsoft documents Dock as a persistent non-auto-hide AppBar ([PowerToys Dock](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/dock)). Before exposing any manager Dock band, run this owner-opt-in matrix:

- Capture all monitor and work-area rectangles, AppBar window rectangles, process IDs, and z-order before enabling Dock.
- Enable Dock manually with the manager AppBar already active. Test different edges first, then the same edge. Our AppBar remains the product's primary reservation; do not silently relocate or disable it.
- On each configuration, restart Explorer ten times. Record `TaskbarCreated`, `ABN_POSCHANGED`, every `ABM_NEW/QUERYPOS/SETPOS` result, work-area changes at 10 ms, duplicate reservations, z-order, fullscreen behavior, and both processes' survival.
- Test monitor hotplug, DPI change, resolution change, sleep/resume, Command Palette exit/restart, manager bar exit/restart, and both startup orders.
- Disable Dock manually and verify exact restoration of the initial work area. Do not persist a Dock setting from the harness.

Pass criteria are deterministic non-overlap, no cumulative shrink, recovery within 500 ms, correct fullscreen demotion, exact work-area restoration, and continued manager operation when PowerToys exits. A same-edge ordering result is configuration-specific; it must not become a hidden dependency.

## Overlay z-order and activation

Windows maintains separate topmost and non-topmost portions of z-order. `HWND_TOPMOST` guarantees placement above non-topmost windows, not exclusive priority over other topmost windows. Owned windows also have ordering relationships with their owner ([z-order](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#z-order), [`SetWindowPos`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)). `WS_EX_NOACTIVATE`, `WS_EX_TOOLWINDOW`, and `WS_EX_TRANSPARENT` are distinct policies, not interchangeable flags ([extended styles](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles)).

The successful probe used no-activate semantics at creation, a non-activating show, and `SWP_NOACTIVATE` for z-order changes. Therefore overlays must be created correctly from the first visible frame. The remaining conflict harness should create competing topmost windows in separate processes and test activation, owned popups, fullscreen, click-through regions, drag capture, alt-tab visibility, secure desktop, and target destruction. Enumerate actual z-order and count received clicks. Never assume that a topmost overlay is above another application's topmost surface.

## Capture fallback policy

`WDA_EXCLUDEFROMCAPTURE` is an owning-process request that omits a top-level window from supported public capture paths; Microsoft explicitly says display affinity is not DRM or an absolute security guarantee ([display affinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity)). Windows Graphics Capture can target an `HWND` through `IGraphicsCaptureItemInterop::CreateForWindow` ([HWND capture](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow)). `PrintWindow` synchronously asks the target application to render through `WM_PRINT` and can block ([`PrintWindow`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow)). Desktop Duplication must be recreated after several desktop and display transitions ([`AcquireNextFrame`](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe)).

Use this order:

1. Live DWM thumbnail for overview and transition visuals.
2. Windows Graphics Capture only when a stream or durable snapshot is actually required and the source is eligible.
3. A labeled application placeholder when content is protected, black, stale, destroyed, or unavailable.

Do not fall through to `PrintWindow`, `BitBlt`, or Desktop Duplication after display-affinity exclusion. `PrintWindow` may be offered as an application-specific compatibility path only on a bounded worker with a timeout and never for a protected source. Registration HRESULT, non-zero source size, and an old successful frame are insufficient readiness signals. The transition surface must carry content generation and timestamp metadata and must replace stale frames rather than flash them.

## Opacity restoration and crash recovery

Layered alpha is not a reversible transaction. `GetLayeredWindowAttributes` only reports state established through `SetLayeredWindowAttributes`, and it can fail for a window rendered through `UpdateLayeredWindow`. Microsoft also warns that calling `SetLayeredWindowAttributes` prevents subsequent `UpdateLayeredWindow` calls until `WS_EX_LAYERED` is cleared and set again ([get alpha](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getlayeredwindowattributes), [set alpha](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes), [per-pixel update](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow)).

The cancellation probe proves exact restoration only for an ordinary owned window. The current code path in `komorebi/src/window.rs` makes a window transparent by adding `WS_EX_LAYERED`, then makes it opaque by removing that style. It does not preserve whether the application originally owned `WS_EX_LAYERED`, which is why layered applications need exclusions today.

The production contract must be stricter:

- Before mutation, record `HWND`, process creation identity, original extended style, whether layered state is manager-owned, alpha/color-key/flags when queryable, generation, and the reason for mutation.
- Never mutate an existing layered window unless an application-specific adapter proves that its full rendering state can be restored.
- Restore on focus change, cancellation, disable, graceful shutdown, window destruction, configuration reload, and before handing ownership to a newer manager process.
- Persist a tiny crash-recovery ledger before the first mutation. On startup, restore only when the `HWND` still belongs to the same process identity and the observed state matches the manager-owned mutation. Then clear the record.
- If exact prior state is unknowable, skip opacity rather than force alpha 255 or remove `WS_EX_LAYERED`.

The remaining crash harness must use two disposable target types: one `SetLayeredWindowAttributes` window and one `UpdateLayeredWindow` per-pixel window. A separate mutator process must record state, apply opacity, and terminate abruptly. A recovery process then verifies persistence, restores from the ledger, compares styles and pixels, and exercises `HWND` reuse. Pass means exact state and imagery for the manager-owned case and guaranteed refusal for the unknown per-pixel case.

## Newly specifiable tickets

### Harden the manager AppBar lifecycle and test PowerToys Dock coexistence

**Question:** What idempotent lifecycle contract keeps the manager AppBar authoritative across Explorer reconstruction, fullscreen, display churn, process restart, and an opt-in PowerToys Dock without duplicate reservations or cumulative work-area shrink?

Use the recovery and coexistence matrix above. Block production implementation on the manager/process boundary decision in #19. The PowerToys portion is additionally blocked on explicit owner opt-in and must complete before any Dock band is added to the adapter in #24. It does not block the core command catalog.

### Define capture placeholders and an exact opacity restoration ledger

**Question:** What content-readiness, placeholder, mutation-ownership, and crash-recovery contract guarantees that protected/unavailable windows never leak or flash stale content and that every eligible third-party window returns to its exact prior style and opacity?

Block the final contract on the native-effect ownership decision in #20. Feed its transition readiness states into #15 and its overview placeholders into #18. The implementation must include the two-process crash harness before foreign-window opacity is enabled by default.

No additional broad composition research is required. The 20-window transition harness belongs in #18/#15, and the optional Acrylic and foreign-corner matrices should remain feasibility gates inside #20 rather than new standalone product promises.
