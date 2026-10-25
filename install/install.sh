#!/bin/sh
# Install rt, the review-tools command line, from its GitHub releases:
#
#   curl -fsSL https://github.com/gschjetne/rt-cli/releases/latest/download/install.sh | sh
#
# The latest release, or RT_VERSION's (such as v0.1.0). Checks the download against the
# release's SHA256SUMS before running it; `gh attestation verify` can also check where it
# was built. Puts rt in ~/.local/bin (or $RT_INSTALL_DIR). Linux and macOS, on x86-64 and
# arm64.
set -eu
repo=https://github.com/gschjetne/rt-cli
case "${RT_VERSION:-latest}" in
  latest) base=$repo/releases/latest/download ;;
  *) base=$repo/releases/download/$RT_VERSION ;;
esac
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-linux ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-linux ;;
  Darwin-x86_64) target=x86_64-macos ;;
  Darwin-arm64) target=aarch64-macos ;;
  *) echo "no rt build for $(uname -s) $(uname -m); build it from source ($repo)" >&2; exit 1 ;;
esac
dir=${RT_INSTALL_DIR:-$HOME/.local/bin}
mkdir -p "$dir"
tmp="$dir/.rt.$$"
trap 'rm -f "$tmp"' EXIT
curl -fsSL "$base/rt-$target" -o "$tmp"
# What the release says it holds, checked before anything runs.
expected=$(curl -fsSL "$base/SHA256SUMS" | awk -v f="rt-$target" '$2 == f { print $1 }')
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$tmp" | awk '{ print $1 }')
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "the download of rt-$target does not match the release's SHA256SUMS; not installed" >&2
  exit 1
fi
chmod +x "$tmp"
mv "$tmp" "$dir/rt"
echo "installed $("$dir/rt" --version) to $dir/rt"
case ":$PATH:" in *":$dir:"*) ;; *) echo "add $dir to PATH" ;; esac
