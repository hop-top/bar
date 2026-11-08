### warm (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar | 0.84 ± 0.10 | 1.04 | 0.77 | 0.78 | 0.76 / 0.00 | 16.5 |
| ccline | 0.94 ± 0.03 | 0.99 | 0.89 | 0.88 | 0.87 / 0.00 | 18.4 |
| claude-powerline | 41.88 ± 3.99 | 45.99 | 40.02 | 48.56 | 38.12 / 10.73 | 102.1 |
| ccstatusline | 91.91 ± 5.28 | 101.49 | 88.91 | 117.45 | 107.52 / 10.27 | 115.8 |

### cold (runs 30, warmup 0, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| bar | 3.60 ± 0.15 | 3.88 | 3.48 | 3.44 | 3.06 / 0.42 | 16.5 |
| ccline | 6.65 ± 0.83 | 7.17 | 4.26 | 2.75 | 0.95 / 1.72 | 17.0 |
| claude-powerline | 57.43 ± 1.85 | 60.55 | 55.43 | 71.73 | 58.58 / 14.37 | 106.2 |
| ccstatusline | 99.27 ± 8.47 | 110.01 | 95.74 | 124.46 | 111.39 / 13.96 | 116.7 |

### floor (runs 100, warmup 10, ranked on median latency_ms)

| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |
|---|---|---|---|---|---|---|
| env-true | 0.61 ± 0.05 | 0.70 | 0.57 | 0.56 | 0.55 / 0.00 | 16.6 |

