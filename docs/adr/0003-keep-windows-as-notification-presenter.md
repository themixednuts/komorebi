# ADR 0003: Keep Windows as the notification presenter

## Status

Accepted.

## Context

The manager wants a cohesive shell experience without duplicate or lost notifications. A feasibility spike tested whether documented Windows 11 APIs support exclusive presentation with complete actions and automatic recovery.

## Decision

Windows remains the sole presenter and original action router for foreign notifications. A separately hosted manager notification role may maintain an explicitly invoked, consented private history and dismiss exact current entries. It never turns a listener observation into a popup.

The role is event-driven. It takes one initial snapshot when history opens, applies `NotificationChanged` additions/removals, and checks permission at each operation boundary. It does not poll access, notification collections, process health, or Focus state.

## Consequences

- Ordinary notifications cannot be lost or duplicated by notification-role failure because the manager acquires no suppression authority.
- Private history can refresh quickly and support exact dismissal, but it cannot reproduce arbitrary original actions.
- Permission revoked while idle is discovered on the next native wake or user request. That delay is safe because Windows presentation remains active.
- Focus state may be observed through its documented property/event, but Focus mutation is not used.
- A future exclusive route requires a new documented OS lease and a new measured decision; it is not an incremental tweak to the listener.
