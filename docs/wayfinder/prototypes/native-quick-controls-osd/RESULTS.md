# Results

## Target

- Windows build: `10.0.26200.0`, x86-64
- Package identity: absent
- Default render endpoint: measured through Core Audio
- Physical-monitor description: raw UTF-16 `[71, 101, 110, 101, 114, 105, 99, 32, 80, 110, 80, 32, 77, 111, 110, 105, 116, 111, 114]` (`Generic PnP Monitor` for display only)
- Power capabilities: S3 and S4; hiberfile present

The probe retains Windows-origin text and executable paths as UTF-16 code units. Display strings are explicitly lossy views and never become lookup or effect operands.

## Volume and OSD

Five warmed isolated runs used a 1% Core Audio change and exact restoration. Each comparison key run was allowed to settle before the next direct-route run.

| Run | Direct callback | Direct top-level shows | Key callback | Key top-level shows | Key show owner | Exact restore |
| ---: | ---: | ---: | ---: | ---: | --- | --- |
| 1 | 0.3174 ms | 0 | 3.7533 ms | 1 | `C:\Windows\explorer.exe` | yes |
| 2 | 0.4848 ms | 0 | 2.5703 ms | 1 | `C:\Windows\explorer.exe` | yes |
| 3 | 0.2491 ms | 0 | 4.4025 ms | 1 | `C:\Windows\explorer.exe` | yes |
| 4 | 0.2600 ms | 0 | 4.8205 ms | 1 | `C:\Windows\explorer.exe` | yes |
| 5 | 0.3504 ms | 0 | 3.0723 ms | 1 | `C:\Windows\explorer.exe` | yes |

- Direct callback median: 0.3174 ms; observed maximum: 0.4848 ms.
- Synthetic-key callback median: 3.7533 ms; observed maximum: 4.8205 ms.
- Direct callbacks carried the probe event-context GUID in 5/5 runs.
- Key callbacks carried a foreign context in 5/5 runs.
- Direct Core Audio caused no top-level Explorer OSD show in 5/5 runs.
- The synthetic key caused exactly one top-level Explorer XAML OSD show in 5/5 runs.

The observer subscribes to `EVENT_OBJECT_SHOW` and `EVENT_OBJECT_HIDE`, accepts only `OBJID_WINDOW`, and receives out-of-context events through the owning thread's message queue. This avoids both timer polling and false attribution from Task Manager's animated accessibility descendants. Process image names are queried from the event PID and stored as native UTF-16 plus a display-only rendering; no class name or process ID is hardcoded into the verdict.

## Route matrix

| Control | Target observation | Selected route | Feedback owner |
| --- | --- | --- | --- |
| Volume | Direct Core Audio and callback available | `IAudioEndpointVolume` plus `IAudioEndpointVolumeCallback` | Manager for manager command; Windows for hardware key |
| Media | GSMTC available; one Chrome session observed | GSMTC session events and explicit transport requests | Open interactive surface; Windows for hardware key |
| Wi-Fi radio | Radio present/on; control access allowed | `Radio::StateChanged` and explicit `SetStateAsync` | Open interactive surface |
| Bluetooth radio | Radio present/on; control access allowed | `Radio::StateChanged` and explicit `SetStateAsync` | Open interactive surface |
| Network | Ethernet 2 with Internet access observed | `NetworkInformation::NetworkStatusChanged`, then one re-query | Interactive surface only |
| Wi-Fi networks | Access status allowed on this installation | Capability/access-gated adapter; otherwise Settings handoff | Interactive surface only |
| Brightness | DDC/CI brightness capability false; no internal WMI provider | Unavailable on this device | None |
| Power | S3, S4, and hibernation available | Separate confirmed command-catalog actions | Windows transition UI |

Availability on this machine is evidence for this installation, not a universal guarantee. Capability denial, policy, a hardware switch, device replacement, or a new Windows build is a normal typed unavailable state.

## Limits

- No radio, network, brightness, media playback, sleep, hibernate, lock, or shutdown mutation was performed.
- The DDC/CI result applies only to the attached monitor. Microsoft warns that arbitrary monitors can misimplement MCCS, so a future device must pass read/change/restore physical validation before that exact device route is enabled.
- The OSD conclusion covers direct endpoint-volume changes on this Windows build and target installation. Other routes earn feedback ownership independently.
- WinEvent visibility is measurement instrumentation, not a production dependency.
