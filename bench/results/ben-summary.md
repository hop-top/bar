### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar | 0.76 ± 0.32 | 1.28 | 0.54 | 0.66 | 0.55 / 0.00 | 14.7 |
| bar-notokens | 1.98 ± 1.82 | 4.57 | 0.74 | 1.41 | 0.00 / 1.02 | 18.3 |
| ccline | 2.59 ± 2.15 | 7.13 | 0.99 | 1.86 | 0.89 / 1.29 | 17.6 |
| claude-powerline | 64.93 ± 16.22 | 107.62 | 52.00 | 69.83 | 54.08 / 16.27 | 80.7 |
| ccstatusline | 142.12 ± 11.48 | 166.57 | 126.23 | 167.19 | 153.45 / 14.20 | 101.1 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-notokens | 1.96 ± 0.73 | 3.09 | 1.36 | 1.58 | 0.00 / 1.50 | 16.5 |
| ccline | 8.61 ± 1.27 | 10.83 | 6.28 | 5.91 | 0.70 / 5.02 | 16.4 |
| bar | 13.38 ± 7.02 | 28.08 | 5.81 | 9.20 | 2.20 / 6.84 | 14.3 |
| claude-powerline | 90.22 ± 14.72 | 122.02 | 80.27 | 104.04 | 86.60 / 18.28 | 83.9 |
| ccstatusline | 225.72 ± 177.50 | 626.28 | 174.90 | 269.45 | 216.41 / 47.93 | 101.2 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.38 ± 0.30 | 0.92 | 0.24 | 0.29 | 0.26 / 0.00 | 14.5 |

