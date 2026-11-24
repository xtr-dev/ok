#!/usr/bin/env bash
# List open issues labelled "bug" that nobody is assigned to, oldest first.
# Needs curl and jq, and GITHUB_TOKEN in the environment.
set -euo pipefail
REPO="${1:?usage: stale-bugs.sh owner/repo}"
LABEL="${LABEL:-bug}"
: "${GITHUB_TOKEN:?set GITHUB_TOKEN}"

curl -fsSL \
  -H "Authorization: Bearer $GITHUB_TOKEN" \
  -H "Accept: application/vnd.github+json" \
  "https://api.github.com/repos/$REPO/issues?state=open&labels=$LABEL&per_page=100" \
| jq -r '[.[] | select(.pull_request | not) | select(.assignees | length == 0)]
         | sort_by(.created_at) | .[]
         | "#\(.number)\t\(.created_at[0:10])\t\(.title)"'
