#!/usr/bin/env python3
"""Show the largest files under a directory, skipping .git, node_modules and target."""
import os
import sys

root = sys.argv[1] if len(sys.argv) > 1 else "."
top = int(sys.argv[2]) if len(sys.argv) > 2 else 10
SKIP = {".git", "node_modules", "target"}

files = []
for d, dirs, names in os.walk(root):
    dirs[:] = [x for x in dirs if x not in SKIP]  # prune in place
    for n in names:
        p = os.path.join(d, n)
        try:
            files.append((os.path.getsize(p), p))
        except OSError:
            pass
files.sort(reverse=True)


def human(n):
    for unit in ["B", "KiB", "MiB", "GiB"]:
        if n < 1024:
            return f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} TiB"


for size, p in files[:top]:
    print(f"{human(size):>10}  {os.path.relpath(p, root)}")
print(f"{len(files)} files, {human(sum(s for s, _ in files))} total")
