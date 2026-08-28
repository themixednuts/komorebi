# Exclusive notification presentation results

## Verdict

**Fail exclusive presentation; pass consented private history.** Windows remains the only popup presenter and original action router. The manager may render an explicitly opened, accessible private-history surface and may dismiss an exact current notification after permission and generation revalidation.

## Machine and method

- Windows 11 x64, build 26200.
- Rust 2024 probe using `windows` 0.62.2 generated WinRT bindings.
- Locally signed MSIX full-trust process with package identity and `uap3:userNotificationListener` capability.
- Six independent normal-popup trials and six producer-suppressed trials.
- Native `NotificationChanged` events plus bounded `recv_timeout`; no status timer, settlement loop, or background polling.
- Each trial waited for the exact marker, copied content, called `RemoveNotification(id)`, and waited for the matching `Removed` event.

## Measurements

| Route | Samples | Added event min / median / max | Dismiss-to-Removed min / median / max |
| --- | ---: | ---: | ---: |
| Windows popup (producer default) | 6 | 9.150 / 9.587 / 11.694 ms | 19.484 / 21.551 / 112.022 ms |
| Producer sets `SuppressPopup=true` | 6 | 7.073 / 7.864 / 9.514 ms | 18.648 / 19.538 / 21.231 ms |

Every trial preserved the expected app identity and both text elements as UTF-16 code units. Every exact listener dismissal produced a matching `Removed` event. The 112 ms removal outlier did not exceed the 10-second operation deadline, but six samples are not a production latency distribution.

The latency is suitable for refreshing a history view. It cannot make the listener a pre-display interceptor: the callback contract is still “added or removed,” and the producer default still instructs Windows to show its popup.

## Contract inventory

| Required capability | Evidence | Result |
| --- | --- | --- |
| Observe current notifications | Packaged listener returned `Allowed`; all 12 controlled additions were observed | Pass |
| Copy display text and app identity | Exact controlled fields captured in every trial | Pass for history |
| Dismiss observed notification | Exact ID removal generated matching `Removed` event in every trial | Pass for history |
| Pre-display veto | No such member exists on `UserNotificationListener`; change kinds are only Added/Removed | Fail |
| Suppress arbitrary producer popup | `SuppressPopup` belongs to the producing `ToastNotification`; listener cannot mutate it | Fail |
| Invoke original action from listener copy | `UserNotification` exposes no activation/action operation | Fail |
| Ordinary Focus/DND mutation | Start/deactivate methods are Limited Access Features | Fail |
| Event-driven permission revocation | Listener exposes only `NotificationChanged`; documentation requires per-operation access checks | Fail |
| Automatic crash/hang fail-open after suppression | No supported suppression lease exists; idle hang detection would require liveness traffic | Fail |
| Zero duplicates/no lost notifications | Guaranteed by selected route because manager creates no observed-notification popup and never suppresses Windows | Pass by ownership |

## Recovery experiments

- Unpackaged execution returned `HRESULT 0x80070490` (“Element not found”), proving package identity/capability is a real boundary.
- Unsigned loose registration returned `HRESULT 0x80073CFF`, so the final harness used a temporary signed MSIX rather than enabling Developer Mode or sideloading policy.
- After the trials, the exact package was uninstalled and the exact certificate thumbprint was removed from CurrentUser and LocalMachine stores. Verification found zero remaining package or certificate entries.
- Because the selected mode acquires no popup-suppression resource, notification-role exit, crash, or hang leaves Windows presentation and action routing unchanged. This is stronger than a watchdog repair: there is nothing presentation-critical to restore.

## Defensive Rust audit

The probe passes tests and Clippy with panic, `unwrap`, `expect`, indexing, undocumented unsafe blocks, and ignored arithmetic warnings denied. Report creation uses `create_new`; operational paths never pass through display strings; WinRT text preserves UTF-16 units; deliberately ignored callback teardown errors have local safety explanations. The review checklist follows [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/).

## Sources

- [Microsoft: notification listener](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener)
- [Microsoft: UserNotificationListener API](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.management.usernotificationlistener)
- [Microsoft: UserNotification API](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.usernotification)
- [Microsoft: ToastNotification.SuppressPopup](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.suppresspopup)
- [Microsoft: FocusSessionManager](https://learn.microsoft.com/en-us/uwp/api/windows.ui.shell.focussessionmanager)
- [Microsoft: app capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations)
- [Corrode: Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/)
- [`normpath` documentation](https://docs.rs/normpath/latest/normpath/)
- [`verbatim` source showing incomplete Windows prefix handling](https://docs.rs/verbatim/latest/src/verbatim/lib.rs.html)
