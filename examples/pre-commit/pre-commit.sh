#!/usr/bin/env bash
# Git pre-commit hook: refuse commits with formatting problems, leftover
# debug statements or large files. Install: copy to .git/hooks/pre-commit.
set -euo pipefail
MAX_SIZE=$((1024 * 1024))

STAGED=$(git diff --cached --name-only --diff-filter=ACM)
[ -z "$STAGED" ] && exit 0
fail=0

RS_FILES=$(echo "$STAGED" | grep '\.rs$' || true)
if [ -n "$RS_FILES" ]; then
  # Unquoted on purpose so each file is one argument; breaks on spaces in names.
  if ! rustfmt --check --edition 2021 $RS_FILES >/dev/null 2>&1; then
    echo "✗ rustfmt: run \`cargo fmt\`" >&2
    fail=1
  fi
fi

for f in $STAGED; do
  if git grep --cached -q -E 'dbg!\(|console\.log\(' -- "$f"; then
    echo "✗ $f: debug statement" >&2
    fail=1
  fi
  size=$(git cat-file -s ":$f")
  if [ "$size" -gt "$MAX_SIZE" ]; then
    echo "✗ $f: $((size / 1024)) KiB is over the 1 MiB limit" >&2
    fail=1
  fi
done
exit $fail
