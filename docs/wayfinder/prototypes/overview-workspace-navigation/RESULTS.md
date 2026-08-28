# Overview and workspace-navigation result

## Recommendation pending owner review

Use the **Spatial map** as the default overview. Preserve each monitor's physical shape, show the active manager workspace inside it, and expose window, container, workspace, and scratchpad destinations directly. Add the Focus board's keyboard navigation and explicit selection detail without keeping it as a second product mode.

Do not use the Task View hybrid as the default. It is familiar and makes individual windows easy to scan, but its flat gallery loses container hierarchy, cross-monitor geometry, and exact manager drop targets. Keep Windows Task View unchanged for Windows virtual-desktop creation, naming, reordering, and switching. Our overview owns the manager concepts Task View cannot represent.

This is a HITL recommendation, not a closed decision. The owner must inspect the three variants and accept or redirect it before this ticket resolves.

## Measured Windows result

Five warmed release runs used 12 eligible live top-level source windows on Windows build 26200.9168 at 5120×1440 / 239 Hz.

| Workload | Median registration | Median registration + caller DWM flush | Median per-run p95 full-slot update |
|---|---:|---:|---:|
| 20 thumbnail slots | 3.925 ms | 9.846 ms | 4.692 ms |
| 50 thumbnail slots | 9.266 ms | 11.777 ms | 9.346 ms |

`DwmFlush` waits for composition work queued by this process. It is not proof that every pixel reached the monitor. The measurements establish that live DWM thumbnails are practical for a static overview, but not that all 50 thumbnail rectangles should be changed every 4.18 ms frame. Keep thumbnail geometry stable. Animate owned GPUI/DirectComposition visuals such as selection, dimming, and drag targets around them.

Foreground activation succeeded 5/5 times when the caller-owned overview first held foreground input. Median observed foreground settlement was 3.045 ms. This validates the intended hotkey/click path on this machine, not unrestricted background focus. Production attempts activation once from current user input, dismisses the overview first, and reports failure instead of looping.

## First-frame and privacy policy

The first complete overview frame does not wait for every live registration. It contains the full shell and a current-generation placeholder for every preview slot. Live DWM thumbnails promote individually as they become ready. This gives a complete non-desktop frame immediately while respecting the measured 50-slot registration tail.

A successful registration does not prove visible content. Protected, black, stale, minimized-unavailable, and destroyed sources become labeled placeholders by the next frame. There is no fallback to `PrintWindow`, `BitBlt`, or desktop duplication for protected content.

## Production boundary

Create one owned overview host per monitor. A renderer-neutral `OverviewSnapshot` carries stable manager identities to GPUI. The Win32 adapter alone owns `HWND`, DWM thumbnail registration, foreground activation, and teardown. DWM thumbnails are visual resources; all hit testing and drag targets belong to the owned overview surface.

The native probe and HTML are disposable. They do not move, hide, resize, or restyle source windows. Raw runs are recorded in `measurements.json`.
