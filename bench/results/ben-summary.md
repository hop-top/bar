### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.79 ± 0.04 | 0.87 | 0.73 | 0.73 | 0.72 / 0.00 | 18.9 |
| bar-notokens | 0.81 ± 0.03 | 0.86 | 0.75 | 0.74 | 0.73 / 0.00 | 18.7 |
| bar | 0.82 ± 0.03 | 0.88 | 0.77 | 0.76 | 0.75 / 0.00 | 16.6 |
| ccline | 0.93 ± 0.04 | 1.01 | 0.87 | 0.86 | 0.85 / 0.00 | 18.6 |
| ccline-subagent | 1.12 ± 0.07 | 1.29 | 1.03 | 1.05 | 1.02 / 0.00 | 19.2 |
| claude-powerline | 41.86 ± 1.06 | 44.04 | 39.90 | 48.50 | 38.63 / 10.04 | 102.3 |
| ccstatusline | 91.69 ± 4.45 | 99.79 | 88.80 | 116.59 | 107.45 / 10.38 | 116.0 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.82 ± 0.05 | 0.91 | 0.76 | 0.76 | 0.74 / 0.00 | 18.1 |
| bar-notokens | 1.01 ± 0.10 | 1.14 | 0.94 | 0.90 | 0.88 / 0.00 | 17.4 |
| ccline-subagent | 1.66 ± 0.10 | 1.84 | 1.54 | 1.59 | 0.79 / 0.84 | 18.2 |
| bar | 1.90 ± 0.05 | 2.02 | 1.83 | 1.73 | 0.90 / 0.84 | 16.2 |
| ccline | 6.61 ± 0.90 | 7.15 | 3.95 | 2.64 | 0.88 / 1.73 | 17.0 |
| claude-powerline | 56.88 ± 1.93 | 61.09 | 55.42 | 71.96 | 59.59 / 13.29 | 105.8 |
| ccstatusline | 97.02 ± 6.20 | 114.52 | 94.82 | 123.57 | 111.06 / 13.11 | 116.3 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.61 ± 0.03 | 0.66 | 0.55 | 0.56 | 0.55 / 0.00 | 16.6 |

