# Popup coordination results

## Decision

Implement popup coordination as a conservative extension of the revisioned manager loop:

1. An out-of-context WinEvent callback only stamps and attempts a bounded enqueue.
2. Any hint wakes an owned observer; hints never mutate authoritative window state.
3. A full Win32 census owns window incarnation, native owner family, styles, visibility, enabled state, cloak state, frame, monitor, DPI, work area, z-neighbors, and foreground evidence.
4. A pure classifier produces the surface role, coordination mode, modal constraint, missing facts, and explanation.
5. UI Automation is supplementary and runs in a sacrificial MTA child process under a one-shot deadline.
6. Missing, stale, denied, contradictory, or timed-out evidence remains visible and observe-only. It never becomes tile eligibility.
7. Placement is one typed, generation-checked effect followed by native event acknowledgement and fresh observation. There is no retry loop.

Do not place UI Automation in the manager process. A responsive thread call is cheaper, but Windows and Rust provide no safe way to reclaim only a thread blocked in a third-party provider.

## Measured evidence

The reference run used Windows 11 build 26200 and observed 29 live top-level windows with zero observation failures. Present live classes identified Chromium/Electron, Java/Swing, WinUI 3, and Windows application-frame surfaces. Titles were never read.

The controlled producer created two independent roots plus modal dialog, modeless dialog, utility, no-activate utility, menu, tooltip, combo popup, drag visual, and hung-provider surfaces in one process. Native owner links—not process identity—produced the two families.

| Measurement | Result |
| --- | ---: |
| Injected pressure events | 20,000 |
| WinEvent callbacks delivered before the deadline | 11,826 |
| Bounded application-queue drops | 9,778 |
| Callback p50 / p95 / p99 | 0.2 / 0.6 / 0.9 µs |
| Callback maximum | 5.2 µs |
| Tail marker observed within 5 seconds | No |
| Sacrificial-process UIA p50 / p99 | 45.69 / 53.92 ms |
| Responsive provider call inside victim p50 / p99 | 18.99 / 27.25 ms |
| Hung provider, process topology | Timed out, terminated, reaped |
| Hung provider, thread-candidate topology | Timed out; only its containing process was reclaimable |

The absent tail marker is an observation gap, not proof of a particular User32 loss mechanism: it may have been dropped or still backlogged at the deadline. Either case requires the same full-census recovery. Queue saturation stayed well below the 100 µs callback p99 budget.

## Classification and modal behavior

| Fixture | Surface role | Coordination |
| --- | --- | --- |
| root, modeless root | primary | ordinary managed |
| modal dialog | modal dialog | attached float |
| modeless dialog | modeless dialog | attached float |
| utility | utility | attached float |
| no-activate utility | utility | observe-only |
| menu, tooltip, combo popup, drag visual | matching transient role | observe-only |
| hung provider dialog | unknown dialog with timed-out fact | attached float, preserve placement |

The active modal constraint blocked move-workspace, desktop-transfer, and close-root operations for its family. Focus-active-dialog and inspect remained allowed. Duplicate, reordered, delayed, or missing hints only marked census required; reconciliation converged from observed facts.

## Controlled effects

Center and restore requests preserved size, native owner/root-owner, style words, topmost state, z-neighbors, and foreground state. Both completions came from `EVENT_OBJECT_LOCATIONCHANGE`, not a timer. A stale generation was rejected before `SetWindowPos`. The focus experiment made one `SetForegroundWindow` request against the controlled no-activate surface and used neither input injection nor `AttachThreadInput`.

## Cancellation and path safety

- `#[tokio::main]` is the only runtime entry.
- Dropping an event observer signals its native stop event.
- Dropping an owned child starts termination and schedules reaping.
- UIA results carry a generation; late generations are rejected.
- Internal pipe commands have no effect until their newline framing commits.
- Report publication writes a non-authoritative partial file and renames only after `sync_all`.
- HWND identity includes process creation time and generation; raw-handle reuse cannot inherit prior state.
- Foreign class data is retained as UTF-16 units. Native arguments and paths use `OsString` and `PathBuf`; no lossy value is used as identity.

This prototype has no durable mutable state, so SQLite and Drizzle do not belong in it. Production persisted records should use Drizzle's query API and derived `FromSqliteRow`; no caller should manually decode columns.

## Unsupported or unresolved classes

- Secure-desktop UAC windows are outside the interactive desktop and cannot be observed here.
- UIPI can deny facts or effects for elevated windows; the manager must not bypass it.
- Firefox, Qt, WPF, and Windows Terminal were not present in the live reference census. Their HWNDs remain governed by the same conservative Win32 path, but provider-specific UIA compatibility needs future live fixtures.
- Browser prompts or drag visuals implemented wholly inside a compositor/widget tree have no separate HWND to coordinate.
- A third-party UIA provider has no guaranteed response bound; timeout is always unavailable evidence.

Raw evidence: [`measurements/latest.json`](measurements/latest.json).
