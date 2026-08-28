# Native quick controls and OSD routes

## Decision

Build quick controls as typed projections of externally owned Windows state. Each control has its own documented adapter, event source, admission rule, and unavailable reason. There is no generic setting value, generic control service, system-state cache, or periodic refresh.

The interactive shell role owns the focusable quick-controls surface. The non-activating OSD role owns manager feedback. The manager admits commands and records their outcomes, but it does not pretend to own volume, media, radio, network, brightness, or power truth. Effective values are platform observations.

Windows owns feedback for physical volume and media keys. External Core Audio and media callbacks update an already open surface without opening the manager OSD. A direct manager volume command uses Core Audio with an event-context GUID; after the matching callback, the OSD role may present manager feedback. Target measurements found no Windows OSD for that direct route and one Explorer OSD for every synthetic-key baseline.

This is route-local authority. Evidence for volume grants nothing to brightness, media, radio, or power. No code suppresses a Windows OSD, intercepts global input for suppression, injects into Explorer, or depends on a private shell interface.

## Primitive model

```rust
pub struct VolumeLevel(f32);              // checked 0.0..=1.0
pub struct AudioEndpointId(Wtf16String);  // Windows identity, not UI text
pub struct MediaSessionId(HStringIdentity);
pub struct RadioId(HStringIdentity);
pub struct MonitorId(NativeMonitorIdentity);

pub enum Availability<T> {
    Available(T),
    ReadOnly(T, ReadOnlyReason),
    Unavailable(UnavailableReason),
}

pub struct QuickControlsSnapshot {
    pub revision: PlatformObservationRevision,
    pub volume: Availability<VolumeObservation>,
    pub media: Availability<MediaObservation>,
    pub wifi_radio: Availability<RadioObservation>,
    pub bluetooth_radio: Availability<RadioObservation>,
    pub network: Availability<NetworkObservation>,
    pub brightness: Availability<BrightnessObservation>,
    pub power: PowerActionAvailability,
}

pub enum QuickControlIntent {
    SetVolume(SetVolumeIntent),
    InvokeMedia(InvokeMediaIntent),
    SetRadio(SetRadioIntent),
    SetBrightness(SetBrightnessIntent),
    InvokePower(InvokePowerIntent),
    OpenNetworkSettings(OpenNetworkSettingsIntent),
}

pub enum FeedbackRoute {
    Volume(ConfirmedVolumeFeedback),
}
```

`QuickControlIntent` is a closed protocol sum, not the implementation interface. Application operations immediately dispatch to concrete route modules. They do not accept an untyped control name and value.

`Availability` keeps absence distinct from stale data, denial, unsupported hardware, and read-only policy. A missing brightness route cannot become zero brightness. A failed network query cannot become offline. An accepted radio request cannot become observed state until `StateChanged` confirms it.

## Route contracts

### Volume

The audio adapter owns an `IMMDeviceEnumerator`, the current endpoint identity, `IAudioEndpointVolume`, and one registered callback. Device-change events rebuild only that endpoint binding.

```text
GPUI range action: SetVolumeIntent(VolumeLevel, endpoint observation revision)
  -> interactive session validates semantic identity and generation
    -> authenticated manager request
      -> quick_controls::set_volume validates current endpoint and policy
        -> audio_adapter::set_endpoint_volume(level, ManagerEventContext)
          -> IAudioEndpointVolume::SetMasterVolumeLevelScalar
        <- EffectOutcome::Accepted | VolumeEffectError
      <- no claim of effective volume yet

Core Audio callback: EndpointVolumeChanged(level, context, endpoint)
  -> audio_adapter maps native identity and checked level
    -> ordered manager platform observation
      -> update QuickControlsSnapshot revision
      -> if context is an admitted manager request, emit ManagerVolumeFeedback
      -> otherwise publish state without manager OSD
```

The callback context correlates origin; it does not prove success by itself. Endpoint identity and request generation must still match. An effect error is returned as an effect error. A callback timeout becomes uncertain, not success and not an automatic retry.

### Media

GSMTC supplies session snapshots, `SessionsChanged`, `CurrentSessionChanged`, and per-session playback changes. Stable session identity combines the Windows session identity with the adapter generation; an application display name is not identity.

```text
InvokeMediaIntent(session, action, observation revision)
  -> validate session still exists and supports the action
    -> exact GSMTC `Try*Async` request
      -> typed accepted/rejected outcome
        -> playback-info event confirms effective state
          -> update the open interactive surface
```

If `globalMediaControl` access is unavailable, the surface reports that exact reason. Hardware media keys continue through Windows. The manager neither synthesizes keys nor shows a second OSD in response to their external session event.

### Radios and network

Radio enumeration and observation use `Windows.Devices.Radios`. The interactive surface requests control access only after the user invokes an exact radio change. It caches the resulting access state for the role session and invalidates it on relevant capability/settings changes. `SetStateAsync` acceptance is followed by `StateChanged`; hardware and policy may override the request.

Wi-Fi and Bluetooth remain distinct `RadioId` routes. “Airplane mode” is not fabricated by toggling both. Wi-Fi scanning and connection require their own capability and consent path; denied access produces a typed explanation and explicit Settings handoff.

Connectivity uses `NetworkInformation::NetworkStatusChanged`. The callback queues one coalesced invalidation; the role re-queries the current profile once. Connection profile objects are not treated as live caches. Connectivity is informational and never converted into a generic online Boolean that other subsystems trust for correctness.

### Brightness

Internal-panel and physical-monitor brightness are different adapters. An internal-panel provider may expose an internal brightness route. An external display must report the DDC/CI capability and then pass exact-device read/change/restore validation because Microsoft documents undefined behavior for misimplemented MCCS monitors.

```text
MonitorId + adapter generation
  -> observe supported range
    -> Availability<BrightnessObservation>
      -> explicit SetBrightnessIntent
        -> set exact device
          -> re-observe exact device value
            -> confirmed outcome | uncertain device state
```

The target monitor reports no supported route, so no slider is projected. There is no WMI command fallback, guessed range, registry write, or generic monitor command.

### Power

Power status and capabilities are observations refreshed by `WM_POWERBROADCAST` and registered power-setting notifications. Power operations are separate action types: lock, sleep, hibernate, sign out, restart, and shutdown. Availability and confirmation policy are per action.

The manager never implements a “power toggle.” Destructive actions require an explicit confirmation surface and invoke one documented API once. Cancellation, API rejection, and a process that remains running are explicit outcomes; there is no retry loop. Windows owns transition feedback.

## OSD policy

The OSD role is a GUI-subsystem, non-activating, non-tab-stop process under its existing generation-fenced role lease. It receives only a concrete `FeedbackRoute`; it cannot subscribe to every platform observation and decide to appear.

A route may construct feedback only when all of these hold:

1. The originating manager request was admitted for the same exact device/session generation.
2. The route's documented effect completed without rejection.
3. A native observation correlates the effective state to that request where the API supports correlation.
4. The maintained Windows 11 compatibility suite proves that exact direct route does not also summon Windows feedback.

Only direct volume currently passes those gates, so `FeedbackRoute` has one variant. Media, radio, brightness, and power do not gain speculative variants; a future measured route expands the type and its vertical tests together. Production does not inspect Explorer windows or branch on a hardcoded Windows build number. WinEvent instrumentation remains in the prototype and compatibility test harness only, where a Windows update can fail the route before it is accepted for the target installation.

The OSD never takes focus. Visual and accessibility output derive from one `FeedbackSnapshot`. Repeated volume steps replace the pending volume presentation with the newest confirmed value; they do not queue stale cards. Reduced-motion mode changes presentation, not information. External hardware events rely on Windows accessibility feedback and do not cause a second manager announcement.

## Native text and path contract

Windows-origin identifiers and filesystem operands never pass through `String`.

- Win32 text that can contain unpaired surrogates is owned as [`wtf_string::Wtf16String`](https://docs.rs/wtf-string/latest/wtf_string/type.Wtf16String.html) or a narrower validated newtype over it.
- Standard filesystem APIs receive `Path`, `PathBuf`, `OsStr`, or `OsString`; Win32 APIs receive the original WTF-16 units. UTF-8 is for authored configuration and UI text only.
- `std::path::Prefix` classifies drive, drive-relative, UNC, verbatim disk, verbatim UNC, and device namespaces. The code does not classify a path by slash counting or a string prefix test.
- Verbatim and device paths are not normalized as ordinary DOS paths. `/` is not rewritten inside an already verbatim path. Drive-relative paths remain drive-relative unless an explicit boundary rejects or resolves them.
- A lossy rendering is allowed only in a field or method named `display`; it can never flow into open, compare, launch, deduplicate, authorization, or persistence operations.
- Repeated filesystem operations anchor on an open handle when identity or privilege matters. A path comparison is not a file-identity comparison, and a check-then-open sequence is treated as a TOCTOU defect.
- Interior NUL, length, namespace, and access failures stay typed. No `unwrap`, `expect`, discarded meaningful `Result`, or synthetic default crosses a native boundary.

The exact `wtf-string` revision must be pinned and audited before production adoption because it is new. If it fails that audit, the fallback is standard `OsString`/`PathBuf` plus narrow owned UTF-16 buffers—not `String`, `camino`, or another UTF-8-only path type.

## Ownership

| Owner | Owns | Must not own |
| --- | --- | --- |
| Manager quick-control operations | Admission, request identity, observation revisions, typed outcomes | Windows setting truth, HWNDs, COM objects, GPUI state |
| Interactive shell role | Quick-controls session, semantic selection/focus, one current snapshot | Native effects, OSD presentation, manager state |
| Audio adapter | Endpoint binding, Core Audio callback, origin context | Media, radio, UI policy |
| Media adapter | GSMTC manager/session bindings and events | Synthetic media keys, renderer state |
| Radio adapter | Radio identities, access state, `StateChanged` bindings | Network reachability, airplane-mode fiction |
| Network adapter | Connectivity event and current-profile observation | Radio mutation, global online truth |
| Monitor adapters | Exact monitor identity, capability, read/set/confirm | Generic brightness assumptions |
| Power adapter | Capabilities, notifications, exact documented actions | Confirmation UI, retry policy |
| OSD role | Non-activating presentation of admitted `FeedbackRoute` | Hardware-key interception, origin inference, Windows suppression |

Concrete adapters are assembled at the composition root. Do not add `QuickControlService`, `SystemSettings`, a dependency bag, a renderer callback in domain state, or a Boolean matrix whose combinations downstream code must reinterpret.

## Failure and recovery

- Adapter loss changes only that route to `Unavailable` and advances the observation revision.
- Role restart revokes its event registrations and generations; late callbacks cannot update a new role.
- A manager restart invalidates pending requests and OSD feedback through the manager epoch.
- Callback registration and removal errors are surfaced. Cleanup errors that cannot be acted on are deliberately documented at their ownership boundary.
- No effect is retried automatically. The next native event or explicit user action is a new input.
- Settings handoff uses a typed, fixed Windows settings destination selected by the failed route; arbitrary URI text is never accepted from configuration or an extension.

## Verification gates

- Strict Clippy and tests for checked ranges, route/action sums, stale generations, and denied/unavailable states.
- Vertical integration tests from keyboard, pointer, and UIA actions through the same intent and concrete adapter.
- Five-run compatibility measurement per supported Windows build for each manager-owned OSD route: native callback observed, zero Windows top-level OSD shows, exact state restoration.
- Hardware-key tests require real device input before claiming physical-device semantics. Synthetic input remains only a labelled comparison.
- External monitor brightness stays disabled until that exact model passes physical read/change/restore testing.
- UTF-16 tests include unpaired surrogates; path tests include drive-relative, UNC, verbatim disk, verbatim UNC, device namespace, trailing-dot/space, and long-path cases. Operational round trips compare original code units, not display text.
- Race tests cover device/session removal between admission and effect, manager/role restart during an async request, and late callbacks.
- Thermo-nuclear review rejects giant route dispatchers, parallel truth, generic control abstractions, hidden retries, polling, magic class names, and lossy path operands.
