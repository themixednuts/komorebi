# Native quick controls and OSD routes

This prototype answers which documented Windows routes can back manager-owned quick controls on the target Windows 11 machine, and where manager feedback would duplicate Windows feedback.

The selected product behavior is:

- Direct manager volume commands use Core Audio and receive authoritative endpoint callbacks. They may show the manager OSD.
- Hardware volume and media keys remain Windows-owned. Their callbacks refresh an open control surface but never summon the manager OSD.
- Media controls use Global System Media Transport Controls when access is available.
- Wi-Fi and Bluetooth radio changes use `Windows.Devices.Radios` after an explicit user action and access grant. Network state uses `NetworkInformation`.
- Brightness appears only for a device with a proved WMI or DDC/CI route. The current external monitor exposes neither through the documented high-level route, so the control is unavailable rather than fake.
- Power is a set of distinct confirmed actions, not a toggle. This prototype observes capabilities but does not invoke disruptive actions.

No route uses a low-level keyboard hook, shell injection, an undocumented Explorer interface, a timer poll, or an OSD-suppression trick.

## Reproduce

From `native-probe`:

```powershell
cargo clippy --all-targets -- -D warnings
cargo test
cargo run --release
```

The probe performs one reversible 1% endpoint-volume change, restores the exact observed value, sends one synthetic volume key as an explicitly labelled comparison baseline, and restores again. It only reads monitor, power, network, radio, Wi-Fi-access, and media-session state.

The synthetic key proves the target Windows path used by `SendInput`; it is not evidence about a particular physical keyboard or permission to synthesize production input.

## Primary references

- [Core Audio endpoint volume and event-context callbacks](https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nf-endpointvolume-iaudioendpointvolume-setmastervolumelevelscalar)
- [Global media session manager and its capability](https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager?view=winrt-26100)
- [Radio access](https://learn.microsoft.com/en-us/uwp/api/windows.devices.radios.radio.requestaccessasync?view=winrt-26100), [state changes](https://learn.microsoft.com/en-us/uwp/api/windows.devices.radios.radio?view=winrt-26100), and [asynchronous state requests](https://learn.microsoft.com/en-us/uwp/api/windows.devices.radios.radio.setstateasync?view=winrt-26100)
- [Wi-Fi access and capability behavior](https://learn.microsoft.com/en-us/uwp/api/windows.devices.wifi.wifiadapter.requestaccessasync?view=winrt-26100)
- [Network status events](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.networkinformation?view=winrt-26100)
- [Physical-monitor brightness warning](https://learn.microsoft.com/en-us/windows/win32/api/highlevelmonitorconfigurationapi/nf-highlevelmonitorconfigurationapi-getmonitorbrightness)
- [Power setting notifications](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerpowersettingnotification)
- [Asynchronous WinEvent observation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
- [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/) for path identity, native encoding, panic, and discarded-error review checks
