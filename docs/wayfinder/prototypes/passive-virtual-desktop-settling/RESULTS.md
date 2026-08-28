# Measurement ledger

## Verdict

Use a documented out-of-context WinEvent hook as the wake-up edge, then run a bounded 16 ms verification burst over public per-window APIs until three cohort snapshots agree. Disarm the sampler immediately after settlement. Do not poll while dormant.

The primary wake is `EVENT_OBJECT_NAMECHANGE` filtered to the current `GetDesktopWindow()` HWND. Microsoft PowerToys FancyZones uses the same Explorer behavior to detect virtual-desktop switches. `EVENT_OBJECT_CLOAKED` and `EVENT_OBJECT_UNCLOAKED`, filtered to managed HWNDs, are corroborating wake sources. `EVENT_SYSTEM_DESKTOPSWITCH` did not fire because Task View virtual desktops are not Win32 input desktops.

This preserves a public state boundary: WinEvent says “re-observe now”; `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` and `DwmGetWindowAttribute(DWMWA_CLOAKED)` provide the facts. No undocumented shell COM is required.

## Environment and method

- Windows 11 25H2, build 26200.9168; Explorer 10.0.26100.8117.
- 5120×1440 desktop and a live komorebi session.
- Two Task View desktops.
- Cohort: 28 controlled HWNDs, one packaged representative, one elevated representative, and two ordinary representatives.
- Ten settled transitions at each 16, 100, and 500 ms interval, both before and after an Explorer restart.
- Three equal snapshots required for settlement.
- Each polling snapshot made both documented `IVirtualDesktopManager` calls for all 32 HWNDs and also recorded DWM cloak and ordinary window state.

## Switch measurements

| Phase | Interval | Complete | Min | P50 | P95 | Max | Queries/s | Process CPU |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Before restart | 16 ms | 10/10 | 47 ms | 79 ms | 123.6 ms | 125 ms | 3,930.0 | 4.52% |
| Before restart | 100 ms | 10/10 | 278 ms | 341 ms | 395.8 ms | 403 ms | 638.2 | 0.72% |
| Before restart | 500 ms | 10/10 | 1,203 ms | 1,351 ms | 1,515.1 ms | 1,516 ms | 127.9 | 0.10% |
| After restart | 16 ms | 10/10 | 109 ms | 110.5 ms | 121.6 ms | 122 ms | 3,932.4 | 4.84% |
| After restart | 100 ms | 10/10 | 200 ms | 277 ms | 332.2 ms | 338 ms | 638.2 | 0.97% |
| After restart | 500 ms | 10/10 | 1,045 ms | 1,273.5 ms | 1,527.9 ms | 1,577 ms | 127.9 | 0.29% |

All 60 requested transitions completed without a timeout. A transition presented at most two intermediate signatures before the three-equal-sample rule settled it. Explorer restart did not change the result shape or lose the 32-window cohort.

Continuous 16 ms sampling is not acceptable: the idle baseline used about 5% of one process and switch runs sustained about 3,930 public calls per second. It is appropriate as a short event-triggered burst because a typical switch then costs only a handful of samples. Continuous 100 ms sampling is cheaper but unnecessary once a native wake exists. The 500 ms option is visibly late and can miss a reversal unless the user leaves each desktop active long enough.

## Native event measurement

Seven user-driven switches were made during a 30-second documented `SetWinEventHook` capture:

| Signal | Relevant events | Interpretation |
| --- | ---: | --- |
| Desktop HWND `EVENT_OBJECT_NAMECHANGE` | 7 | Exactly one primary wake per observed switch |
| Normal probe cloak/uncloak | 7 | One corroborating visibility edge per switch |
| Pinned probe cloak/uncloak | 0 | Correctly remained surfaced across desktops |
| `EVENT_SYSTEM_DESKTOPSWITCH` | 0 | Not a Task View virtual-desktop notification |

The debug event collector received 269 total foreground/name/cloak events and used 249 ms of process CPU over 30 seconds. Production must filter at the callback boundary: global cloak events form a noisy fan-out, while the desktop-HWND name event is one clean wake per switch.

## Per-window API behavior

`IsWindowOnCurrentVirtualDesktop` returned `S_OK` for every sampled HWND in every run. The normal probe and one ordinary window alternated membership and DWM cloak state. The pinned probe remained current and uncloaked on both desktops.

`GetWindowDesktopId` is supplemental rather than cohort-critical:

- The normal probe, pinned probe, and one ordinary window returned stable non-null GUIDs.
- The packaged representative returned `GUID_NULL` and remained cloaked during the sampled phase.
- All 26 minimized tool windows, the elevated representative, and one ordinary representative returned `0x8002802B` (`TYPE_E_ELEMENTNOTFOUND`) while membership calls still succeeded.
- DWM cloak queries had no errors.

Therefore an unavailable desktop GUID must never be converted into “other desktop,” and it must not invalidate an otherwise usable membership observation. Window classes that Windows does not assign to ordinary virtual desktops can be observed, but they cannot serve as desktop-identity sentinels.

## Idle baseline before Explorer restart

| Interval | Elapsed | Polls | Public queries | Query rate | Process CPU | Approx. process CPU |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 ms | 10.003 s | 615 | 39,360 | 3,934.8/s | 499 ms | 4.99% |
| 100 ms | 10.027 s | 100 | 6,400 | 638.3/s | 61 ms | 0.61% |
| 500 ms | 10.005 s | 20 | 1,280 | 127.9/s | 77 ms | 0.77%* |

The 500 ms CPU value is dominated by short-run scheduling noise. The meaningful comparison is that 16 ms issued about 6.2 times as many polls as 100 ms and consumed about 5% while idle.

## Public surface audit

The installed Windows SDKs include 10.0.22621, 10.0.26100, and 10.0.28000. The newest 10.0.28000 `ShObjIdl_core.idl` still defines only these public methods:

- `IsWindowOnCurrentVirtualDesktop`
- `GetWindowDesktopId`
- `MoveWindowToDesktop`

It exposes no public virtual-desktop enumeration, naming, ordering, current-desktop getter, notification registration, or switch method. `Windows.UI.ViewManagement.IActivationViewSwitcher::IsViewPresentedOnActivationVirtualDesktop` is an old app-activation query, not a general desktop observer. Microsoft-owned tools that enumerate or switch desktops still declare private shell interfaces whose ABI changes across Windows releases.

Primary references:

- [Microsoft Learn: `IVirtualDesktopManager`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager)
- [Microsoft Learn: WinEvent constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants)
- [Microsoft Learn: `SetWinEventHook`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
- [Microsoft PowerToys: FancyZones WinEvent registration and desktop-HWND filter](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/fancyzones/FancyZones/FancyZonesApp.cpp)

## Limits

- Explorer changing the desktop window's accessibility name is measured Windows 11 shell behavior and current Microsoft PowerToys practice, not a semantic guarantee stated by the WinEvent documentation.
- The adapter therefore also accepts managed-window cloak/uncloak wakes and validates every wake through public membership facts.
- A callback only schedules work; it never mutates manager state reentrantly.
- A 500 ms settling deadline bounds each 16 ms burst. Failure preserves the previous stable state and reports degraded observation rather than moving or hiding windows from uncertain evidence.
- This design observes switches and per-window membership. It still cannot publicly enumerate, name, order, create, or switch Windows virtual desktops.

## Reproducibility

Run `summarize-results.ps1` to regenerate `results/summary.json` and `results/summary.md` from the raw captures. Primary evidence is stored in:

- `results/idle-pre-16ms.json`
- `results/idle-pre-100ms.json`
- `results/idle-pre-500ms.json`
- `results/pre-restart-16ms.json`
- `results/pre-restart-100ms.json`
- `results/pre-restart-500ms.json`
- `results/post-restart-16ms.json`
- `results/post-restart-100ms.json`
- `results/post-restart-500ms.json`
- `results/native-events.json`
- `results/summary.json`
- `results/summary.md`
