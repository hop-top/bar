### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-notokens | 0.75 ± 0.03 | 0.81 | 0.70 | 0.69 | 0.68 / 0.00 | 18.4 |
| bar | 0.77 ± 0.04 | 0.85 | 0.71 | 0.71 | 0.70 / 0.00 | 14.6 |
| ccline | 0.92 ± 0.03 | 0.97 | 0.87 | 0.86 | 0.84 / 0.00 | 17.5 |
| claude-powerline | 42.12 ± 1.61 | 45.92 | 39.59 | 48.75 | 38.34 / 10.48 | 102.3 |
| ccstatusline | 92.38 ± 6.24 | 101.62 | 89.37 | 117.85 | 107.66 / 10.79 | 116.2 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar-notokens | 0.93 ± 0.07 | 1.05 | 0.86 | 0.83 | 0.81 / 0.00 | 16.7 |
| bar | 1.65 ± 0.32 | 2.51 | 1.57 | 1.47 | 1.46 / 0.00 | 14.4 |
| ccline | 6.95 ± 0.76 | 7.48 | 4.90 | 3.17 | 1.10 / 1.83 | 16.5 |
| claude-powerline | 57.51 ± 5.30 | 60.05 | 55.92 | 73.02 | 61.37 / 11.64 | 106.3 |
| ccstatusline | 97.93 ± 3.19 | 105.42 | 95.08 | 123.24 | 112.29 / 10.98 | 116.0 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.60 ± 0.03 | 0.65 | 0.56 | 0.55 | 0.55 / 0.00 | 14.7 |

