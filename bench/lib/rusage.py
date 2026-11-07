#!/usr/bin/env python3
"""Per-run CPU and memory of a status-line command, foreground and detached.

Runs CMD N times with stdin from a file and stdout discarded. The script
marks itself a child subreaper (prctl PR_SET_CHILD_SUBREAPER), so processes
the command detaches (double fork, setsid) are re-parented here when their
parent exits and their rusage can be collected with wait4.

Per run:
  fg_*  the command process itself (what Claude Code waits on), including
        children it waited for before exiting. maxrss is inflated by this
        script's own RSS (kept across exec); use GNU time -v for memory.
  bg_*  every descendant still alive when the command exited (detached
        refresh jobs), collected until they all exit or --bg-timeout passes.

Usage: rusage.py --runs N --input FILE [--prepare SHELLCMD] [--bg-timeout S] [--json OUT] [--stdout F] -- CMD...
"""
import argparse, ctypes, json, os, statistics, subprocess, sys, time

PR_SET_CHILD_SUBREAPER = 36


def ms(ru):
    return (ru.ru_utime + ru.ru_stime) * 1000.0


def run_once(cmd, input_path, bg_timeout, stdout_path=None):
    stdin = os.open(input_path, os.O_RDONLY)
    devnull = os.open(stdout_path or os.devnull, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
    t0 = time.perf_counter()
    # posix_spawn (vfork-style): no Python code runs in the child before exec, so its CPU
    # time is the command's own. ru_maxrss still inherits this process's high-water mark
    # across exec; use GNU time -v for memory.
    pid = os.posix_spawnp(cmd[0], cmd, os.environ, file_actions=[
        (os.POSIX_SPAWN_DUP2, stdin, 0), (os.POSIX_SPAWN_DUP2, devnull, 1)])
    os.close(stdin); os.close(devnull)
    fg = None
    bg_cpu = 0.0
    bg_rss = 0
    bg_n = 0
    deadline = None
    while True:
        try:
            if fg is None:
                wpid, status, ru = os.wait4(-1, 0)
            else:
                wpid, status, ru = os.wait4(-1, os.WNOHANG)
                if wpid == 0:
                    if time.perf_counter() > deadline:
                        break
                    time.sleep(0.002)
                    continue
        except ChildProcessError:
            break
        if wpid == pid:
            fg = {'wall_ms': (time.perf_counter() - t0) * 1000.0,
                  'cpu_ms': ms(ru), 'user_ms': ru.ru_utime * 1000.0, 'sys_ms': ru.ru_stime * 1000.0,
                  'maxrss_kb': ru.ru_maxrss, 'exit': os.waitstatus_to_exitcode(status)}
            deadline = time.perf_counter() + bg_timeout
        else:
            bg_n += 1
            bg_cpu += ms(ru)
            bg_rss = max(bg_rss, ru.ru_maxrss)
    fg['bg_procs'] = bg_n
    fg['bg_cpu_ms'] = bg_cpu
    fg['bg_maxrss_kb'] = bg_rss
    fg['bg_done_ms'] = (time.perf_counter() - t0) * 1000.0
    return fg


def summary(vals):
    vals = sorted(vals)
    return {'mean': statistics.fmean(vals), 'median': statistics.median(vals),
            'min': vals[0], 'max': vals[-1]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--runs', type=int, default=20)
    ap.add_argument('--warmup', type=int, default=0)
    ap.add_argument('--input', required=True)
    ap.add_argument('--prepare')
    ap.add_argument('--bg-timeout', type=float, default=30.0)
    ap.add_argument('--json')
    ap.add_argument('--stdout', help='write the last run\'s output here instead of /dev/null')
    ap.add_argument('cmd', nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd and a.cmd[0] == '--' else a.cmd
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
        sys.exit('prctl(PR_SET_CHILD_SUBREAPER) failed')
    runs = []
    for i in range(a.warmup + a.runs):
        if a.prepare:
            subprocess.run(a.prepare, shell=True, check=True)
        r = run_once(cmd, a.input, a.bg_timeout, a.stdout)
        if i >= a.warmup:
            runs.append(r)
    keys = ['wall_ms', 'cpu_ms', 'user_ms', 'sys_ms', 'maxrss_kb', 'bg_procs', 'bg_cpu_ms', 'bg_maxrss_kb']
    out = {'command': cmd, 'runs': len(runs), 'prepare': a.prepare,
           'summary': {k: summary([r[k] for r in runs]) for k in keys}, 'per_run': runs}
    s = out['summary']
    print(f"{' '.join(cmd)}\n  fg cpu {s['cpu_ms']['mean']:.2f} ms mean ({s['cpu_ms']['median']:.2f} median)"
          f"  maxrss {s['maxrss_kb']['mean']/1024:.1f} MiB  | bg procs {s['bg_procs']['mean']:.2f}/run"
          f"  bg cpu {s['bg_cpu_ms']['mean']:.2f} ms mean")
    if a.json:
        with open(a.json, 'w') as f:
            json.dump(out, f, indent=1)


if __name__ == '__main__':
    main()
