### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar | 0.80 ± 0.11 | 0.95 | 0.72 | 0.73 | 0.71 / 0.00 | 16.7 |
| ccline | 0.93 ± 0.05 | 1.01 | 0.87 | 0.87 | 0.86 / 0.00 | 18.3 |
| claude-powerline | 42.15 ± 1.40 | 44.92 | 40.40 | 48.90 | 38.47 / 10.33 | 102.1 |
| ccstatusline | 92.12 ± 11.46 | 100.40 | 89.32 | 117.61 | 108.29 / 9.67 | 115.6 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar | 1.62 ± 0.12 | 1.94 | 1.57 | 1.46 | 1.45 / 0.00 | 16.7 |
| ccline | 6.61 ± 0.49 | 7.14 | 5.35 | 2.61 | 1.03 / 1.61 | 17.0 |
| claude-powerline | 58.89 ± 9.69 | 88.50 | 55.51 | 76.47 | 63.72 / 13.38 | 106.7 |
| ccstatusline | 98.13 ± 4.91 | 110.69 | 95.56 | 124.10 | 110.93 / 14.11 | 116.1 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.67 ± 0.09 | 0.84 | 0.59 | 0.61 | 0.60 / 0.00 | 16.5 |

