#!/usr/bin/env bash
# Cut a release: check the tree, bump the version, test, tag and build binaries.
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <version> [--push]" >&2
  exit 2
fi
VERSION="$1"
PUSH="${2:-}"
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must look like 1.2.3, got $VERSION" >&2
  exit 2
fi

BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$BRANCH" != "main" ]]; then
  echo "error: releases are cut from main (on $BRANCH)" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree is dirty" >&2
  exit 1
fi
if git rev-parse --verify --quiet "v$VERSION" >/dev/null; then
  echo "error: tag v$VERSION already exists" >&2
  exit 1
fi

# GNU and BSD sed disagree about -i; the .bak dance works on both.
sed -i.bak -E "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml && rm Cargo.toml.bak
cargo test --workspace
git commit -am "chore: release v$VERSION"
git tag -a "v$VERSION" -m "v$VERSION"

TARGETS=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
mkdir -p dist
for t in "${TARGETS[@]}"; do
  cargo build --release --target "$t"
  cp "target/$t/release/myapp" "dist/myapp-$t"
done
# sha256sum is called shasum -a 256 on macOS.
(cd dist && sha256sum myapp-* > SHA256SUMS)

if [[ "$PUSH" == "--push" ]]; then
  git push --follow-tags
fi
echo "release v$VERSION ready in dist/"
