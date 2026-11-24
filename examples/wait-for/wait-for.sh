#!/usr/bin/env bash
# Wait until a URL answers with a 2xx status, then optionally run a command.
# usage: wait-for.sh <url> [timeout-seconds] [-- command args...]
set -euo pipefail
URL="${1:?usage: wait-for.sh <url> [timeout-seconds] [-- command args...]}"
shift
TIMEOUT=60
if [[ "${1:-}" =~ ^[0-9]+$ ]]; then TIMEOUT="$1"; shift; fi
[[ "${1:-}" == "--" ]] && shift

start=$(date +%s)
until curl -fsS -o /dev/null --max-time 2 "$URL" 2>/dev/null; do
  if (( $(date +%s) - start >= TIMEOUT )); then
    echo "timed out after ${TIMEOUT}s waiting for $URL" >&2
    exit 1
  fi
  sleep 1
done
echo "$URL is up after $(( $(date +%s) - start ))s"
if [[ $# -gt 0 ]]; then exec "$@"; fi
