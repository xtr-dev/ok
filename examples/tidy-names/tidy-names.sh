#!/usr/bin/env bash
# Normalise file names in a folder: lowercase, runs of spaces and underscores
# become dashes, and each name gets its modification date as a prefix.
# Pass -n first for a dry run.
set -euo pipefail
DRY=0
if [ "${1:-}" = "-n" ]; then DRY=1; shift; fi
DIR="${1:-.}"

for f in "$DIR"/*; do
  [ -f "$f" ] || continue
  base=$(basename "$f")
  # GNU date has -r FILE; BSD/macOS needs stat -f.
  date=$(date -r "$f" +%Y-%m-%d 2>/dev/null || stat -f %Sm -t %Y-%m-%d "$f")
  new=$(printf '%s' "$base" | tr '[:upper:]' '[:lower:]' | sed -E 's/[ _]+/-/g')
  case "$new" in
    [0-9][0-9][0-9][0-9]-*) ;;
    *) new="$date-$new" ;;
  esac
  [ "$base" = "$new" ] && continue
  if [ -e "$DIR/$new" ]; then
    echo "skip $base: $new exists" >&2
    continue
  fi
  echo "$base -> $new"
  [ $DRY -eq 1 ] || mv "$f" "$DIR/$new"
done
