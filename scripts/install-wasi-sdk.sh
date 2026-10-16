#!/usr/bin/env bash
# Installs the pinned wasi-sdk (the C toolchain that builds the script engine, SCR-08).
#
#   scripts/install-wasi-sdk.sh [--dest DIR]
#
# Downloads exactly the version in crates/kadakas-script-js/wasi-sdk.env for this operating system
# and processor (macOS and Linux, arm64 and x86_64; Windows in Git Bash), checks its SHA-256
# against the pinned value, and unpacks it to DIR (default:
# ${XDG_DATA_HOME:-~/.local/share}/wasi-sdk/<version>; the environment variable WASI_SDK_DEST also
# sets it). It prints the line to set WASI_SDK_PATH. Without bash on Windows, download the archive
# by hand (see docs/dev-guide.md); the hashes are in the same file.
# CI uses this script too, with a cache of the folder.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
pins="$here/../crates/kadakas-script-js/wasi-sdk.env"
[ -f "$pins" ] || { echo "error: $pins not found" >&2; exit 1; }
get() { grep -E "^$1=" "$pins" | head -n1 | cut -d= -f2-; }

version="$(get WASI_SDK_VERSION)"
tag="$(get WASI_SDK_TAG)"

dest="${WASI_SDK_DEST:-${XDG_DATA_HOME:-$HOME/.local/share}/wasi-sdk/$version}"
while [ $# -gt 0 ]; do
  case "$1" in
    --dest) dest="$2"; shift 2 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "error: unknown argument $1" >&2; exit 1 ;;
  esac
done

case "$(uname -s)" in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  # Git Bash on Windows (CI uses it). Without bash, install the archive by hand (docs/dev-guide.md).
  MINGW*|MSYS*|CYGWIN*) os=windows ;;
  *) echo "error: $(uname -s) is not supported by this script. See docs/dev-guide.md." >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64|aarch64) arch=arm64 ;;
  x86_64|amd64) arch=x86_64 ;;
  *) echo "error: the processor $(uname -m) has no pinned wasi-sdk" >&2; exit 1 ;;
esac

want="$(get "WASI_SDK_SHA256_${arch}_${os}")"
[ -n "$want" ] || { echo "error: no pinned hash for ${arch}-${os}" >&2; exit 1; }

if [ -f "$dest/VERSION" ] && [ "$(head -n1 "$dest/VERSION")" = "$version" ]; then
  echo "wasi-sdk $version is already installed in $dest"
  echo "export WASI_SDK_PATH=\"$dest\""
  exit 0
fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

name="wasi-sdk-${version}-${arch}-${os}.tar.gz"
url="https://github.com/WebAssembly/wasi-sdk/releases/download/${tag}/${name}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "Downloading $url"
curl --fail --location --silent --show-error --output "$work/$name" "$url"
got="$(sha256 "$work/$name")"
if [ "$got" != "$want" ]; then
  echo "error: the SHA-256 of $name is $got, but $want is pinned. Nothing was installed." >&2
  exit 1
fi
echo "SHA-256 is $got (as pinned)"

mkdir -p "$work/unpack"
tar -xzf "$work/$name" -C "$work/unpack"
inner="$(find "$work/unpack" -mindepth 1 -maxdepth 1 -type d | head -n1)"
[ -f "$inner/VERSION" ] || { echo "error: the archive has no VERSION file" >&2; exit 1; }
[ "$(head -n1 "$inner/VERSION")" = "$version" ] || { echo "error: the archive is not wasi-sdk $version" >&2; exit 1; }

mkdir -p "$(dirname "$dest")"
rm -rf "$dest"
mv "$inner" "$dest"
echo "Installed wasi-sdk $version in $dest"
echo "Set this in your shell profile (or in the shell that runs cargo):"
echo "export WASI_SDK_PATH=\"$dest\""
