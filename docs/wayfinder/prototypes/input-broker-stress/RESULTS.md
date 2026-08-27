# Input broker stress prototype results

Run on 2026-08-27 for [Prototype the input broker under privilege and device stress](https://github.com/themixednuts/komorebi/issues/25).

## Verdict

Keep physical keyboard and mouse hooks plus Raw Input in the medium-integrity input service. The medium hook both observed and suppressed physical F12 and G305 Back input while the elevated target had focus. The optional high-integrity broker is needed for native window operations rejected by UIPI, not for ordinary global input ownership.

Use one latest-value pointer slot and a bounded lossless key or button transition queue. Cancel both by generation across session, desktop, device, and broker boundaries. Restrict the broker to a closed command set over a named pipe accessible only to the current logon SID. Contain every broker child in a kill-on-close job.

The attached Logitech G305 verified a 1000 Hz operating point. It cannot test 4000 or 8000 Hz because Logitech specifies a 1 ms maximum report rate for this model. Do not claim either higher rate for this machine. Rerun the same test if capable hardware is attached later.

## Measured results

| Test | Result |
| --- | --- |
| Direct medium move of ordinary target | Passed and restored |
| Direct medium move of elevated target | Rejected with `ERROR_ACCESS_DENIED` 5 |
| Broker move of elevated target | Passed and restored |
| Broker named-pipe round trip | p95 31 microseconds over 200 requests |
| Medium hook observation | Physical F12, Back, Forward, and movement observed with injected count 0 |
| Medium hook suppression against elevated target | Passed; broker remained in observe mode and elevated target counters stayed at zero |
| Broker hook suppression | Four physical transitions suppressed |
| Hook transition queue | Maximum depth 1, zero drops, zero stale deliveries |
| UAC desktop boundary | Generation advanced; stale canary rejected |
| Lock and unlock | Two session events plus desktop switches; stale canary rejected |
| G305 disconnect and reconnect | Two device events; stale canary rejected |
| Broker crash | Observer stayed responsive; generation advanced; elevated child exited through job containment |

## Pointer measurements

| Load | Duration | Hook rate | Raw rate | Raw median cadence | Hook p99 / max | Preview p95 | Drops |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Idle | 15.00 s | 923.8/s | 923.8/s | 1002 Hz | 5.4 / 192.9 microseconds | 1.09 ms | 0 |
| 12 of 24 logical processors | 15.01 s | 908.0/s | 908.3/s | 1000 Hz | 5.3 / 200.3 microseconds | 1.23 ms | 0 |
| 22 of 24 logical processors | 10.12 s | 938.0/s | 938.0/s | 1001 Hz | 2.8 / 16.0 microseconds | 1.15 ms | 0 |

`GetRawInputBuffer` drained 113 events across the completed run. The lower average rates reflect pauses or unchanged reports while the mouse was moved by hand; the median nonzero report cadence establishes the 1000 Hz operating point.

## Acceptance result

- Hook callback p99 stayed below 100 microseconds and maximum stayed below 1 millisecond.
- Physical key and button down/up counts balanced. The medium transition queue dropped nothing.
- Preview p95 stayed below 8.33 milliseconds under moderate load.
- Near saturation preserved input and remained below the latency budget.
- Session, desktop, device, and broker changes invalidated earlier generations.

The first report in `results/` is a deliberately retained failed run. It used injected pointer input and saved before the sample finished. The second report is the accepted physical run. The targeted medium-suppression retest supplements it because that mode was armed but not exercised during the accepted full run.
