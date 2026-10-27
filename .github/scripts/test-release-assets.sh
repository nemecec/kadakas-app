#!/usr/bin/env bash
# Tests rename-installers.sh and release-notes-table.sh in a temporary folder.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
ok() { echo "ok: $1"; }
bad() { echo "FAIL: $1" >&2; fails=$((fails + 1)); }
has() { [ -f "$1" ] && ok "$2" || bad "$2 (missing $1)"; }

ver=0.1.0-beta.1
mk() { mkdir -p "$1"; shift; for f in "$@"; do : > "$f"; done; }

# Packager names in, one pattern out.
mk "$tmp/mac" "$tmp/mac/Kadakas_0.1.0-beta.1_aarch64.dmg" "$tmp/mac/kadakas-update-macos-aarch64.tar.gz"
"$here/rename-installers.sh" "$tmp/mac" "$ver" macos arm64 > /dev/null
has "$tmp/mac/Kadakas-$ver-macos-arm64.dmg" "macOS dmg renamed"
has "$tmp/mac/kadakas-update-macos-aarch64.tar.gz" "update archive kept"

mk "$tmp/win" "$tmp/win/kadakas-launcher_0.1.0-beta.1_x64-setup.exe" "$tmp/win/kadakas-launcher_0.1.0-10001_x64_en-US.msi" "$tmp/win/kadakas-update-windows-x86_64.tar.gz"
"$here/rename-installers.sh" "$tmp/win" "$ver" windows x64 > /dev/null
has "$tmp/win/Kadakas-$ver-windows-x64.exe" "exe renamed"
has "$tmp/win/Kadakas-$ver-windows-x64.msi" "msi renamed with the semver"

mk "$tmp/lin" "$tmp/lin/kadakas-launcher_0.1.0-beta.1_x86_64.AppImage" "$tmp/lin/kadakas-launcher_0.1.0~beta.1_amd64.deb" "$tmp/lin/kadakas-update-linux-x86_64.tar.gz"
"$here/rename-installers.sh" "$tmp/lin" "$ver" linux x64 > /dev/null
has "$tmp/lin/Kadakas-$ver-linux-x64.AppImage" "AppImage renamed"
has "$tmp/lin/Kadakas-$ver-linux-x64.deb" "deb renamed with a dash"
# A second run changes nothing.
"$here/rename-installers.sh" "$tmp/lin" "$ver" linux x64 > /dev/null && ok "rename is repeatable" || bad "rename is repeatable"

mkdir -p "$tmp/empty"
if "$here/rename-installers.sh" "$tmp/empty" "$ver" linux x64 2> /dev/null; then bad "empty folder is an error"; else ok "empty folder is an error"; fi
mk "$tmp/two" "$tmp/two/a.deb" "$tmp/two/b.deb"
if "$here/rename-installers.sh" "$tmp/two" "$ver" linux x64 2> /dev/null; then bad "two .deb files are an error"; else ok "two .deb files are an error"; fi
if "$here/rename-installers.sh" "$tmp/lin" "0.1" linux x64 2> /dev/null; then bad "bad version is an error"; else ok "bad version is an error"; fi

# The table.
rel="$tmp/rel"; mkdir -p "$rel"
cp "$tmp"/mac/Kadakas-* "$tmp"/win/Kadakas-* "$tmp"/lin/Kadakas-* "$rel"/
: > "$rel/Kadakas-$ver-macos-x64.dmg"
: > "$rel/kadakas-update-macos-aarch64.tar.gz"; : > "$rel/kadakas-web.tar.gz"
out="$("$here/release-notes-table.sh" "$rel" "$ver" owner/repo "v$ver")"
[ "$(echo "$out" | grep -c '^| ' )" -eq 7 ] && ok "header and six rows" || bad "header and six rows"
echo "$out" | grep -q "releases/download/v$ver/Kadakas-$ver-linux-x64.deb" && ok "link to the deb" || bad "link to the deb"
echo "$out" | grep -q "kadakas-update-\*" && ok "sentence about update files" || bad "sentence about update files"
echo "$out" | grep -q "not signed" && ok "pre-release has the unsigned note" || bad "pre-release has the unsigned note"
"$here/release-notes-table.sh" "$rel" "$ver" | grep -q 'https://' && bad "no links without repo" || ok "no links without repo"

rm "$rel/Kadakas-$ver-windows-x64.msi"
if "$here/release-notes-table.sh" "$rel" "$ver" > /dev/null 2>&1; then bad "missing file is an error"; else ok "missing file is an error"; fi
: > "$rel/Kadakas-$ver-windows-x64.msi"; : > "$rel/Kadakas-$ver-linux-arm64.deb"
if "$here/release-notes-table.sh" "$rel" "$ver" > /dev/null 2>&1; then bad "extra installer is an error"; else ok "extra installer is an error"; fi
rm "$rel/Kadakas-$ver-linux-arm64.deb"
# A stable version has no unsigned note.
for f in "$rel"/Kadakas-*; do mv "$f" "${f//$ver/0.1.0}"; done
"$here/release-notes-table.sh" "$rel" 0.1.0 | grep -q "not signed" && bad "stable has no unsigned note" || ok "stable has no unsigned note"

[ "$fails" -eq 0 ] || { echo "$fails failed" >&2; exit 1; }
echo "all passed"
