### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.79 ± 0.03 | 0.86 | 0.74 | 0.73 | 0.72 / 0.00 | 18.4 |
| bar-notokens | 0.81 ± 0.05 | 0.86 | 0.76 | 0.75 | 0.74 / 0.00 | 18.0 |
| bar | 0.83 ± 0.42 | 0.88 | 0.75 | 0.76 | 0.75 / 0.00 | 16.7 |
| ccline | 0.94 ± 0.03 | 0.99 | 0.88 | 0.88 | 0.86 / 0.00 | 17.9 |
| ccline-subagent | 1.11 ± 0.05 | 1.21 | 1.04 | 1.05 | 1.03 / 0.00 | 19.0 |
| claude-powerline | 43.59 ± 2.32 | 49.13 | 40.98 | 50.39 | 39.76 / 11.30 | 102.2 |
| ccstatusline | 94.39 ± 9.95 | 108.88 | 90.64 | 120.53 | 109.73 / 11.46 | 115.6 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.87 ± 0.10 | 1.13 | 0.81 | 0.82 | 0.76 / 0.00 | 18.0 |
| bar-notokens | 1.00 ± 0.11 | 1.23 | 0.97 | 0.90 | 0.88 / 0.00 | 16.9 |
| ccline-subagent | 1.94 ± 0.31 | 2.50 | 1.61 | 1.85 | 0.92 / 1.04 | 18.0 |
| bar | 2.14 ± 0.63 | 2.53 | 1.88 | 1.94 | 0.98 / 0.95 | 16.5 |
| ccline | 6.44 ± 1.07 | 7.54 | 3.73 | 3.00 | 0.96 / 2.15 | 16.8 |
| claude-powerline | 58.21 ± 1.19 | 60.17 | 56.27 | 74.05 | 60.91 / 14.09 | 106.6 |
| ccstatusline | 101.02 ± 4.35 | 110.79 | 97.62 | 127.85 | 113.71 / 15.65 | 111.3 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.61 ± 0.25 | 0.67 | 0.56 | 0.57 | 0.56 / 0.00 | 16.6 |

