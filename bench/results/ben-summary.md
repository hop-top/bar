### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-notokens | 0.75 ± 0.03 | 0.80 | 0.70 | 0.69 | 0.69 / 0.00 | 18.7 |
| bar | 0.77 ± 0.04 | 0.82 | 0.72 | 0.71 | 0.70 / 0.00 | 16.5 |
| ccline | 0.92 ± 0.26 | 1.01 | 0.87 | 0.87 | 0.85 / 0.00 | 18.4 |
| claude-powerline | 41.76 ± 0.88 | 43.32 | 39.82 | 48.31 | 37.96 / 10.35 | 102.2 |
| ccstatusline | 91.63 ± 3.49 | 100.76 | 89.37 | 117.47 | 107.84 / 9.80 | 116.0 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-notokens | 0.95 ± 0.05 | 1.04 | 0.89 | 0.84 | 0.83 / 0.00 | 17.5 |
| bar | 1.65 ± 0.27 | 2.33 | 1.53 | 1.45 | 1.44 / 0.00 | 16.6 |
| ccline | 6.54 ± 0.74 | 7.19 | 4.38 | 2.71 | 0.90 / 1.68 | 17.5 |
| claude-powerline | 59.07 ± 4.12 | 68.11 | 56.16 | 76.06 | 62.09 / 13.94 | 106.3 |
| ccstatusline | 97.39 ± 8.43 | 112.27 | 94.48 | 124.24 | 111.59 / 13.84 | 115.8 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.61 ± 0.03 | 0.66 | 0.56 | 0.56 | 0.55 / 0.00 | 16.6 |

