### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.80 ± 0.06 | 0.96 | 0.73 | 0.74 | 0.73 / 0.00 | 18.9 |
| bar-notokens | 0.81 ± 0.04 | 0.89 | 0.75 | 0.75 | 0.74 / 0.00 | 18.7 |
| bar | 0.81 ± 0.09 | 0.92 | 0.75 | 0.76 | 0.75 / 0.00 | 16.5 |
| ccline | 0.95 ± 0.09 | 1.18 | 0.87 | 0.88 | 0.87 / 0.00 | 18.4 |
| ccline-subagent | 1.19 ± 0.11 | 1.45 | 1.06 | 1.11 | 1.08 / 0.00 | 19.5 |
| claude-powerline | 45.89 ± 2.35 | 51.41 | 42.96 | 53.47 | 40.94 / 13.24 | 101.9 |
| ccstatusline | 98.75 ± 4.94 | 108.44 | 93.19 | 125.73 | 114.18 / 11.37 | 114.8 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.91 ± 0.20 | 1.39 | 0.80 | 0.84 | 0.77 / 0.00 | 18.0 |
| bar-notokens | 1.03 ± 0.10 | 1.22 | 0.95 | 0.89 | 0.87 / 0.00 | 17.0 |
| ccline-subagent | 1.75 ± 0.19 | 2.15 | 1.54 | 1.66 | 0.86 / 0.94 | 18.6 |
| bar | 2.19 ± 0.48 | 3.55 | 1.96 | 1.88 | 1.85 / 0.00 | 16.6 |
| ccline | 6.58 ± 0.87 | 7.41 | 4.12 | 2.68 | 1.07 / 1.68 | 17.0 |
| claude-powerline | 60.20 ± 2.98 | 67.71 | 57.95 | 78.39 | 64.79 / 13.69 | 94.1 |
| ccstatusline | 103.63 ± 4.09 | 113.49 | 99.39 | 132.20 | 114.37 / 17.12 | 107.5 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.62 ± 0.06 | 0.69 | 0.57 | 0.57 | 0.56 / 0.00 | 16.6 |

