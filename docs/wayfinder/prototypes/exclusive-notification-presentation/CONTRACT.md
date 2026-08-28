# Notification presentation feasibility contract

## Pass gate

Exclusive manager presentation could pass only if one documented route simultaneously provides:

1. a pre-display decision for arbitrary ordinary notifications;
2. complete content and original action fidelity;
3. zero Windows/manager duplicate popups;
4. an ordinary supported way to acquire and release suppression;
5. an event-driven permission-loss signal;
6. fail-open recovery after crash, idle hang, stale proof, or revoked permission;
7. a Windows-owned route for security, system, urgent, and unknown classes.

One missing contract fails the route. Low listener latency cannot compensate for missing authority or action fidelity.

## Rejected listener-presenter stack

```text
foreign producer calls Windows notification API
  -> Windows accepts notification and owns popup policy
    -> Windows popup may become visible
    -> UserNotificationListener.NotificationChanged(Added, id)
      -> listener reads UserNotification content
        -> manager popup would be a second presentation
```

The event reports `Added` or `Removed`, not a pre-display decision. `UserNotification` exposes app info, creation time, ID, and notification content; it exposes no operation for invoking the producing app's original action. Removing the Windows entry after `Added` is both too late to be a veto and destroys the notification rather than transferring presentation ownership.

## Rejected Focus stack

```text
manager attempts to suppress Windows globally
  -> FocusSessionManager.TryStartFocusSession / DeactivateFocus
    -> Limited Access Feature token required
  -> no ordinary supported suppression lease acquired
```

`IsSupported`, `IsFocusActive`, and `IsFocusActiveChanged` are valid observations. Focus mutation is not an ordinary app contract. Even user-enabled Focus would not supply the manager with a revocable OS lease: listener permission has no change event, a crashed process cannot release anything, and an idle hang cannot be distinguished without periodic liveness traffic. This route cannot fail open without polling or unsupported policy manipulation.

## Selected private-history stack

```text
foreign producer calls Windows notification API
  -> Windows remains sole popup presenter and action router
    -> listener NotificationChanged(Added, id)
      -> notification role checks current access
        -> GetNotification(id)
          -> copy permitted UTF-16 fields into an in-memory private-history observation
            -> publish revisioned history snapshot; never publish a popup intent
```

```text
explicit history open
  -> notification role checks current access
    -> Allowed: take one GetNotificationsAsync snapshot, then subscribe to native changes
    -> Denied/Unspecified/error: HistoryUnavailable; clear manager-held content
      -> GPUI renders one accessible unavailable state
```

```text
explicit dismiss on NotificationHandle { generation, id }
  -> reject stale generation
    -> check current access
      -> re-read exact notification id
        -> RemoveNotification(id)
          -> wait for matching Removed native event within the invocation deadline
            -> commit Dismissed | RemovedElsewhere | PermissionLost | WindowsRejected
```

The operation deadline bounds one requested action; it is not a background poll. A late or missing result becomes an explicit outcome and does not authorize a retry loop.

## Permission and failure behavior

- Check access at role startup, explicit history open, every listener event before content access, and every history action. Do not schedule access-status polling.
- Windows documents that permission can be revoked and listener APIs may then return an empty collection. Empty is not evidence of “no notifications”; it is `HistoryUnavailable` unless access was checked as `Allowed` for that operation.
- There is no native access-status-change event. Revocation while completely idle is discovered at the next user request or native listener wake. This is safe only because the manager owns no popup suppression.
- Notification-role crash or hang cannot lose or duplicate a popup because Windows never ceded presentation. A supervisor may observe process death through a process handle. A user request may time out a hung role and replace it. No heartbeat is required for notification safety.
- Prior content is memory-only by default and cleared when access loss is observed. Durable private history requires a separate explicit retention choice and encrypted-storage design.

## String and path boundaries

WinRT notification text is carried as UTF-16 units. A lossy `String` exists only as a display projection and cannot drive identity, matching, dismissal, or activation. Native paths remain `PathBuf`/`OsString`; Win32 calls receive encoded UTF-16 only at the adapter. UNC, verbatim UNC/disk, trailing dot/space, and unpaired-surrogate tests are part of the crate.

Path normalization is not authorization. `normpath` is the candidate for an operation that specifically needs a normalized absolute native path. Object authority and containment require an opened handle. The incomplete `verbatim` crate is rejected because its current source panics for most Windows prefix classes.
