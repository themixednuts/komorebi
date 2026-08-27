# Windows 11 input service operating limits

Resolved 2026-08-27 for [Wayfinder issue #21](https://github.com/themixednuts/komorebi/issues/21).

## Decision

Keep the manager, command catalog, configuration, scripting, and first-party UI at medium integrity. Run low-level keyboard and mouse hooks on a dedicated highest-priority message-loop thread that only classifies input, updates bounded state, publishes a typed transition or latest pointer sample, and returns. Run commands, IPC, layout work, logging, and rendering elsewhere.

Add a narrow, optional high-integrity broker for elevated-window operations. A medium-integrity probe could read Task Manager's rectangle but `SetWindowPos` and close operations failed with `ERROR_ACCESS_DENIED` (5). Do not elevate the whole manager and do not use `uiAccess`: Microsoft reserves `uiAccess` for assistive technology and requires signing plus installation in a protected location. The broker should expose a closed operation set over a named pipe restricted to the current logon SID. Microsoft specifically recommends a logon-SID DACL to exclude other users and Terminal Services sessions. ([UAC integrity model](https://learn.microsoft.com/en-au/windows/security/application-security/application-control/user-account-control/how-it-works), [UIAccess requirements](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview), [named-pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights))

Use `WH_KEYBOARD_LL` and `WH_MOUSE_LL` for bindings and suppression. Add Raw Input only for per-device telemetry and high-rate pointer sampling. Microsoft recommends `GetRawInputBuffer` when a 1000 Hz mouse can enqueue multiple events between message-loop iterations. Raw pointer updates should replace an older queued update, while keyboard and mouse-button transitions must remain ordered and lossless. ([low-level keyboard hook](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc), [low-level mouse hook](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc), [buffered Raw Input](https://learn.microsoft.com/en-us/windows/win32/inputdev/using-raw-input))

Treat every session or desktop boundary as cancellation, not suspension. On `WTS_SESSION_LOCK`, disconnect, logoff, or `EVENT_SYSTEM_DESKTOPSWITCH`, increment an input generation, clear pressed keys and modal state, cancel pointer ownership, remove previews, and reject commands queued under the old generation. Reinstall hooks after return to the Default input desktop. Never capture, automate, draw on, or otherwise operate on the Winlogon or secure desktop. ([session-change messages](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change), [desktop-switch event](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants), [Windows desktops](https://learn.microsoft.com/en-us/windows/win32/winstation/desktops))

## Measured limits

Tests ran on Windows 11 Home 10.0.26200, an AMD Ryzen 9 5900X with 12 cores and 24 logical processors, 32 GB RAM, interactive session 1, and a medium-integrity token. The installed Komorebi and AppBar scheduled tasks both use `RunLevel=Limited`; PowerToys and whkd were running. The registry had no explicit `LowLevelHooksTimeout` override.

All input observations came from physical devices. The probes did not call `SendInput`, `mouse_event`, `keybd_event`, or journal playback; they did not suppress input. Callback own-work timing ended before `CallNextHookEx`, so it measures the proposed fast-path work rather than the rest of the hook chain. Delivery timing compared the `MSLLHOOKSTRUCT.time` millisecond timestamp with `GetTickCount`, so values below 1 ms appear as zero. The [`MSLLHOOKSTRUCT` contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct) defines this timestamp and its injected-event flags.

### Physical mouse delivery under CPU load

The observation callback decoded the hook structure, recorded into preallocated arrays with an atomic index, and chained the event. The hook thread ran at `ThreadPriority.Highest`. Load workers performed continuous floating-point work at normal priority.

| Load | Duration | Physical events | Average rate | Callback p50 / p95 / p99 / max | Delivery p50 / p95 / p99 / max |
| --- | ---: | ---: | ---: | --- | --- |
| 12 of 24 logical processors | 15 s | 3,019 | 201.3/s | 0.8 / 3.7 / 10.4 / 219.2 us | 0 / 0 / 0 / 16 ms |
| 22 of 24 logical processors | 10 s | 68 | 6.8/s | 8.8 / 21.3 / 723.7 / 723.7 us | 16 / 16 / 31 / 31 ms |

The moderate-load result supports a callback own-work budget of 100 microseconds at p99 and a hard 1 ms maximum on this machine. The near-saturation result is a degradation boundary, not a representative percentile: only 68 physical events arrived, yet delivery was already delayed by one to two 60 Hz frames. The manager should protect button and key transitions, coalesce pointer motion, lower preview quality when dispatch falls behind, and cancel rather than commit an interaction whose generation or target is stale.

These results do not establish 1000 Hz capacity. The highest observed average was 201.3 events/s, and the installed HID devices' configured report rates were not independently verified. Microsoft documents that mouse movement can be coalesced and exposes `MOUSE_MOVE_NOCOALESCE` in `RAWMOUSE`; an event count is therefore not a polling-rate measurement. ([`RAWMOUSE`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawmouse)) The verified 1000/4000/8000 Hz test is assigned to [prototype #25](https://github.com/themixednuts/komorebi/issues/25).

### Elevated-window boundary without a broker

Task Manager was started from the medium-integrity research process. Windows and the computer-control accessibility bridge identified the target as higher integrity. The medium probe obtained its top-level `HWND` and rectangle, then attempted a reversible 17-pixel `SetWindowPos` move and restore:

| Operation | Result |
| --- | --- |
| `GetWindowRect` | Succeeded |
| `SetWindowPos` move | Failed, Win32 error 5 (`ERROR_ACCESS_DENIED`) |
| `SetWindowPos` restore | Failed, error 5; no move occurred |
| normal `CloseMainWindow` request | Target remained open |
| `Stop-Process` cleanup attempt | Failed, access denied |
| one normal click on the visible Close button through the medium accessibility bridge | Target remained open |

This establishes that the current limited scheduled-task process cannot rely on ordinary window-management or automation calls against a higher-integrity target. It does not prove whether a medium `WH_KEYBOARD_LL` or `WH_MOUSE_LL` hook observes or can suppress every physical event while that target is foreground. Microsoft's hook pages describe desktop scope and callback context but do not specify this integrity combination, so it must be measured rather than inferred.

The broker half was not launched because it requires an explicit UAC consent transition and a disposable broker implementation. [Prototype #25](https://github.com/themixednuts/komorebi/issues/25) contains the paired medium/high test. Until it runs, put elevated `SetWindowPos`, focus, and related operations behind the broker boundary, but do not move all hook ownership there by assumption.

### Session and desktop registration

An observation-only probe created a hidden window and called `WTSRegisterSessionNotification(NOTIFY_FOR_THIS_SESSION)`. Registration succeeded with error 0. `GetThreadDesktop` and `OpenInputDesktop` both returned `Default`. No lock, disconnect, UAC prompt, or desktop switch was triggered by the probe.

Microsoft sends lock, unlock, console/remote connect and disconnect, logon, logoff, and desktop-ready states through `WM_WTSSESSION_CHANGE` to registered windows. `EVENT_SYSTEM_DESKTOPSWITCH` independently reports an active-desktop transition. A desktop owns its own hooks, and a hook receives only messages for windows on that desktop. The input desktop switches to Winlogon for Ctrl+Alt+Delete and normally for UAC consent. ([`WTSRegisterSessionNotification`](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification), [`WM_WTSSESSION_CHANGE`](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change), [desktop isolation](https://learn.microsoft.com/en-us/windows/win32/winstation/desktops))

The cancellation behavior is therefore specified without attempting a secure-desktop experiment:

1. Receive a WTS or desktop-switch boundary on the message-loop thread.
2. Atomically increment the input generation before publishing any more events.
3. Clear pressed-key, leader, submap, drag, resize, and gesture state.
4. Tell the render worker to remove previews and animate any reversible interaction back to rest.
5. Drop queued commands and samples whose generation is older than the current value.
6. Stop accepting commands until the input desktop is `Default` and the session reports unlock or desktop ready.
7. Reinstall hooks, rebuild no state from polling, and resume only from new physical transitions.

This prevents a key released on the secure desktop from remaining logically held after return. A high-integrity broker follows the same rule; elevation does not grant access to the Winlogon desktop.

## Hook failure and queue budgets

Low-level hook callbacks execute on the installing thread, which must pump messages. If a callback exceeds `LowLevelHooksTimeout`, Windows passes the event onward and can silently remove the hook; Windows 10 version 1709 and later cap the timeout at 1000 ms. Microsoft recommends a dedicated hook thread that immediately hands work to another thread. ([keyboard callback](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc), [mouse callback](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc))

The 1000 ms operating-system cutoff is a failure boundary, not a latency target. Use these product budgets on the target machine:

| Stage | Budget |
| --- | --- |
| Hook callback own work | p99 < 100 us; max < 1 ms |
| Callback allocation and blocking | none on the common path |
| Keyboard and button transitions | bounded lossless queue; never overwrite |
| Pointer movement | one latest-value slot per active pointer/device |
| Input-to-command dispatch | p95 < 8.33 ms under moderate load |
| Input-to-preview | p95 < one 120 Hz frame under moderate load |
| Near-saturation behavior | preserve transitions, coalesce motion, degrade visuals, cancel stale interactions |

Because Windows gives no notification when a timed-out low-level hook is silently removed, thread liveness alone cannot prove delivery health. Expose callback age and queue depth as diagnostics, reinstall hooks on every session/desktop return, and provide a user-invoked physical-input health test. This research intentionally did not inject a sentinel.

Raw Input requires explicit device registration; only one window per device class can be registered inside a process, so the input service, not a reusable UI library, must own registration. `WM_INPUT` distinguishes foreground input from `RIM_INPUTSINK` background delivery. `GetRawInputBuffer` removes batches from the calling thread's queue and can be called repeatedly until `QS_RAWINPUT` clears. ([registration ownership](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices), [`WM_INPUT`](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-input), [`GetRawInputBuffer`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputbuffer))

## Minimum process and privilege design

1. **Medium manager and UI:** own state, command catalog, configuration, scripts, layout, animation, and control surfaces.
2. **Medium input service:** own hook threads, Raw Input registration, modal input state, cancellation generation, and the bounded transition/sample queues. It may begin in the manager process if issue #19 prefers simplicity, provided hook ownership remains an isolated module and thread.
3. **Optional high-integrity broker:** perform only the typed window/input operations that fail across integrity levels. Authenticate the client and restrict the pipe to the current logon SID. Exit or idle out when not needed.
4. **No secure-desktop component:** neither medium nor high processes manage Winlogon, UAC consent, lock, or Ctrl+Alt+Delete surfaces.

The broker must not parse configuration, load plugins, host Lua, render UI, or expose a general Win32-call proxy. A broker crash or disconnect increments the input generation, cancels any privileged interaction, and leaves the medium manager responsive.

## Exact remaining experiment

[Prototype #25](https://github.com/themixednuts/komorebi/issues/25) resolves the two facts that cannot be established from passive observation:

* compare physical hook observation and suppression plus window operations with no broker, a high-integrity broker, and an elevated target;
* feed verified 1000, 4000, and 8000 Hz physical mouse traffic through low-level hooks and buffered Raw Input at idle, moderate load, and near saturation;
* record hook callback time, Raw Input batches, device counts, coalescing flags, queue depth, dropped transitions, broker round-trip time, and input-to-preview latency;
* manually lock/unlock, disconnect/reconnect, and approve one ordinary UAC prompt while proving that every boundary cancels state and no pre-boundary command executes afterward;
* never synthesize input or interact with the secure desktop.

The findings here are sufficient for [input semantics issue #13](https://github.com/themixednuts/komorebi/issues/13) and [process-boundary issue #19](https://github.com/themixednuts/komorebi/issues/19) to choose the default architecture. Prototype #25 is an acceptance gate before claiming elevated-target suppression or a specific high-polling-rate ceiling.
