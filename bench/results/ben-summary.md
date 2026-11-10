### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.78 ± 0.04 | 0.85 | 0.73 | 0.72 | 0.71 / 0.00 | 18.9 |
| bar-notokens | 0.80 ± 0.05 | 0.91 | 0.76 | 0.75 | 0.73 / 0.00 | 18.2 |
| bar | 0.83 ± 0.38 | 0.98 | 0.78 | 0.77 | 0.75 / 0.00 | 16.7 |
| ccline | 0.92 ± 0.03 | 0.98 | 0.86 | 0.87 | 0.85 / 0.00 | 18.2 |
| ccline-subagent | 1.09 ± 0.05 | 1.18 | 1.04 | 1.03 | 1.02 / 0.00 | 19.3 |
| claude-powerline | 41.53 ± 5.25 | 48.88 | 39.69 | 48.18 | 38.39 / 10.59 | 102.4 |
| ccstatusline | 91.97 ± 3.22 | 99.04 | 89.31 | 117.47 | 108.34 / 9.68 | 115.9 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.81 ± 0.04 | 0.89 | 0.76 | 0.76 | 0.75 / 0.00 | 18.1 |
| bar-notokens | 1.03 ± 0.10 | 1.21 | 0.94 | 0.91 | 0.88 / 0.00 | 17.0 |
| ccline-subagent | 1.67 ± 0.18 | 2.12 | 1.53 | 1.60 | 0.78 / 0.85 | 18.2 |
| bar | 1.88 ± 0.07 | 2.05 | 1.81 | 1.71 | 0.88 / 0.84 | 16.5 |
| ccline | 6.55 ± 0.78 | 7.13 | 4.32 | 2.89 | 0.96 / 1.71 | 16.7 |
| claude-powerline | 57.48 ± 1.40 | 59.74 | 55.62 | 73.83 | 61.02 / 12.85 | 106.7 |
| ccstatusline | 96.99 ± 1.84 | 99.70 | 94.39 | 123.00 | 110.98 / 12.69 | 116.2 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.63 ± 0.10 | 0.77 | 0.57 | 0.58 | 0.57 / 0.00 | 17.0 |

