# Bar control actions

## Behavior and seam

The stable seam for this slice is `CommandQueue`, the bar's Rust API for
submitting user intent. A click on paused enqueues one canonical global pause
action. Selecting floating writes the exact tiling state for the bar's monitor
and workspace. Selecting monocle writes the desired monocle state for that same
exact target. Clicking the lock control writes the desired lock state for the
active lockable container on an exact workspace. Pause edges are lossless;
repeated state writes coalesce by exact workspace. The async actor owns
reconnection and transport failures.

The old `SocketMessage` action route is removed from each migrated caller in the
same change. Configuration replacement is still unmigrated. This slice does not
wrap or duplicate that path.

## Typed call stack

```text
egui click: pointer event
  -> KomorebiLayout::on_click_option or Komorebi::render_locked_container
    -> CommandQueue::toggle_pause, set_workspace_tiling, set_workspace_monocle,
       or set_workspace_active_container_lock
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
                      -> apply the desired monocle or container-lock state
```

The queue rejects a closed or poisoned mailbox. Protocol rejection and transport
failure are logged by the actor; a transport failure drops the session so the
next command reconnects. No caller retries a toggle because replay could invert
the user's intended state.

## Proof

`komorebi-bar` tests enter through the `CommandQueue` methods and inspect the
adapter output before the async transport boundary. They prove canonical action
identity, exact indices and state, empty pause arguments, coalescing, and
lossless pause edges. Manager tests prove disabled monocle and repeated lock
writes are idempotent, monocle containers are lockable, floating windows are
not, and invalid targets cannot partially change focus. Existing protocol tests
cover the real named-pipe and durable-admission boundary used by the actor.
