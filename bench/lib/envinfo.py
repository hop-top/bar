#!/usr/bin/env python3
"""Tool versions and environment for bench/results/ben-meta.json.

Prints JSON. Records the environment the run sees (the container, or the
prepared environment of a native run): OS, architecture, kernel, CPU model,
CPUs, memory, CPU quota, libc, and the tool versions found there. No host
name, user or path.
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


def os_name():
    try:
        return 'Debian ' + open('/etc/debian_version').read().strip()
    except OSError:
        pass
    try:
        for line in open('/etc/os-release'):
            if line.startswith('PRETTY_NAME='):
                return line.split('=', 1)[1].strip().strip('"')
    except OSError:
        pass
    return None


def cpu_model():
    """Model name from /proc/cpuinfo or lscpu; the vendor when no model is exposed (some VMs)."""
    try:
        for line in open('/proc/cpuinfo'):
            if line.lower().startswith('model name'):
                return line.split(':', 1)[1].strip()
    except OSError:
        pass
    try:
        info = subprocess.run(['lscpu'], capture_output=True, text=True, env={**os.environ, 'LC_ALL': 'C'}).stdout
    except OSError:
        return None
    fields = dict(l.split(':', 1) for l in info.splitlines() if ':' in l)
    model = fields.get('Model name', '').strip()
    if model and model != '-':
        return model
    vendor = fields.get('Vendor ID', '').strip()
    return f'{vendor} (model not exposed)' if vendor else None


def ben_commit(path):
    """The vcs.revision Go embeds in the binary's build info."""
    try:
        m = re.search(rb'vcs\.revision=([0-9a-f]{40})', open(path, 'rb').read())
    except OSError:
        return None
    return m.group(1).decode() if m else None


SLB = os.environ.get('SLB', '/opt/slbench')


print(json.dumps({  # ben's own version is in its run JSON (metadata.ben_version)
    'container': {
        'os': os_name(),
        'arch': platform.machine(),
        'kernel': re.match(r'[\d.]+', platform.release()).group(0).rstrip('.'),  # version only, no build string
        'cpu': cpu_model(),
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
        'ccline': ver(f'{SLB}/ccline/dl/ccline', '--version') or os.environ.get('BENCH_CCLINE'),
        'ben_commit': ben_commit(os.environ.get('BEN', f'{SLB}/bin/ben')) or os.environ.get('BENCH_BEN_COMMIT'),
    },
}, indent=1))
