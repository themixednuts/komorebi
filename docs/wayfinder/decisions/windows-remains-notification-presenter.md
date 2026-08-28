# Windows remains the notification presenter

## Decision

Keep Windows as the sole presenter and original action router for ordinary, security, system, urgent, and unknown notifications. The manager may offer an explicitly invoked, consent-gated, accessible private notification history. It does not create a popup from a `UserNotificationListener` observation.

This decision applies to the personal Windows 11 profile. Manager-authored status surfaces and notifications from a producer that is itself part of the manager remain separate first-party behavior; they do not grant authority over foreign notifications.

## Why

The native probe measured fast and complete enough listener delivery for private history: normal additions arrived in 9.150–11.694 ms across five trials, producer-suppressed additions in 7.073–9.514 ms, and exact dismissals generated matching removal events. That evidence does not establish presentation authority.

The documented Windows call stack has four non-negotiable gaps:

1. `NotificationChanged` reports that a notification was added or removed; it is not a pre-display veto.
2. `SuppressPopup` is a policy on the producing app's own `ToastNotification`; a listener cannot set it for a foreign notification.
3. `UserNotification` exposes content and identity but no operation to invoke the producer's original action.
4. Focus start/deactivation are Limited Access Features, while listener permission has no revocation event. There is no ordinary event-driven suppression lease that automatically fails open after an idle hang, crash, or revoked permission.

Any one gap fails exclusive presentation. Together they make a copied manager popup both duplicative under normal Windows policy and behaviorally incomplete under suppression.

## Behavior contract

- Windows owns popup visibility, priority class, Do Not Disturb behavior, original actions, and fallback.
- The notification role requests listener consent only from its accessible first-run/history UI.
- History is memory-only by default. Access is checked at startup, explicit open, each native change before content access, and each action. No periodic access poll exists.
- An empty listener collection after access loss is unavailable evidence, not “zero notifications.”
- Permission loss clears manager-held content when discovered and publishes one accessible unavailable state.
- A private-history item carries a generation and exact notification ID. Dismiss revalidates both, checks access, and calls `RemoveNotification`; a matching native removal settles the action.
- Original notification actions remain on the Windows popup/Notification Center. The manager does not synthesize or guess them.
- Notification-role crash or hang cannot affect popup delivery because the role owns no suppression. Process death may trigger normal role supervision; one timed-out user request may replace a hung role without a heartbeat.

## Typed call stack

```text
NotificationChanged(Added, NotificationId)
  -> WindowsNotificationAdapter::observe
    -> AccessStatus::Allowed | HistoryUnavailable
      -> UserNotificationListener::GetNotification
        -> ObservedNotification { generation, id, app, Utf16Fields }
          -> NotificationHistory::apply(Observed)
            -> NotificationHistorySnapshot
              -> GPUI accessible history projection
```

```text
DismissNotification { HistoryHandle { generation, id } }
  -> NotificationHistory::admit
    -> CurrentHandle | StaleHandle
      -> WindowsNotificationAdapter::dismiss
        -> Allowed + RemoveNotification(id) | PermissionLost | WindowsRejected
          -> NotificationChanged(Removed, id)
            -> committed DismissalOutcome
```

The WinRT adapter owns package capability, consent status, IDs, callbacks, and HRESULT translation. The history domain owns generations and retained content. The role session owns a requested-operation deadline and cancellation. The renderer owns neither notification truth nor native effects.

## Rejected alternatives

**Listener plus manager popup.** Rejected because Windows may already display the popup, and the listener copy cannot route arbitrary original actions.

**Focus/Do Not Disturb as a suppression switch.** Rejected because mutation is Limited Access, not a normal lease, and cannot provide automatic no-poll fail-open recovery.

**Private Shell COM, injection, process patching, or edition policy.** Rejected as undocumented, invasive, or unavailable in the target personal profile.

**Cooperative producer suppression as general replacement.** Useful only for a producer we control. It proves a first-party app can choose silent delivery, not that the manager can own foreign notifications.

## Reversal evidence

Reconsider only if Microsoft ships a documented Windows 11 API that gives a third-party manager a pre-display notification decision, complete action forwarding, explicit safe-class classification, an OS-owned exclusive lease, and event-driven fail-open revocation on crash, hang, and permission loss. Faster listener timing or private APIs do not reverse the decision.

Measured evidence and the reproducible harness are in [the exclusive notification presentation probe](../prototypes/exclusive-notification-presentation/RESULTS.md).
