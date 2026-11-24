#!/usr/bin/env bash
# Download a pinned release of a tool, verify its checksum and install it into ./bin.
set -euo pipefail
NAME=protolint
VERSION=1.6.0

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)          TARGET=linux-amd64 ;;
  Linux-aarch64)         TARGET=linux-arm64 ;;
  Darwin-arm64)          TARGET=darwin-arm64 ;;
  Darwin-x86_64)         TARGET=darwin-amd64 ;;
  MINGW64*-x86_64)       TARGET=windows-amd64 ;;   # only under Git Bash
  *) echo "unsupported platform: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

URL="https://github.com/example/$NAME/releases/download/v$VERSION/$NAME-$TARGET.tar.gz"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
curl -fsSL "$URL" -o "$TMP/archive.tar.gz"

EXPECTED=$(grep " $NAME-$TARGET.tar.gz\$" checksums.txt | cut -d' ' -f1)
if command -v sha256sum >/dev/null; then
  ACTUAL=$(sha256sum "$TMP/archive.tar.gz" | cut -d' ' -f1)
else
  ACTUAL=$(shasum -a 256 "$TMP/archive.tar.gz" | cut -d' ' -f1)   # macOS
fi
if [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "checksum mismatch for $TARGET: expected $EXPECTED, got $ACTUAL" >&2
  exit 1
fi

tar -xzf "$TMP/archive.tar.gz" -C "$TMP"
mkdir -p bin
install -m 0755 "$TMP/$NAME" "bin/$NAME"
echo "installed bin/$NAME $VERSION"
