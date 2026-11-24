#!/usr/bin/env python3
"""Run the test suite for each feature combination and print a summary."""
import subprocess
import sys
import time

COMBOS = [[], ["json"], ["yaml"], ["json", "yaml"], ["full"]]

results = []
for feats in COMBOS:
    args = ["cargo", "test", "--no-default-features"]
    if feats:
        args += ["--features", ",".join(feats)]
    label = ",".join(feats) or "(none)"
    print(f"==> {label}", flush=True)
    start = time.monotonic()
    proc = subprocess.run(args, capture_output=True, text=True)
    elapsed = time.monotonic() - start
    results.append((label, proc.returncode == 0, elapsed))
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)

print()
print(f"{'features':<16} {'result':<6} {'time':>8}")
for label, ok, t in results:
    print(f"{label:<16} {'ok' if ok else 'FAIL':<6} {t:>7.1f}s")
sys.exit(0 if all(ok for _, ok, _ in results) else 1)
