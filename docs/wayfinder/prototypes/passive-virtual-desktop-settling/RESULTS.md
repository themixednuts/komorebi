# Measurement ledger

Machine: Windows 11, 5120×1440 desktop, live komorebi session. Probe cohort: 28 controlled HWNDs, one packaged representative, one elevated representative, and two ordinary representatives. Each poll performs the two documented `IVirtualDesktopManager` queries for all 32 HWNDs and also records DWM cloak and window state.

## Idle baseline before Explorer restart

| Interval | Elapsed | Polls | Public queries | Query rate | Process CPU | Approx. process CPU |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 ms | 10.003 s | 615 | 39,360 | 3,934.8/s | 499 ms | 4.99% |
| 100 ms | 10.027 s | 100 | 6,400 | 638.3/s | 61 ms | 0.61% |
| 500 ms | 10.005 s | 20 | 1,280 | 127.9/s | 77 ms | 0.77% |

The 500 ms CPU value is dominated by short-run scheduling noise and is not evidence that it costs more than 100 ms. The useful result is that 16 ms produces about 6.2 times as many polls as 100 ms and consumed about 5% of the probe process, while 100 ms remained below 1%.

Raw evidence:

- `results/idle-pre-16ms.json`
- `results/idle-pre-100ms.json`
- `results/idle-pre-500ms.json`

## Pending switch matrix

For each interval, collect repeated Task View switches in both directions before and after Explorer restart. Record first-change latency, three-equal-sample settlement, intermediate signature count, HRESULT distribution, foreground state, and the settled behavior of normal, pinned, minimized, cloaked, packaged, and elevated windows.

No debounce decision is claimed until those traces exist.
