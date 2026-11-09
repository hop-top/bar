### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.78 ± 0.03 | 0.84 | 0.73 | 0.72 | 0.71 / 0.00 | 18.6 |
| bar-notokens | 0.78 ± 0.03 | 0.84 | 0.74 | 0.72 | 0.72 / 0.00 | 18.4 |
| bar | 0.80 ± 0.03 | 0.86 | 0.75 | 0.75 | 0.74 / 0.00 | 17.0 |
| ccline | 0.95 ± 0.09 | 1.15 | 0.89 | 0.89 | 0.88 / 0.00 | 18.3 |
| ccline-subagent | 1.13 ± 0.06 | 1.26 | 1.06 | 1.06 | 1.04 / 0.00 | 19.1 |
| claude-powerline | 42.27 ± 1.59 | 46.32 | 40.83 | 48.96 | 38.26 / 11.21 | 102.4 |
| ccstatusline | 92.67 ± 5.41 | 102.70 | 89.66 | 118.11 | 108.31 / 10.35 | 116.0 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.83 ± 0.04 | 0.88 | 0.78 | 0.76 | 0.75 / 0.00 | 17.3 |
| bar-notokens | 1.01 ± 0.07 | 1.16 | 0.93 | 0.88 | 0.85 / 0.00 | 16.7 |
| ccline-subagent | 1.76 ± 0.11 | 1.94 | 1.55 | 1.69 | 0.86 / 0.87 | 18.2 |
| bar | 1.91 ± 0.22 | 2.20 | 1.80 | 1.72 | 1.68 / 0.00 | 16.1 |
| ccline | 6.76 ± 0.72 | 7.65 | 3.98 | 2.88 | 1.63 / 1.60 | 16.7 |
| claude-powerline | 57.70 ± 1.33 | 60.00 | 55.33 | 72.01 | 59.85 / 13.17 | 106.6 |
| ccstatusline | 98.88 ± 10.87 | 110.83 | 96.16 | 125.11 | 112.05 / 13.71 | 116.0 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.61 ± 0.04 | 0.67 | 0.56 | 0.56 | 0.55 / 0.00 | 16.5 |

