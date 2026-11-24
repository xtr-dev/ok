#!/usr/bin/env python3
"""Render the app config for one environment: config/base.yaml merged with
config/<env>.yaml, secrets taken from the environment, written as JSON."""
import json
import os
import sys

import yaml  # third-party: pip install pyyaml


def merge(a, b):
    """Deep-merge two dicts; values from b win, nested dicts are merged."""
    out = dict(a)
    for k, v in b.items():
        if isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = merge(out[k], v)
        else:
            out[k] = v
    return out


env = sys.argv[1] if len(sys.argv) > 1 else "dev"
with open("config/base.yaml") as f:
    base = yaml.safe_load(f)
with open(f"config/{env}.yaml") as f:
    overrides = yaml.safe_load(f) or {}  # an empty file loads as None

cfg = merge(base, overrides)
cfg["database"]["password"] = os.environ["DB_PASSWORD"]

os.makedirs("build", exist_ok=True)
out = f"build/config.{env}.json"
with open(out, "w") as f:
    json.dump(cfg, f, indent=2)
    f.write("\n")
print(out)
