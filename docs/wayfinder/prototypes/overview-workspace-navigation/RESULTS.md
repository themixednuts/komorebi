# Overview and workspace-navigation result

## Decision

Use the **Spatial map** as the only overview. Preserve each monitor's physical shape, show its manager workspaces and container topology, and expose window, container, workspace, and scratchpad destinations directly. Incorporate the Focus board's keyboard navigation and explicit selection detail without retaining a second mode.

Reject the Task View hybrid. Its flat gallery makes individual windows easy to scan but loses container hierarchy, cross-monitor geometry, and exact manager drop targets. Keep Windows Task View unchanged for Windows virtual-desktop creation, naming, ordering, and switching. The manager overview owns only concepts Task View cannot represent.

Windows outside the current Windows desktop visibility domain may be identified from public observation, but the overview neither switches to them nor invokes private virtual-desktop APIs. It directs the owner to Task View instead.

## Measured Windows result

Five warmed release runs used 12–13 eligible live top-level source windows on Windows build 26200 at 5120×1440 / 239 Hz. The revised probe receives foreground changes from `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` and sleeps inside `MsgWaitForMultipleObjectsEx`; it does not sample foreground state on an interval.

| Workload | Median registration | Median registration + caller DWM flush | Median per-run p95 full-slot update |
|---|---:|---:|---:|
| 20 thumbnail slots | 4.158 ms | 7.495 ms | 7.676 ms |
| 50 thumbnail slots | 8.844 ms | 13.684 ms | 9.475 ms |

`DwmFlush` waits for composition work queued by this process. It is not proof that every pixel reached the monitor. The measurements establish that live DWM thumbnails are practical for a static overview, but not that all thumbnail rectangles should change every 4.18 ms frame. Keep thumbnail geometry stable. Animate owned GPUI/DirectComposition visuals such as selection, dimming, and drag targets around them. `DwmFlush` is a measurement fence here, not a production frame-loop call.

Foreground activation succeeded 5/5 times when the caller-owned overview first held foreground input. Median event-observed foreground settlement was 3.145 ms. This validates the intended hotkey/click path on this machine, not unrestricted background focus. Production attempts activation once from current user input, dismisses the overview first, and reports failure instead of retrying.

## First-frame and privacy policy

The first complete overview frame does not wait for every live registration. It contains the full shell and a current-generation placeholder for every preview slot. Live DWM thumbnails promote individually as they become ready. This gives a complete non-desktop frame immediately while respecting the measured 50-slot registration tail.

A successful registration does not prove that DWM produced useful pixels. The public thumbnail API does not report black or stale output, so the manager must not claim it can detect those cases. Known privacy denial, registration failure, source destruction, or source invalidation becomes a labeled placeholder. Every card keeps an owned label and actions independent of its pixels. There is no fallback to `PrintWindow`, `BitBlt`, or desktop duplication.

## Production boundary

Create one owned overview host per monitor within one interactive shell session. A renderer-neutral `OverviewSnapshot` carries stable manager identities to GPUI. The Win32 adapter alone owns `HWND`, DWM thumbnail registration, foreground activation, WinEvent subscriptions, and teardown. DWM thumbnails are visual resources; all hit testing and drag targets belong to the owned overview surface.

The native probe and HTML are disposable. They do not move, hide, resize, or restyle source windows. Raw runs are recorded in `measurements.json`.

## Evidence sources

- Microsoft documents the owned-destination and same-process lifetime rules for [`DwmRegisterThumbnail`](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail) and [`DwmUnregisterThumbnail`](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmunregisterthumbnail).
- [`SetWinEventHook`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) provides sequential out-of-context delivery on the installing message-loop thread; [`MsgWaitForMultipleObjectsEx`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-msgwaitformultipleobjectsex) supplies the blocking queue wait.
- The public [`IVirtualDesktopManager`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager) surface exposes membership queries and moving a window, but not Task View's desktop lifecycle or switching controls.
- Rust's [`OsStrExt` and `OsStringExt`](https://doc.rust-lang.org/std/os/windows/ffi/index.html) preserve ill-formed UTF-16 across Windows round trips. The [Corrode systems-code audit](https://corrode.dev/blog/bugs-rust-wont-catch/) supplies the separate no-lossy-conversion and no path TOCTOU review checks.
