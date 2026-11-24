#!/usr/bin/env bash
# Check that a developer machine has the tools and settings the project needs.
missing=0
for cmd in git node npm docker; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing tool: $cmd" >&2
    missing=1
  fi
done

NODE_MAJOR=$(node --version 2>/dev/null | sed -E 's/^v([0-9]+).*/\1/')
if [ -n "$NODE_MAJOR" ] && [ "$NODE_MAJOR" -lt 20 ]; then
  echo "node $NODE_MAJOR is too old (need 20 or newer)" >&2
  missing=1
fi

for var in DATABASE_URL API_KEY; do
  # ${!var} is bash indirection: the value of the variable named by $var.
  if [ -z "${!var:-}" ]; then
    echo "missing environment variable: $var" >&2
    missing=1
  fi
done

[ -f .env ] || echo "hint: copy .env.example to .env"
if [ $missing -eq 0 ]; then echo "all good"; else exit 1; fi
