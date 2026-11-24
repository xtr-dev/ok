#!/usr/bin/env bash
# Nightly backup: archive the data directory, keep the newest backups and
# remove anything older than 30 days.
set -euo pipefail
SRC="${SRC:-/var/lib/app/data}"
DEST="${DEST:-/backups/app}"
KEEP="${KEEP:-7}"
STAMP=$(date +%Y-%m-%d_%H%M%S)

mkdir -p "$DEST"
ARCHIVE="$DEST/data-$STAMP.tar.gz"
tar -czf "$ARCHIVE" -C "$(dirname "$SRC")" "$(basename "$SRC")"
(cd "$DEST" && sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256")

# Keep the newest $KEEP archives. Breaks on file names with newlines.
ls -1t "$DEST"/data-*.tar.gz | tail -n +$((KEEP + 1)) | while read -r old; do
  rm -f "$old" "$old.sha256"
done
# GNU find; BSD find needs -mtime +30d semantics checked by hand.
find "$DEST" -name 'data-*.tar.gz*' -mtime +30 -delete
echo "backup done: $ARCHIVE"
