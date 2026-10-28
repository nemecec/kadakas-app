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
# The negative tests keep the exit code and the message, so a crash cannot pass as an error.
ren() { out="$("$here/rename-installers.sh" "$@" 2>&1)" && rc=0 || rc=$?; }
ren "$tmp/lin" "$ver" linux x64
[ "$rc" -eq 0 ] && ok "rename is repeatable" || bad "rename is repeatable: $out"
mkdir -p "$tmp/empty"
ren "$tmp/empty" "$ver" linux x64
[ "$rc" -eq 1 ] && [[ "$out" == *"no installer in"* ]] && ok "empty folder is an error" || bad "empty folder is an error (rc=$rc, $out)"
mk "$tmp/two" "$tmp/two/a.deb" "$tmp/two/b.deb"
ren "$tmp/two" "$ver" linux x64
[ "$rc" -eq 1 ] && [[ "$out" == *"more than one .deb"* ]] && ok "two .deb files are an error" || bad "two .deb files are an error (rc=$rc, $out)"
ren "$tmp/lin" "0.1" linux x64
[ "$rc" -eq 1 ] && [[ "$out" == *"bad version"* ]] && ok "bad version is an error" || bad "bad version is an error (rc=$rc, $out)"
ren "$tmp/lin" "$ver" beos x64
[ "$rc" -eq 1 ] && [[ "$out" == *"bad os"* ]] && ok "bad os is an error" || bad "bad os is an error (rc=$rc, $out)"

# The table. run() keeps the output and the exit code, so a crash cannot pass as a result.
table() { out="$("$here/release-notes-table.sh" "$@" 2>&1)" && rc=0 || rc=$?; }
assert_rc() { [ "$rc" -eq "$1" ] && ok "$2: exit code $1" || bad "$2: exit code $rc, wanted $1: $out"; }
assert_has() { [[ "$out" == *"$1"* ]] && ok "$2" || bad "$2 (no '$1' in: ${out:0:200})"; }
assert_lacks() { [[ "$out" != *"$1"* ]] && ok "$2" || bad "$2 (found '$1')"; }

rel="$tmp/rel"; mkdir -p "$rel"
cp "$tmp"/mac/Kadakas-* "$tmp"/win/Kadakas-* "$tmp"/lin/Kadakas-* "$rel"/
: > "$rel/Kadakas-$ver-macos-x64.dmg"
: > "$rel/kadakas-update-macos-aarch64.tar.gz"; : > "$rel/kadakas-web.tar.gz"
export PACKAGER_MACOS_CONFIG="$tmp/no-identity.toml"; printf '[macos]\n# signing-identity = "x"\n' > "$PACKAGER_MACOS_CONFIG"
table "$rel" "$ver" owner/repo "v$ver"
assert_rc 0 "table"
[ "$(echo "$out" | grep -c '^| ')" -eq 7 ] && ok "header and six rows" || bad "header and six rows"
assert_has "releases/download/v$ver/Kadakas-$ver-linux-x64.deb" "link to the deb"
assert_has 'kadakas-update-*' "sentence about update files"
assert_has "Open an unsigned build" "unsigned section"
assert_has "Apple could not verify" "macOS 15 text"
table "$rel" "$ver"
assert_rc 0 "table without links"
assert_lacks 'https://' "no links without repo"

rm "$rel/Kadakas-$ver-windows-x64.msi"
table "$rel" "$ver"
assert_rc 1 "missing file"
assert_has "missing release file: Kadakas-$ver-windows-x64.msi" "missing file is named"
: > "$rel/Kadakas-$ver-windows-x64.msi"; : > "$rel/Kadakas-$ver-linux-arm64.deb"
table "$rel" "$ver"
assert_rc 1 "extra installer"
assert_has "installer without a row: Kadakas-$ver-linux-arm64.deb" "extra installer is named"
rm "$rel/Kadakas-$ver-linux-arm64.deb"
table "$tmp/nothing" "$ver"
assert_rc 1 "missing folder"

# A stable version is unsigned too, as long as no signing identity is set.
for f in "$rel"/Kadakas-*; do mv "$f" "${f//$ver/0.1.0}"; done
table "$rel" 0.1.0
assert_rc 0 "stable table"
assert_has "Open an unsigned build" "stable has the unsigned section while nothing is signed"
printf '[macos]\nsigning-identity = "Developer ID Application: X (ABC)"\n' > "$PACKAGER_MACOS_CONFIG"
table "$rel" 0.1.0
assert_rc 0 "stable table, signed"
assert_lacks "Open an unsigned build" "no unsigned section when a signing identity is set"
UNSIGNED_NOTE=1 table "$rel" 0.1.0
assert_has "Open an unsigned build" "UNSIGNED_NOTE=1 shows the section"

[ "$fails" -eq 0 ] || { echo "$fails failed" >&2; exit 1; }
echo "all passed"
