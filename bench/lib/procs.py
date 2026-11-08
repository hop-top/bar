#!/usr/bin/env python3
"""Print PIDs whose environment has HOME=<arg>, minus this process and its ancestors.

Used to wait for a tool's detached refresh jobs before wiping its cache dir:
each tool runs with its own HOME, so its leftovers are found by environment.
"""
import os, sys

want = b'HOME=' + sys.argv[1].encode()
skip, p = set(), os.getpid()
while p > 1:
    skip.add(p)
    try:
        p = int(open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()[1])
    except OSError:
        break
for d in os.listdir('/proc'):
    if not d.isdigit() or int(d) in skip:
        continue
    try:
        if want in open(f'/proc/{d}/environ', 'rb').read().split(b'\0'):
            print(d)
    except OSError:
        pass
