| tool | warm mean | warm median | warm p95 | warm min | cold median | cold p95 | CPU fg (warm) | max RSS fg | detached jobs (warm) | CPU fg (cold) | detached CPU (cold) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| bar | 0.3 | 0.3 | 0.4 | 0.3 | 2.5 | 3.2 | 0.36 | 3.5 | 0.00 procs, 0.0 ms | 2.4 | 8.7 (2.0 procs) |
| ccstatusline | 94.3 | 92.0 | 110.0 | 89.5 | 96.9 | 100.4 | 118.87 | 115.9 | 0.00 procs, 0.0 ms | 124.3 | 0.0 (0.0 procs) |
| claude-powerline | 41.6 | 41.3 | 44.2 | 39.4 | 58.7 | 66.1 | 47.93 | 102.5 | 0.00 procs, 0.0 ms | 74.9 | 0.0 (0.0 procs) |
| ccline | 0.4 | 0.4 | 0.5 | 0.4 | 6.3 | 7.1 | 0.50 | 5.3 | 0.00 procs, 0.0 ms | 2.7 | 0.0 (0.0 procs) |

| tool (stale: previous render 11 s ago) | wall median | CPU fg | detached procs/run | detached CPU/run |
|---|---|---|---|---|
| bar | 4.2 | 3.94 | 1.0 | 7.83 |
| ccstatusline | 143.5 | 166.59 | 0.0 | 0.00 |
| claude-powerline | 84.8 | 85.02 | 0.0 | 0.00 |
| ccline | 5.8 | 4.22 | 1.0 | 6.93 |

Times in ms, RSS in MiB. CPU = user + sys of the foreground process, mean of wait4 rusage over warm runs. Max RSS = GNU time -v, mean. Detached = processes still running when the foreground exited.

| invocation | mean | median | p95 | min |
|---|---|---|---|---|
| ccstatusline (bunx) | 223.8 | 213.1 | 272.5 | 191.5 |
| ccstatusline (npx) | 372.2 | 367.8 | 409.2 | 353.0 |
| claude-powerline (bunx) | 24.0 | 24.0 | 24.8 | 22.9 |
| claude-powerline (npx) | 332.0 | 326.9 | 359.0 | 315.6 |
| bun -e 0 (runtime floor) | 0.9 | 0.9 | 1.1 | 0.8 |
| node -e 0 (runtime floor) | 10.2 | 10.1 | 11.6 | 9.0 |
