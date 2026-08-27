# Windows 11 input capability matrix

Resolved 2026-08-27 for [Wayfinder issue #3](https://github.com/themixednuts/komorebi/issues/3).

## Decision

The Windows manager can own global keyboard bindings, modal submaps, and mouse-driven tiling with documented Win32 APIs. Put `WH_KEYBOARD_LL` and `WH_MOUSE_LL` on one or more dedicated message-loop threads. Their callbacks should classify the event, update a small input state machine, enqueue a typed command or pointer sample, and return immediately. The command catalog and window manager must run off those threads.

Use `RegisterHotKey` only for a small set of simple, fixed shortcuts where Windows should arbitrate ownership. It cannot express a leader sequence, key-up behavior, or a modal submap. Use Raw Input for optional device-aware telemetry and high-rate measurements, not for binding suppression.

Precision Touchpad support splits in two:

* Windows Settings can map a three- or four-finger gesture to a keyboard shortcut. This gives the Windows manager a practical discrete gesture path now.
* The documented high-fidelity API can provide contact motion and continuous progress, but Windows considers a controller only when its process is in the foreground. A background window manager therefore cannot use it for global workspace swipes. That feature needs a hardware spike and may remain unavailable unless Microsoft expands the contract.

Do not attempt to operate on the secure desktop. Cancel all modal, drag, and gesture state on a desktop switch. Elevated application support should use an optional elevated input and window-operation broker rather than `uiAccess`; Microsoft reserves `uiAccess` for assistive technology.

## Capability matrix

| Capability | Documented path | Background or global | Can consume input | Progress and cancellation | Decision |
| --- | --- | --- | --- | --- | --- |
| Fixed global keyboard chord | `RegisterHotKey` and `WM_HOTKEY` | Global on the calling thread's desktop | Windows owns a successfully registered chord | One notification per activation; `MOD_NOREPEAT` suppresses repeat | Use sparingly for bootstrap and emergency commands |
| Arbitrary global bindings | `WH_KEYBOARD_LL` | Global on the current desktop | Yes, by returning nonzero | Key down and key up allow an explicit state machine | Build this into the input service |
| Modal submaps and leader keys | `WH_KEYBOARD_LL` plus an internal state machine | Global on the current desktop | Yes, only while a configured sequence owns the event | Cancel on Escape, timeout, config replacement, hook restart, session lock, or desktop switch | Fully feasible |
| Mouse modifier drag | `WH_MOUSE_LL` | Global on the current desktop | Yes | Button transitions and screen coordinates support begin, update, commit, and cancel | Fully feasible; render previews off-thread |
| Observe a normal title-bar drag | `SetWinEventHook` with move/size and location events | Out-of-context global event hook | No | Start, location changes, and end are observable; pointer position supplies the proposed drop target | Feasible without taking over native dragging |
| High-rate device-specific mouse data | Raw Input with `RIDEV_INPUTSINK`; drain with `GetRawInputBuffer` | Background delivery to a registered window | No | Device-relative deltas and buttons; application supplies interaction state | Optional diagnostics, not the command-binding path |
| Discrete three- or four-finger touchpad command | Windows Settings custom gesture mapped to a keyboard chord | System global | Windows owns the gesture | Completion only; no useful progress or rollback | Ship as documented setup integration |
| Continuous two-finger touchpad input | `RegisterTouchpadCapableWindow` or `RegisterTouchpadCapableThread`, then `WM_POINTER` and `GetPointerTouchpadInfo*` | Only for input hit-tested to an opted-in window | The opted-in target handles it | Contact frames, history, and canceled pointer flags are available | Useful inside our own control surface, bar, or overview, not globally |
| Continuous global three- to five-finger touchpad input | `TouchpadGesturesController` and `PhysicalGestureRecognizer` | Only a controller in the foreground process is considered | It replaces the shell handler only while eligible | Pressed, moved, and released events expose contacts; recognizer updates expose continuous translation | Not viable for an always-background manager under the published contract |
| Elevated application input and window operations | High-integrity broker, or run the complete manager elevated | Same interactive desktop, subject to integrity boundaries | A high-integrity input process can be tested against both ordinary and elevated targets | Same state machines as above | Broker is the preferred design; complete the elevation spike before specification |
| UAC consent, lock screen, and Ctrl+Alt+Delete | Separate Winlogon or secure desktop | Not visible to hooks on the default desktop | No | `EVENT_SYSTEM_DESKTOPSWITCH` supplies a boundary signal | Unsupported by design; cancel and resume cleanly |

## Keyboard findings

[`RegisterHotKey`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey) asks Windows to register one modifier and virtual-key combination and post `WM_HOTKEY` to a window or thread queue. Registration usually fails if another application already owns the combination. Windows-key combinations are reserved for the operating system, and F12 is reserved for a debugger. `MOD_NOREPEAT` prevents typematic repeats. These rules make the API predictable for a few fixed bindings, but it is the wrong base for a Hyprland-style modal key map.

A [`WH_KEYBOARD_LL` callback](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc) runs before Windows posts each keyboard event to its target queue. It receives key-down and key-up transitions and can stop a handled event. The callback runs in the installing process rather than being injected into every target, but its thread must pump messages. Windows silently removes a low-level hook if its callback exceeds `LowLevelHooksTimeout`; Windows 10 version 1709 and later cap that timeout at 1000 ms. Microsoft recommends a dedicated hook thread that hands work to another thread and returns.

That gives us enough information for nested submaps, press and release bindings, repeats under our control, leader sequences, and catch-all behavior. The input state should contain pressed physical keys, the active key-map path, its deadline, and whether each event must be consumed. Command execution, config parsing, logging, and IPC do not belong in the callback.

Use scan codes as the stable binding identity and keep virtual-key names as configuration conveniences. This distinguishes physical positions from the current keyboard layout. The configuration should also let a binding opt into injected events. By default, ignore events marked `LLKHF_INJECTED` or `LLKHF_LOWER_IL_INJECTED` so commands that synthesize input cannot trigger themselves; the flags come from [`KBDLLHOOKSTRUCT`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct).

## Mouse-driven tiling

[`WH_MOUSE_LL`](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc) receives move, wheel, and button events before their target queue and can consume handled input. It has the same message-loop and silent-timeout rules as the keyboard hook. While a configured modifier and button are held, the hook can own the drag, enqueue the latest screen coordinate, and prevent the target application from also acting on the gesture. A UI or render worker should coalesce movement to one update per frame. The callback should never calculate layouts or paint a drop preview.

There is also a less invasive route for normal Windows title-bar dragging. [`SetWinEventHook`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) can observe `EVENT_SYSTEM_MOVESIZESTART`, `EVENT_OBJECT_LOCATIONCHANGE`, and `EVENT_SYSTEM_MOVESIZEEND`. The current code already models these events in [`komorebi/src/winevent.rs`](../../../komorebi/src/winevent.rs). We can show a drop target while Windows performs the native move, then tile on completion. This path preserves ordinary Windows dragging and should be the default. Modifier drag can add direct move and resize behavior for users who want it.

[`Raw Input`](https://learn.microsoft.com/en-us/windows/win32/inputdev/raw-input) can deliver keyboard and mouse data to a registered background window with `RIDEV_INPUTSINK`. Microsoft recommends [`GetRawInputBuffer`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputbuffer) when high-frequency devices can produce several events between message-loop iterations. Raw Input is asynchronous and device-aware, but it cannot stop the legacy event from reaching its destination without broader `RIDEV_NOLEGACY` registration that changes input for the registering application. It is useful for latency instrumentation and device-specific settings, not global command capture.

Mouse interaction cancellation must be explicit. End on the owned button's release. Roll back on Escape, desktop switch, session lock, hook restart, target destruction, config replacement, or a lost-capture signal. The [`EVENT_SYSTEM_CAPTUREEND` and `EVENT_SYSTEM_DESKTOPSWITCH` constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants) cover two of those boundaries.

## Precision Touchpad findings

Microsoft's new [Precision Touchpad programming portal](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/precision-touchpad-portal) documents why mouse hooks and Raw Input are not equivalent to direct touchpad access. Windows discards raw contacts during ordinary pointer movement and generates mouse input. For gestures, it routes higher-fidelity pointer data to the hit-tested destination or the shell.

For two-finger pan and zoom, [`RegisterTouchpadCapableWindow` and `RegisterTouchpadCapableThread`](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/registertouchpadcapable) opt an application's own windows into touchpad `WM_POINTER` messages. This cannot turn a background process into the destination for gestures over another application's window. The APIs are still useful for our bar, overview, and control surface.

The [`GetPointerTouchpadInfo*` functions](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/getpointertouchpadinfo) return device-relative contact positions, an entire contact frame, and coalesced history. Microsoft warns that the first and second frames may have a large spatial and temporal gap while the system disambiguates the gesture. Code must not treat the first observed delta as continuous movement. [`IS_POINTER_CANCELED_WPARAM`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-is_pointer_canceled_wparam) identifies a pointer stream that ended abruptly, so our interaction can animate back to its origin rather than commit.

For three or more contacts, [`TouchpadGesturesController`](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/touchpadgesturescontroller) exposes global tap, press, release, and manipulation events. The hard limit is foreground eligibility. Windows checks controllers registered by the foreground process, ignores controllers in background processes, then routes unclaimed gestures to the shell. The controller therefore cannot supply global workspace-swipe progress to a background Windows manager.

When eligible, the controller supplies per-contact pressed, moved, and released events. [`PhysicalGestureRecognizer`](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/physicalgesturerecognizer) converts those frames into device-relative manipulation started, updated, and completed events, with configurable contact counts. It can provide smooth progress and velocity for our own foreground interface.

Windows already reserves common three- and four-finger gestures for shell actions. [Windows Support](https://support.microsoft.com/en-us/windows/hardware/input-devices/touch-gestures-for-windows) documents remapping them in Settings. A custom shortcut is the practical integration: the shell recognizes the gesture and emits a chord bound to the command catalog. This consumes the user's previous shell action and produces no progress signal. We should make that trade explicit rather than silently replacing Task View or desktop switching.

The Win32 touchpad functions require Windows 11. The `TouchpadGesturesController` and `PhysicalGestureRecognizer` runtime classes also require Universal API Contract version 19. Microsoft currently marks this documentation as related to prerelease functionality, so runtime feature detection is mandatory. Do not assume the OS version is enough. Check `TouchpadGesturesController.IsSupported`, resolve newer User32 functions dynamically where necessary, and fall back to gesture-to-shortcut setup.

## Privilege and desktop boundaries

Windows desktops contain their own windows, menus, and hooks. Microsoft's [desktop documentation](https://learn.microsoft.com/en-us/windows/win32/winstation/desktops) states that a hook on one desktop receives only messages for windows on the same desktop. Windows switches to the Winlogon desktop for Ctrl+Alt+Delete and normally for a UAC prompt. No default-desktop input design should try to capture, draw over, or manage those screens.

The input service should subscribe to `EVENT_SYSTEM_DESKTOPSWITCH`. On either transition, it must clear pressed keys, leave every submap, cancel pointer ownership, remove previews, and invalidate any cached foreground `HWND`. On return it should verify that the hook threads are alive and rebuild state only from new input. Session lock and disconnect notifications should take the same path.

User Interface Privilege Isolation blocks a normal process from controlling higher-integrity UI. Microsoft's [security guidance for assistive technology](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview) requires a signed executable in a secure installation directory before Windows grants `uiAccess`, and explicitly says non-assistive applications should not use it. `uiAccess` is not our escape hatch.

The clean option is a narrow, high-integrity broker launched through the existing personal autostart mechanism. It can host the low-level input hooks if the elevation spike proves that medium-integrity hooks miss or cannot consume input over elevated targets, and it can perform the window operations that UIPI rejects. Keep parsing, UI, scripting, and network-capable code at medium integrity. The broker should accept a closed set of typed operations over a named pipe restricted to the current user's SID. Running the whole manager elevated remains a simpler fallback for this personal-only product, but it unnecessarily enlarges the trusted process.

## Latency and reliability budget

Microsoft's [Precision Touchpad hardware requirements](https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/touchpad-module-design-for-windows-hck-requirements) call for at least 125 Hz reporting for one contact and 100 Hz for multiple contacts, with contact-down and update latency goals of 25 ms and 15 ms. The Windows manager cannot remove that hardware and system cost.

For keyboard and mouse hooks, the 1000 ms removal threshold is a failure boundary, not a performance target. Set our callback budget to less than 1 ms at p99 on the owner's machine. The callback should do no allocation on the common path and should use a bounded queue with a latest-value slot for mouse movement. Measure input-to-command dispatch separately from command completion. A reasonable acceptance target for ordinary commands is p95 under one 120 Hz frame, 8.33 ms, before window movement or animation work.

The hook service needs a health check because Windows provides no notification when it silently removes a timed-out low-level hook. A periodic injected sentinel can verify receipt, provided the callback recognizes and consumes only that tagged sentinel. Reinstall the hook on failure and cancel all active state before accepting new input.

## Conflicts and coexistence

The fork currently delegates input to whkd or AutoHotkey, as described in [`docs/design.md`](../../design.md). The owner's machine also had whkd and PowerToys running during this research. Enabling the first-party input service without a migration would create duplicate commands and competing hooks.

The rollout should:

1. Parse the existing whkd configuration into the command catalog where possible.
2. Validate every binding for duplicates within our configuration.
3. Report `RegisterHotKey` failures with the combination and Win32 error.
4. Maintain an explicit exclusion list for PowerToys and system shortcuts.
5. Stop whkd only after the first-party bindings pass a live smoke test.
6. Pass every unowned event to `CallNextHookEx` so PowerToys and accessibility software remain functional.

There is no documented API that enumerates which process owns every global hotkey or hook. Conflict diagnostics must combine registration results, configuration knowledge, and a live test mode.

## Local experiments

Tests ran on Windows 11 Home 10.0.26200 at medium integrity.

### Hotkey registration

A disposable P/Invoke probe called `RegisterHotKey` without creating repository files.

| Case | Result |
| --- | --- |
| Ctrl+Alt+F24 | Registered successfully |
| A second registration of Ctrl+Alt+F24 | Failed with Win32 error 1409, hotkey already registered |
| F12 without modifiers | Failed with error 1409 |

This confirms collision reporting and F12 reservation on this machine. The probe unregistered every successful test binding.

### Low-level keyboard delivery

A disposable .NET probe installed `WH_KEYBOARD_LL` on a dedicated thread, injected 250 F24 down/up pairs with `SendInput`, consumed them in the callback, and measured from the call immediately before injection to callback entry.

| Events | Mean | Median | p95 | Maximum |
| ---: | ---: | ---: | ---: | ---: |
| 500 | 0.025 ms | 0.017 ms | 0.055 ms | 1.105 ms |

This is a scheduling and callback lower bound. Synthetic input inside one process excludes physical-device, driver, shell, IPC, command, and window-operation latency. It does show that a dedicated hook thread can deliver far inside the proposed 1 ms callback budget when it performs no real work.

### Touchpad API availability

The User32 exports documented at ordinals 2688 through 2694 were all present. These cover touchpad-capable registration and touchpad pointer information functions. This machine has no present Precision Touchpad device, so it could not test `IsSupported`, contact delivery, foreground routing, system-gesture replacement, cancellation, or latency. Export presence alone does not establish usable hardware support.

### Privilege coverage

The probe process ran at medium integrity and no elevated interactive target was available without prompting for elevation. The elevated-target behavior remains deliberately unresolved rather than inferred from ordinary-window results.

## Required follow-up spikes

Only three unknowns still block precise behavior contracts.

1. **Elevated input and window broker.** Run paired medium- and high-integrity probes. With ordinary and elevated Notepad windows foreground in turn, record keyboard and mouse hook delivery, suppression, `SetWindowPos`, focus, and WinEvent delivery. Compare a fully elevated manager with a high-integrity broker and medium-integrity UI. The result chooses process ownership and IPC boundaries.
2. **Precision Touchpad foreground contract.** On Windows 11 hardware with a certified Precision Touchpad, call `TouchpadGesturesController.IsSupported`, capture three- and four-finger contact frames, measure report cadence and first-to-second-frame gaps, then move the probe to background during a gesture and before a new gesture. Confirm cancellation flags on Alt+Tab, desktop switch, and contact rejection. Do not design global continuous workspace swipes unless this contradicts the published foreground restriction.
3. **Mouse interaction under load.** Feed a 1000 Hz mouse through both `WH_MOUSE_LL` and Raw Input while rendering drop previews at 60, 120, and 144 Hz. Measure callback p99, dropped or coalesced samples, input-to-preview latency, and behavior alongside PowerToys. Choose hook-only or hook-plus-Raw-Input based on measurements.

## Features that can now be specified

The research resolves enough uncertainty to write implementation tickets for:

* a first-party input service with keyboard and mouse hook threads;
* scan-code bindings, modal submaps, leader keys, repeat policy, and injected-input policy;
* a native-drag drop preview driven by WinEvent notifications;
* modifier-based move and resize interactions;
* secure-desktop and session-boundary cancellation;
* binding migration and conflict diagnostics for whkd and PowerToys;
* discrete Precision Touchpad commands through Windows Settings shortcuts;
* touchpad manipulation inside first-party foreground interfaces.

Continuous global touchpad workspace gestures and the final elevated-process architecture should stay as spikes until the remaining hardware and integrity tests finish.
