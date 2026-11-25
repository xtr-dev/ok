#!/bin/sh
# Install ok on Linux or macOS:
#
#   curl -fsSL https://xtr-dev.github.io/ok/install.sh | sh
#
# Downloads the release binary for this machine from GitHub, checks it
# against the release's SHA256SUMS, and installs it as `ok` in
# ~/.local/bin (or $OK_INSTALL_DIR). Set OK_VERSION=0.1.0 to pin a version;
# the default is the latest release.
set -eu

repo="https://github.com/xtr-dev/ok/releases"
if [ -n "${OK_VERSION:-}" ]; then
  base="$repo/download/v${OK_VERSION#v}"
else
  base="$repo/latest/download"
fi
dir="${OK_INSTALL_DIR:-$HOME/.local/bin}"

fail() { echo "ok install: $*" >&2; exit 1; }

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)                target=x86_64-unknown-linux-musl ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl ;;
  Darwin-x86_64)               target=x86_64-apple-darwin ;;
  Darwin-arm64)                target=aarch64-apple-darwin ;;
  *) fail "no prebuilt ok for $(uname -s) $(uname -m); see https://github.com/xtr-dev/ok#build-from-source" ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q "$1" -O "$2"; }
else
  fail "curl or wget is needed"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading ok for $target"
fetch "$base/ok-$target" "$tmp/ok" || fail "download failed: $base/ok-$target"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "download failed: $base/SHA256SUMS"

expected=$(awk -v f="ok-$target" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS has no entry for ok-$target"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/ok" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$tmp/ok" | cut -d' ' -f1)
fi
[ "$actual" = "$expected" ] || fail "checksum mismatch (expected $expected, got $actual); not installing"

mkdir -p "$dir"
chmod +x "$tmp/ok"
mv "$tmp/ok" "$dir/ok"
echo "installed $("$dir/ok" --version) to $dir/ok"

case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "note: $dir is not on your PATH; add it, for example with:"
     echo "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.profile" ;;
esac
