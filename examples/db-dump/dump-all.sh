#!/usr/bin/env bash
# Dump every database on the server to dumps/<db>-<date>.sql.gz.
set -euo pipefail
OUT_DIR="${OUT_DIR:-dumps}"
mkdir -p "$OUT_DIR"
DATE=$(date +%F)

DBS=$(psql -At -c "SELECT datname FROM pg_database WHERE NOT datistemplate AND datname <> 'postgres'")
for db in $DBS; do
  echo "dumping $db"
  pg_dump --no-owner "$db" > "$OUT_DIR/$db-$DATE.sql"
  gzip -f "$OUT_DIR/$db-$DATE.sql"
done
ls -lh "$OUT_DIR"
