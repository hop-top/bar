### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.77 ± 0.04 | 0.85 | 0.74 | 0.72 | 0.70 / 0.00 | 18.7 |
| bar-notokens | 0.80 ± 0.04 | 0.85 | 0.74 | 0.73 | 0.72 / 0.00 | 18.2 |
| bar | 0.82 ± 0.03 | 0.87 | 0.76 | 0.75 | 0.75 / 0.00 | 16.5 |
| ccline | 0.92 ± 0.03 | 0.97 | 0.87 | 0.86 | 0.85 / 0.00 | 18.2 |
| ccline-subagent | 1.10 ± 0.06 | 1.20 | 1.03 | 1.04 | 1.03 / 0.00 | 19.2 |
| claude-powerline | 42.81 ± 4.48 | 54.95 | 40.47 | 49.38 | 39.75 / 11.42 | 102.3 |
| ccstatusline | 91.95 ± 2.32 | 96.03 | 88.68 | 117.47 | 107.50 / 10.10 | 115.9 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.82 ± 0.08 | 0.87 | 0.79 | 0.76 | 0.74 / 0.00 | 18.2 |
| bar-notokens | 0.99 ± 0.05 | 1.10 | 0.93 | 0.87 | 0.84 / 0.00 | 17.5 |
| ccline-subagent | 1.74 ± 0.21 | 2.08 | 1.52 | 1.67 | 0.82 / 0.93 | 18.2 |
| bar | 1.91 ± 0.31 | 2.32 | 1.82 | 1.72 | 1.68 / 0.00 | 16.4 |
| ccline | 6.61 ± 0.67 | 7.13 | 4.41 | 2.66 | 1.17 / 1.38 | 17.0 |
| claude-powerline | 57.10 ± 1.03 | 58.62 | 55.48 | 74.25 | 59.90 / 13.53 | 106.3 |
| ccstatusline | 97.62 ± 2.95 | 105.62 | 95.09 | 123.21 | 112.59 / 11.51 | 115.8 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.61 ± 0.06 | 0.72 | 0.56 | 0.56 | 0.55 / 0.00 | 16.5 |

