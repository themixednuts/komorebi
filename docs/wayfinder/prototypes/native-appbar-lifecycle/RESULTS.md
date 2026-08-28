# Measured results

The native Windows 11 run passed ten checks on a 5120×1440 primary monitor:

- the child PE subsystem was `IMAGE_SUBSYSTEM_WINDOWS_GUI` (`2`);
- the HWND stayed hidden until its first negotiated rectangle;
- a duplicate registration in one Shell generation was suppressed;
- 13 px and 17 px right-edge AppBars negotiated adjacent, non-overlapping rectangles and a 30 px reservation;
- graceful `ABM_REMOVE` restored the surviving 13 px reservation;
- killing a 19 px competing AppBar caused Explorer to release it and notify the survivor;
- a 21 DIP geometry update converged after Explorer's native work-area publication callback;
- Explorer restart changed process ID and creation time, then restored one 21 px reservation through `TaskbarCreated`;
- the same position call stack converted 21 DIP at synthetic 144 DPI to 32 physical pixels;
- removing the observed right-edge AppBar restored the baseline right work-area edge.

The final right-edge cleanup used a temporary left-edge sentinel AppBar as the event witness. Its `ABN_POSCHANGED` callback replaced any work-area polling; the sentinel was then removed normally.

## Limits

- The DPI transition used the real production-shaped position stack with an injected DPI value. This machine did not expose a second physical-DPI monitor for a live cross-monitor drag.
- Process death was forced and observed. Sudden machine power loss is not reproducible from a user-mode prototype.
- Explorer and DWM still own taskbar policy, arbitration, composition, and final crash recovery. The manager owns only its AppBar registration and process lifecycle.

The immutable JSON report in `native/results` contains the exact rectangles and Shell identities from the successful run.

Seven unit tests also passed, including lossless ill-formed UTF-16, UNC and verbatim-prefix preservation, and interior-NUL rejection.

## Production patch audit

The pre-existing uncommitted AppBar integration in this worktree is not implementation-ready:

- it adds lifecycle branches to `bar.rs`, which is already over 1,400 lines, instead of the dedicated shell-role process selected in issue #19;
- its `AtomicBool positioning` guard discards `ABN_POSCHANGED` received during a position call, but the live probe proved that callback can be required to observe Explorer's work-area publication;
- it unconditionally re-registers on `TaskbarCreated` without fencing the Explorer process generation;
- it creates the eframe window before AppBar negotiation, so GUI-subsystem startup removes the console but does not prove a flash-free first bar frame;
- its 100 ms connection retry loops violate the event-driven lifecycle constraint.

Those files were preserved and excluded from this prototype commit. Production implementation should replace that shape with the model/host/adapter split in `CONTRACT.md`, not extend it.
