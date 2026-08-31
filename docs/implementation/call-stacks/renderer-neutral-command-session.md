# Renderer-neutral command session

## Scope and decision

The shell owns the authenticated command-session lifecycle shared by the bar,
configuration UI, shortcuts UI, command palette, and script adapters. A caller
submits work through a `ShellHandle` and optionally awaits an
`InvocationTicket`. It does not coordinate protocol negotiation, invocation-ID
leases, catalog cache stamps, lease renewal, reconnects, or transport
cancellation.

Three placements were considered:

1. Keep one protocol client in each executable. This leaves every renderer to
   reproduce lease and catalog lifecycle rules and makes future palette and
   plugin adapters repeat them again.
2. Put the session in `komorebi-command-transport`. This would make the
   transport crate own catalog and durable invocation semantics that belong to
   the shell consumer, rather than framed pipe I/O.
3. Put the session in `komorebi-shell`. This keeps the protocol transport as a
   concrete adapter below the shell seam and gives every renderer one shared
   domain interface. This is selected.

The transport is local and owned. Its real `CommandProtocolServer` is usable in
tests, so this slice adds no public port trait or test-only client abstraction.
The dispatcher is actor-owned because `CommandProtocolClient` deliberately
poisons a session when its request future is cancelled. Keeping the transport
future inside the actor means dropping a caller's ticket never cancels a partial
pipe exchange or makes the next command inherit poisoned state.

## Typed contract

```rust
pub enum SessionLifetime {
    OneShot,
    Persistent,
}

pub struct ShellSession { /* dispatcher, cancellation token, owned task */ }
#[derive(Clone)]
pub struct ShellHandle { /* bounded sender */ }
pub struct InvocationTicket { /* one result receiver */ }

impl ShellSession {
    pub fn start(
        role: RoleHint,
        lifetime: SessionLifetime,
    ) -> Result<Self, ShellSessionStartError>;
    pub fn handle(&self) -> ShellHandle;
    pub async fn shutdown(self) -> Result<(), ShellSessionShutdownError>;
}

impl ShellHandle {
    pub fn invoke_builtin(
        &self,
        action: BuiltInActionId,
        arguments: ActionArguments,
    ) -> Result<InvocationTicket, ShellRequestError>;
    pub fn invoke_binding(
        &self,
        binding: ActionBinding,
    ) -> Result<InvocationTicket, ShellRequestError>;
}

impl InvocationTicket {
    pub async fn outcome(self)
        -> Result<InvocationSubmissionReply, ActionInvocationError>;
}
```

`start` requires an active Tokio runtime and creates one bounded, single-owner
task. Connection is lazy, so manager downtime is reported to the affected
ticket rather than making renderer construction partial. Runtime failures remain
typed data; executable adapters decide how to present them.

## Entrypoint to effect

```text
renderer or plugin adapter: ActionBinding | BuiltInActionId + BuiltInArguments
  -> ShellHandle::invoke_* -> InvocationTicket
    [nonblocking bounded enqueue; queue-full and closed are typed]
    -> shell session actor
      [owns request future even if InvocationTicket is dropped]
      -> connect or refresh private CommandSession
        -> ActionBinding::bind(current CatalogSnapshot), for user input
          -> allocate one ID from the current lease; renew only when exhausted
            -> CommandProtocolClient::invoke(ActionInvocation)
              [async authenticated named-pipe request/reply]
              -> manager command ingress and durable admission
                -> logical transition
                  -> native Windows effect
      <- typed result sent to ticket when its receiver still exists
```

## Failure and cancellation paths

- Negotiation, lease, catalog, and invocation transport failures are translated
  once to `ActionInvocationError::Session`; no renderer sees pipe details. The
  private connection is discarded after any such failure.
- A `NotModified` response during initial connection is rejected because no
  local catalog exists yet.
- A renewed lease with another namespace is rejected; invocation identity never
  silently changes ownership.
- An unavailable or ambiguous action is rejected before transport invocation.
- Manager rejection remains an `InvocationSubmissionReply::Rejected` domain
  value. The session never retries an action implicitly.
- Dropping an `InvocationTicket` drops only result interest. The actor continues
  the already accepted command to a complete reply, so transport state stays
  reusable and the action has one unambiguous submission outcome.
- Explicit session shutdown stops accepting work, finishes the one in-flight
  exchange, rejects queued work, and then drops the private connection. It never
  cancels a request between queueing bytes and validating its reply.
- No path conversion occurs in this module. `WindowsPathInput` remains WTF-16
  inside `ActionArguments` through the wire codec.

## Ownership and migration

- `komorebi-shell/src/session.rs` owns the actor, private connection state, lease
  cursors, bounded queue, ticket, shutdown, and typed errors.
- `komorebi-command-transport` continues to own authenticated pipe framing and
  protocol request/reply I/O.
- Renderer adapters own only renderer-domain projection and presentation.
- All callers migrate from `komorebi_client::command` in this wave, and
  `komorebi-client/src/command.rs` is deleted. No re-export or compatibility
  module remains.

## Stable test seam

An integration test enters through `ShellSession::start`, talks to a real local
`CommandProtocolServer`, drops the first invocation ticket, and observes both
exact invocations on the server before awaiting the second ticket. It proves
negotiation, initial lease, catalog load, action selection, invocation identity
allocation, and caller-cancellation safety through the same public interface
production callers use.
