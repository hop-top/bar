#!/usr/bin/env python3
"""Tool versions and container environment for bench/results/ben-meta.json.

Prints JSON. Records the container only: architecture, kernel, CPUs, memory,
CPU quota, libc. No host name, user or path.
"""
import json, os, platform, re, subprocess


def out(*cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    except OSError:
        return None
    return (r.stdout or r.stderr).strip().splitlines()[0] if (r.stdout or r.stderr).strip() else None


def ver(*cmd):
    s = out(*cmd) or ''
    m = re.search(r'\d+\.\d+(\.\d+)?', s)
    return m.group(0) if m else s


def npm_global(name):
    try:
        d = json.loads(subprocess.run(['npm', 'ls', '-g', '--json', '--depth=0'], capture_output=True, text=True).stdout)
        return d['dependencies'][name]['version']
    except (OSError, ValueError, KeyError):
        return None


def mem_gib():
    for line in open('/proc/meminfo'):
        if line.startswith('MemTotal:'):
            return round(int(line.split()[1]) / 1048576)
    return None


def cpu_quota():
    try:
        q, p = open('/sys/fs/cgroup/cpu.max').read().split()
        return 'none' if q == 'max' else f'{int(q) / int(p):g} CPUs'
    except (OSError, ValueError):
        return 'unknown'


def debian():
    try:
        return open('/etc/debian_version').read().strip()
    except OSError:
        return None


print(json.dumps({  # ben's own version is in its run JSON (metadata.ben_version)
    'container': {
        'os': f'Debian {debian()}',
        'arch': platform.machine(),
        'kernel': re.match(r'[\d.]+', platform.release()).group(0).rstrip('.'),  # version only, no build string
        'cpus': os.cpu_count(),
        'cpu_quota': cpu_quota(),
        'mem_gib': mem_gib(),
        'libc': ' '.join(platform.libc_ver()),
        'base_image': os.environ.get('BENCH_BASE_IMAGE'),
        'debian_snapshot': os.environ.get('BENCH_DEBIAN_SNAPSHOT'),
    },
    'versions': {
        'node': ver('node', '--version'),
        'npm': ver('npm', '--version'),
        'bun': ver('bun', '--version'),
        'rust': ver('rustc', '--version'),
        'git': ver('git', '--version'),
        'python': ver('python3', '--version'),
        'hyperfine': ver('hyperfine', '--version'),
        'gnu_time': ver('/usr/bin/time', '--version'),
        'ccstatusline': npm_global('ccstatusline'),
        'claude-powerline': npm_global('@owloops/claude-powerline'),
        'ccline': os.environ.get('BENCH_CCLINE'),
        'ben_commit': os.environ.get('BENCH_BEN_COMMIT'),
    },
}, indent=1))
