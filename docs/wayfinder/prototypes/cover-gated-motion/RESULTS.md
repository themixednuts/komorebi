# Measured results

## Environment and method

- Windows 11, RTX 3090, one 5120×1440 display.
- Exact current-resolution modes exposed by Windows: 60, 120, and 240 Hz. The requested 144 Hz mode was unavailable and is not claimed.
- Twenty repetitions for each refresh/count/scenario cell; 480 trials total.
- Workloads contained 20 or 50 native subjects plus available real application textures from Chromium/Electron, Paint, Notepad, and PowerShell during the captured runs.
- One source in every trial used `WDA_EXCLUDEFROMCAPTURE` and was represented only by a privacy-safe placeholder.
- Cover latency is a conservative upper bound from show through paint, one `DwmFlush`, and opaque desktop-pixel sentinel verification. There is no polling or inferred compositor timestamp.

Limits are those accepted in issue #15: cover p95 ≤ two refresh intervals; frame-interval p95 ≤ 1.5 refresh intervals; no trial has two consecutive intervals above 2 refresh intervals; retirement by configured end plus `max(2 frames, 50 ms)`.

## Normal presentation

| Refresh | Subjects | Preview mix | Cover p95 | Cover limit | Frame p95 | Frame limit | Consecutive >2× | Result |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 60 Hz | 20 | 19 live + 1 protected placeholder | 17.01 ms | 33.33 ms | 17.03 ms | 25.00 ms | 0 | Pass |
| 60 Hz | 50 | 19 live + 31 placeholders | 17.42 ms | 33.33 ms | 17.11 ms | 25.00 ms | 0 | Pass |
| 120 Hz | 20 | 19 live + 1 protected placeholder | 8.79 ms | 16.67 ms | 8.79 ms | 12.50 ms | 0 | Pass |
| 120 Hz | 50 | 19 live + 31 placeholders | 8.73 ms | 16.67 ms | 8.80 ms | 12.50 ms | 0 | Pass |
| 240 Hz | 20 | 19 live + 1 protected placeholder | 8.55 ms | 8.33 ms | 4.29 ms | 6.25 ms | 0 | Cover miss |
| 240 Hz | 50 | 19 live + 31 placeholders | 8.54 ms | 8.33 ms | 4.79 ms | 6.25 ms | 0 | Cover miss |

All 120 budgeted normal trials preserved foreground focus, performed exactly two placement batches after cover completion, reached exact final geometry, retired within deadline, and cleaned up the cover and thumbnails.

The all-live comparison isolates the capacity failure. At 240 Hz and 50 subjects, 49 live thumbnails produced an 8.44 ms frame p95 against a 6.25 ms limit and as many as three consecutive over-2× pairs in one trial. At 60 and 120 Hz the all-live normal matrix passed pacing. This proves that live-preview admission must be measured and degradable.

## Cancellation and content loss

| Scenario | Pacing and settlement | Cover observation |
| --- | --- | --- |
| Cancel at 90 ms | Every cell met frame pacing, retirement, focus, exact geometry, and cleanup budgets. | Cover p95 missed narrowly in the 120 Hz cells and both 240 Hz cells, so those runs must take the live `SettleNow` path when the deadline expires. |
| Live preview loss at 90 ms | Every affected preview was replaced by its geometry placeholder on the next frame; every cell met pacing, retirement, focus, geometry, and cleanup budgets. | Cover p95 again missed in both 120 Hz and both 240 Hz cells. The later content-loss mechanism passed, but admission cannot ignore the earlier cover miss. |

No scenario stretched its configured duration. Cleanup left no visible cover window or registered probe thumbnail, and no foreign opacity or style mutation was used.

## Decision

The cover-gated mechanism is viable, but only as a deadline-enforced, capability-admitted optimization:

1. A current `MotionCapabilityProfile` may admit the preview mix that its evidence supports.
2. A live deadline still guards every presentation. Missing the complete-cover deadline immediately applies native settlement.
3. Preview admission degrades from live content to privacy-safe placeholders, then skips motion. The production design has no hardcoded `20` threshold.
4. The measured profile can admit normal 60/120 Hz presentations on this topology. It must direct-settle at 240 Hz with this GDI probe backend because the cover upper bound missed by about 0.2 ms.
5. A future GPUI/D3D presentation backend earns a new profile only by rerunning the same gates.

## Explicit limitations

- Five desktop-pixel sentinels plus an opaque rectangular window region verify sampled coverage, not every output pixel. Full-frame Desktop Duplication or equivalent production capture remains required for a global zero-pixel claim.
- Real application windows supplied DWM texture load; disposable native windows received the two geometry batches. The spike does not claim foreign application geometry responsiveness.
- This machine exposed no 144 Hz current-resolution mode and no second monitor. HDR, mixed-DPI, fullscreen, monitor removal, competing topmost, device-loss, and GPUI-renderer cases remain direct-settle until separately measured.
- The probe verifies resource ownership and handle cleanup, but it does not attribute compositor GPU residency to this process. The accepted 500 ms CPU/working-set/GPU/thread return gate remains a production GPUI/ETW validation requirement.
- Display strings in evidence are diagnostic `from_utf16_lossy` views. The authoritative class and title values are retained as raw UTF-16 code units; no display conversion feeds a Windows operation.
