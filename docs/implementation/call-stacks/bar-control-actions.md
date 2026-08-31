# Bar control actions

## Behavior and seam

The stable seam for this slice is `CommandQueue`, the bar's Rust API for
submitting user intent. A click on paused enqueues one canonical global pause
action. Selecting floating writes the exact tiling state for the bar's monitor
and workspace. Selecting monocle writes the desired monocle state for that same
exact target. Pause edges are lossless; repeated state writes coalesce by exact
workspace. The async actor owns reconnection and transport failures.

The old `SocketMessage` action route is removed from each migrated caller in the
same change. Container lock still needs an exact typed target before migration.
Configuration replacement is also still unmigrated. This slice does not wrap or
duplicate either path.

## Typed call stack

```text
egui click: pointer event
  -> KomorebiLayout::on_click_option
    -> CommandQueue::toggle_pause, set_workspace_tiling, or set_workspace_monocle
       input: validated bar monitor/workspace indices and desired state
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
                      -> validate exact monitor/workspace before changing focus
                      -> apply monocle only when current state differs
```

The queue rejects a closed or poisoned mailbox. Protocol rejection and transport
failure are logged by the actor; a transport failure drops the session so the
next command reconnects. No caller retries a toggle because replay could invert
the user's intended state.

## Proof

`komorebi-bar` tests enter through the `CommandQueue` methods and inspect the
adapter output before the async transport boundary. They prove canonical action
identity, exact indices and state, empty pause arguments, coalescing, and
lossless pause edges. Manager tests prove disabled monocle is idempotent and an
invalid exact target cannot partially change focus. Existing protocol tests
cover the real named-pipe and durable-admission boundary used by the actor.
