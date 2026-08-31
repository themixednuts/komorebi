# Bar control actions

## Behavior and seam

The stable seam for this slice is `CommandQueue`, the bar's Rust API for
submitting user intent. A click on paused must enqueue one canonical global
pause action. Toggle edges are lossless and never coalesce. The async actor owns
reconnection and transport failures.

The old `SocketMessage` action route is removed from the migrated caller in the
same change. Monitor-sensitive monocle, tiling, and container-lock controls need
typed monitor-at-cursor targets before migration. Configuration replacement is
also still unmigrated. This slice does not wrap or duplicate either path.

## Typed call stack

```text
egui click: pointer event
  -> KomorebiLayout::on_click_option
    -> CommandQueue::toggle_pause
       input: no untrusted scalar data
       output: Result<(), CommandQueueError>
      -> BarCommand { key, BuiltInArguments }
         invariant: key determines the canonical BuiltInActionId
         invariant: every toggle edge remains queued
        -> bounded process-local mailbox wake
          -> command actor, owned Tokio task
            -> CommandClient::refresh_catalog
            -> CommandClient::invoke_builtin
              -> authenticated named pipe
                -> durable invocation admission
                  -> manager action transition
                    -> native window-manager effect
```

The queue rejects a closed or poisoned mailbox. Protocol rejection and transport
failure are logged by the actor; a transport failure drops the session so the
next command reconnects. No caller retries a toggle because replay could invert
the user's intended state.

## Proof

`komorebi-bar` tests enter through `CommandQueue::toggle_pause` and inspect the
adapter output before the async transport boundary. They prove the canonical
action identity, empty arguments, and lossless edge ordering. Existing protocol
tests cover the real named-pipe and durable-admission boundary used by the actor.
