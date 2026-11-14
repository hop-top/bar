### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar | 0.81 ± 0.04 | 0.92 | 0.77 | 0.76 | 0.74 / 0.00 | 16.6 |
| bar-notokens | 0.82 ± 0.06 | 0.94 | 0.75 | 0.77 | 0.74 / 0.00 | 18.5 |
| bar-subagent | 0.84 ± 0.07 | 1.00 | 0.76 | 0.79 | 0.77 / 0.00 | 18.5 |
| ccline | 0.91 ± 0.03 | 0.97 | 0.88 | 0.86 | 0.85 / 0.00 | 18.1 |
| ccline-subagent | 1.11 ± 0.36 | 1.28 | 1.05 | 1.05 | 1.04 / 0.00 | 19.1 |
| claude-powerline | 42.88 ± 2.61 | 49.47 | 40.31 | 49.32 | 38.74 / 11.43 | 102.4 |
| ccstatusline | 92.85 ± 5.16 | 106.04 | 89.74 | 118.36 | 107.91 / 11.18 | 113.6 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-subagent | 0.83 ± 0.06 | 0.97 | 0.79 | 0.78 | 0.75 / 0.00 | 18.5 |
| bar-notokens | 0.99 ± 0.49 | 1.05 | 0.92 | 0.87 | 0.85 / 0.00 | 17.6 |
| ccline-subagent | 1.64 ± 0.11 | 1.86 | 1.47 | 1.57 | 0.78 / 0.88 | 18.7 |
| bar | 1.96 ± 0.31 | 2.81 | 1.85 | 1.77 | 1.31 / 0.42 | 16.3 |
| ccline | 6.50 ± 0.90 | 7.26 | 4.17 | 2.63 | 0.87 / 1.74 | 16.9 |
| claude-powerline | 57.52 ± 2.52 | 60.36 | 55.92 | 74.34 | 61.85 / 12.90 | 106.2 |
| ccstatusline | 98.33 ± 7.30 | 120.08 | 95.09 | 124.37 | 111.73 / 12.19 | 115.3 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.60 ± 0.04 | 0.67 | 0.56 | 0.56 | 0.55 / 0.00 | 16.6 |

