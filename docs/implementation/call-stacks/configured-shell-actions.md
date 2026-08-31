# Configured shell actions

## Decision

User-authored shell bindings name a stable action and its named input values. The
renderer-neutral shell core resolves those values against the manager's current
catalog before any command enters a transport queue. The resolved value carries
the exact `ActionKey` and canonical `ActionArguments`; downstream callers never
receive JSON, `SocketMessage`, shell text, or an unvalidated action identifier.

This boundary is shared by pointer bindings, keyboard shortcuts, the command
palette, and script-plugin adapters. Each adapter supplies input values, but none
duplicates action-specific parsing or manager admission rules.

## Entrypoint to effect

```text
pointer, shortcut, palette, or plugin adapter: ActionBinding
  -> komorebi_shell::ActionBinding::bind(current CatalogSnapshot)
    [stable action and parameter IDs; cardinality/domain/dynamic-choice checks]
    -> BoundAction { exact ActionKey, canonical ActionArguments }
      -> renderer-neutral shell command session
        [owned event-driven task; bounded/coalesced wakeup; no polling]
        -> CommandClient::invoke(exact ActionKey, ActionArguments)
          -> authenticated command transport
            -> manager catalog binding and action admission
              -> logical transition
                -> native Windows effect
```

## Ownership and failure rules

- `ActionBinding` owns only validated, renderer-independent input data.
- `bind` uses one immutable catalog snapshot and either returns one complete
  `BoundAction` or a typed error. Partial arguments never escape.
- The manager remains authoritative. A successfully bound action may still be
  rejected when its expected state becomes stale before admission.
- Windows paths enter the canonical argument map as native UTF-16 code units;
  no display string becomes a filesystem operand.
- Input mechanisms converge before transport. Pointer, keyboard, UI Automation,
  palette, and plugin activation cannot select different command paths.
- Arbitrary PowerShell and raw `SocketMessage` execution are not shell actions
  and are deleted when their callers migrate.

## Stable test seam

`ActionBinding::bind(&CatalogSnapshot)` is the public seam. Contract tests build
a real catalog snapshot and verify exact action schema selection and canonical
argument shapes. Adapter tests verify they submit `BoundAction`, not transport
messages.
