#!/usr/bin/env bash
# Summarise an application log: lines per level and the most common errors.
# Log lines look like: 2026-11-20T10:00:01Z ERROR db connection refused
set -euo pipefail
LOG="${1:-app.log}"
TOP="${TOP:-5}"

echo "== $LOG =="
echo "lines: $(wc -l < "$LOG" | tr -d ' ')"
for level in ERROR WARN INFO; do
  # grep -c exits 1 when it counts zero lines, hence the || true.
  printf "%-6s %6d\n" "$level" "$(grep -c " $level " "$LOG" || true)"
done
echo
echo "top $TOP errors:"
grep " ERROR " "$LOG" | cut -d' ' -f3- | sort | uniq -c | sort -rn | head -n "$TOP"
