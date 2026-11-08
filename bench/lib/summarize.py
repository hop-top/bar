#!/usr/bin/env python3
"""Summaries for bench/results.

  summarize.py time-v FILE     GNU `time -v` appended runs -> JSON averages
  summarize.py table DIR       markdown results table from hyperfine/rusage/time-v JSON
  summarize.py ben DIR         markdown tables from ben-*.json
"""
import glob, json, os, re, statistics, sys

TOOLS = ['bar', 'ccstatusline', 'claude-powerline', 'ccline']


def time_v(path):
    runs, cur = [], {}
    pats = {'user_s': r'User time \(seconds\): ([\d.]+)', 'sys_s': r'System time \(seconds\): ([\d.]+)',
            'maxrss_kb': r'Maximum resident set size \(kbytes\): (\d+)', 'wall': r'Elapsed \(wall clock\) time.*: ([\d:.]+)'}
    for line in open(path):
        for k, p in pats.items():
            m = re.search(p, line)
            if m:
                cur[k] = m.group(1)
        if 'Exit status' in line:
            runs.append(cur); cur = {}
    u = [float(r['user_s']) * 1000 for r in runs]
    s = [float(r['sys_s']) * 1000 for r in runs]
    rss = [int(r['maxrss_kb']) for r in runs]
    return {'runs': len(runs), 'user_ms_mean': statistics.fmean(u), 'sys_ms_mean': statistics.fmean(s),
            'cpu_ms_mean': statistics.fmean([a + b for a, b in zip(u, s)]),
            'maxrss_kb_mean': statistics.fmean(rss), 'maxrss_kb_max': max(rss),
            'note': 'GNU time prints CPU seconds with 2 decimals (10 ms steps); see rusage-*.json for µs resolution'}


def pct(vals, p):
    v = sorted(vals)
    k = (len(v) - 1) * p
    f = int(k)
    c = min(f + 1, len(v) - 1)
    return v[f] + (v[c] - v[f]) * (k - f)


def hf(path):
    """hyperfine 2.x export: results[0].measurements[] / summary{}."""
    r = json.load(open(path))['results'][0]
    t = [m['time_wall_clock']['value'] * 1000 for m in r['measurements']]
    sm = r['summary']
    return {'name': r.get('name') or r['command'], 'mean': statistics.fmean(t), 'median': statistics.median(t),
            'p95': pct(t, 0.95), 'min': min(t), 'max': max(t), 'runs': len(t),
            'cpu_ms_mean': sm['time_cpu']['mean'] * 1000,
            'rss_mib_median': sm['memory_peak_resident']['median'] / 1048576}


def table(d):
    out = ['| tool | warm mean | warm median | warm p95 | warm min | cold median | cold p95 | CPU fg (warm) | max RSS fg | detached jobs (warm) | CPU fg (cold) | detached CPU (cold) |',
           '|---|---|---|---|---|---|---|---|---|---|---|---|']
    for t in TOOLS:
        try:
            w = hf(f'{d}/hyperfine-warm-{t}.json'); c = hf(f'{d}/hyperfine-cold-{t}.json')
            rw = json.load(open(f'{d}/rusage-warm-{t}.json'))['summary']
            tv = json.load(open(f'{d}/time-v-{t}.json'))
            rc = json.load(open(f'{d}/rusage-cold-{t}.json'))['summary']
        except FileNotFoundError:
            continue
        out.append(f"| {t} | {w['mean']:.1f} | {w['median']:.1f} | {w['p95']:.1f} | {w['min']:.1f} | {c['median']:.1f} | {c['p95']:.1f} "
                   f"| {rw['cpu_ms']['mean']:.2f} | {tv['maxrss_kb_mean']/1024:.1f} | {rw['bg_procs']['mean']:.2f} procs, {rw['bg_cpu_ms']['mean']:.1f} ms "
                   f"| {rc['cpu_ms']['mean']:.1f} | {rc['bg_cpu_ms']['mean']:.1f} ({rc['bg_procs']['mean']:.1f} procs) |")
    stale = [t for t in TOOLS if os.path.exists(f'{d}/rusage-stale-{t}.json')]
    if stale:
        out += ['', '| tool (stale: previous render 11 s ago) | wall median | CPU fg | detached procs/run | detached CPU/run |', '|---|---|---|---|---|']
        for t in stale:
            r = json.load(open(f'{d}/rusage-stale-{t}.json'))['summary']
            out.append(f"| {t} | {r['wall_ms']['median']:.1f} | {r['cpu_ms']['mean']:.2f} | {r['bg_procs']['mean']:.1f} | {r['bg_cpu_ms']['mean']:.2f} |")
    out.append('')
    out.append('Times in ms, RSS in MiB. CPU = user + sys of the foreground process, mean of wait4 rusage over warm runs. Max RSS = GNU time -v, mean. Detached = processes still running when the foreground exited.')
    rows = sorted(glob.glob(f'{d}/hyperfine-runner-*.json'))
    if rows:
        out += ['', '| invocation | mean | median | p95 | min |', '|---|---|---|---|---|']
        for p in rows:
            h = hf(p)
            out.append(f"| {h['name']} | {h['mean']:.1f} | {h['median']:.1f} | {h['p95']:.1f} | {h['min']:.1f} |")
    return '\n'.join(out)


def ben(d):
    """Tables from ben run JSON (ben-floor/warm/cold.json)."""
    out = []
    for suite in ('warm', 'cold', 'floor'):
        p = f'{d}/ben-{suite}.json'
        if not os.path.exists(p):
            continue
        r = json.load(open(p))
        out += [f"### {suite} (runs {r.get('runs', 1)}, warmup {r.get('warmup', 0)}, ranked on {r.get('aggregate', 'median')} latency_ms)", '',
                '| candidate | latency median ± sd | latency p95 | latency min | cpu median | cpu user / sys median | max RSS median (MiB) |',
                '|---|---|---|---|---|---|---|']
        for c in r['candidates']:
            a = c.get('aggregates') or {}
            if c.get('error') or 'latency_ms' not in a:
                out.append(f"| {c['name']} | error: {c.get('error')} | | | | | |")
                continue
            l, cpu, u, sy, rss = (a[k] for k in ('latency_ms', 'cpu_ms', 'cpu_user_ms', 'cpu_sys_ms', 'max_rss_kb'))
            out.append(f"| {c['name']} | {l['median']:.2f} ± {l['stddev']:.2f} | {l['p95']:.2f} | {l['min']:.2f} | {cpu['median']:.2f} "
                       f"| {u['median']:.2f} / {sy['median']:.2f} | {rss['median']/1024:.1f} |")
        out.append('')
    return '\n'.join(out)


if __name__ == '__main__':
    if sys.argv[1] == 'time-v':
        print(json.dumps(time_v(sys.argv[2]), indent=1))
    elif sys.argv[1] == 'ben':
        print(ben(sys.argv[2]))
    elif sys.argv[1] == 'table':
        print(table(sys.argv[2]))
