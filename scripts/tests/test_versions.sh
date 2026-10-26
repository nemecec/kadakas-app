#!/usr/bin/env bash
# Tests the version scripts: packager-version.sh (one mapping for every package), next-dev-version.sh
# and version_cmp.py. No cargo, no network.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
fails=0
eq() { # label got want
  if [ "$2" = "$3" ]; then echo "ok: $1"; else echo "FAIL: $1: got '$2', want '$3'" >&2; fails=$((fails + 1)); fi
}
fails_with() { # label command...
  local l="$1"; shift
  if "$@" >/dev/null 2>&1; then echo "FAIL: $l should fail" >&2; fails=$((fails + 1)); else echo "ok: $l"; fi
}
pv() { scripts/packager-version.sh "$@" 2>/dev/null; }
eq "wix stable" "$(pv wix 0.1.0)" 0.1.0
eq "wix beta" "$(pv wix 0.1.0-beta.1)" 0.1.0-10001
eq "wix dev" "$(pv wix 0.1.1-dev)" 0.1.1-20000
eq "wix rc" "$(pv wix 0.1.0-rc.2)" 0.1.0-30002
eq "wix alpha" "$(pv wix 0.1.0-alpha.3)" 0.1.0-3
eq "deb beta" "$(pv deb 0.1.0-beta.1)" '0.1.0~beta.1'
eq "deb dev" "$(pv deb 0.1.1-dev)" '0.1.1~dev'
eq "deb stable" "$(pv deb 0.1.0)" 0.1.0
eq "pep440 stable" "$(pv pep440 0.1.0)" 0.1.0
eq "pep440 beta" "$(pv pep440 0.1.0-beta.1)" 0.1.0b1
eq "pep440 dev" "$(pv pep440 0.1.1-dev)" 0.1.1rc0.dev0
eq "pep440 rc" "$(pv pep440 0.1.0-rc.1)" 0.1.0rc1
eq "semver" "$(pv semver 0.1.0-beta.1)" 0.1.0-beta.1
fails_with "wix refuses a free label" scripts/packager-version.sh wix 0.1.0-x.1
fails_with "pep440 refuses a free label" scripts/packager-version.sh pep440 0.1.0-local.g1
fails_with "bad version" scripts/packager-version.sh deb 1.0
fails_with "wix: major above 255" scripts/packager-version.sh wix 256.0.0
fails_with "wix: patch above 65535" scripts/packager-version.sh wix 0.0.65536
eq "wix: the limits are fine" "$(pv wix 255.255.65535)" 255.255.65535
# The order of the WiX numbers is the order of the versions.
a=$(pv wix 0.1.0-alpha.1 | cut -d- -f2); b=$(pv wix 0.1.0-beta.9999 | cut -d- -f2); d=$(pv wix 0.1.0-dev | cut -d- -f2); r=$(pv wix 0.1.0-rc.1 | cut -d- -f2)
eq "wix order" "$([ "$a" -lt "$b" ] && [ "$b" -lt "$d" ] && [ "$d" -lt "$r" ] && echo yes)" yes
nd() { scripts/next-dev-version.sh "$@" 2>/dev/null; }
eq "next after stable" "$(nd 0.1.0)" 0.1.1-dev
eq "next after beta" "$(nd 0.1.0-beta.2)" 0.1.0-dev
eq "next after 1.9.9" "$(nd 1.9.9)" 1.9.10-dev
eq "next override" "$(nd 0.1.0 0.2.0-dev)" 0.2.0-dev
fails_with "override must end in dev" scripts/next-dev-version.sh 0.1.0 0.2.0-beta.1
fails_with "override dev.N is refused" scripts/next-dev-version.sh 0.1.0 0.2.0-dev.1
fails_with "after an rc the computed dev version is older" scripts/next-dev-version.sh 0.1.0-rc.1
eq "after an rc an override works" "$(nd 0.1.0-rc.1 0.2.0-dev)" 0.2.0-dev
eq "beta.1-dev is no development version" "$(nd 0.1.0-beta.1-dev)" 0.1.0-dev
fails_with "dev.2 is a development version" scripts/next-dev-version.sh 0.1.0-dev.2
fails_with "label dev.1 cannot be mapped" scripts/packager-version.sh wix 0.1.0-dev.1
fails_with "leading zero" scripts/packager-version.sh deb 0.1.0-beta.01
fails_with "double hyphen" scripts/packager-version.sh deb 0.0.0--x
fails_with "trailing newline" scripts/packager-version.sh deb $'0.1.0\n'
fails_with "too long" scripts/packager-version.sh deb 0.1.0-beta.1111111111111111111111111111111111
fails_with "valid: leading zero" python3 -I scripts/version_cmp.py valid 0.1.0-beta.01
fails_with "valid: newline" python3 -I scripts/version_cmp.py valid $'0.1.0\n'

fails_with "override must be newer" scripts/next-dev-version.sh 0.1.0 0.0.9-dev
fails_with "released version is not dev" scripts/next-dev-version.sh 0.1.1-dev
vc() { python3 -I scripts/version_cmp.py "$@"; }
eq "beta1 < beta2" "$(vc 0.1.0-beta.1 0.1.0-beta.2)" -1
eq "beta2 < dev" "$(vc 0.1.0-beta.2 0.1.0-dev)" -1
eq "dev < release" "$(vc 0.1.0-dev 0.1.0)" -1
eq "release < next dev" "$(vc 0.1.0 0.1.1-dev)" -1
# Windows (Git bash): python3 is often missing or a stub of the Store that fails. The scripts then
# use python, or $KADAKAS_PYTHON.
real_python="$(command -v python3)"
stub=$(mktemp -d)
printf '#!/bin/sh\nexit 49\n' > "$stub/python3"; chmod +x "$stub/python3"
ln -s "$real_python" "$stub/python"
eq "a failing python3 stub is skipped" "$(PATH="$stub:$PATH" pv pep440 0.1.0-beta.1)" 0.1.0b1
eq "KADAKAS_PYTHON is used" "$(PATH="$stub:$PATH" KADAKAS_PYTHON="$real_python" pv pep440 0.1.0-beta.1)" 0.1.0b1
rm -rf "$stub"

# The macOS bundles: short version = the numbers, CFBundleVersion = a build number, full version in KadakasVersion.
mac=$(mktemp -d); mkdir -p "$mac/rel"; for b in kadakas-app kadakas kadakas-modeler; do : > "$mac/rel/$b"; done
BUILD_NUMBER=20261026.103000 scripts/macos-app.sh "$mac/rel" "$mac/out" 0.1.0-beta.1 >/dev/null
plist_value() { awk -v k="$2" '$0 ~ "<key>" k "</key>" { getline; gsub(/.*<string>|<\/string>.*/, ""); print; exit }' "$1"; }
for plist in "$mac/out/Info.plist" "$mac/out/Kadakas Engine.app/Contents/Info.plist"; do
  eq "plist short version" "$(plist_value "$plist" CFBundleShortVersionString)" 0.1.0
  eq "plist KadakasVersion" "$(plist_value "$plist" KadakasVersion)" 0.1.0-beta.1
done
eq "inner CFBundleVersion is the build number" "$(plist_value "$mac/out/Kadakas Engine.app/Contents/Info.plist" CFBundleVersion)" 20261026.103000
fails_with "a bad build number" env BUILD_NUMBER=1.2-x scripts/macos-app.sh "$mac/rel" "$mac/out" 0.1.0
rm -rf "$mac"

# The record of the highest published version (replay guard).
pg() { python3 -I scripts/published_guard.py "$@"; }
rec="$(mktemp -d)/hp.json"
expect_ok() { if "$@" >/dev/null 2>&1; then echo "ok: $*"; else echo "FAIL: $*" >&2; fails=$((fails + 1)); fi; }
expect_ok pg check "$rec" 0.0.1
pg record "$rec" 0.1.0-beta.2 beta
expect_ok pg check "$rec" 0.1.0-beta.3 --channel beta
fails_with "a lower beta is refused" pg check "$rec" 0.1.0-beta.1 --channel beta
fails_with "the same version is refused for a new release" pg check "$rec" 0.1.0-beta.2 --channel beta
expect_ok pg check "$rec" 0.1.0-beta.2 --channel beta --allow-equal
expect_ok pg check "$rec" 0.0.5 --channel stable
fails_with "a version below any channel is refused" pg check "$rec" 0.0.5
pg record "$rec" 0.1.0 stable beta
pg record "$rec" 0.1.0-beta.9 beta
eq "the record never goes down" "$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d['beta'], d['stable'])" "$rec")" "0.1.0 0.1.0"
fails_with "a beta after the stable release is refused" pg check "$rec" 0.1.0-beta.10 --channel beta
fails_with "a bad record is refused" pg record "$rec" 0.1.0 nightly
# Hotfix: a stable 0.1.1 follows 0.1.0 although 0.2.0-beta.1 exists. A beta follows all.
pg record "$rec" 0.2.0-beta.1 beta
expect_ok pg check "$rec" 0.1.1 --channel stable
fails_with "a stable version below the stable record is refused" pg check "$rec" 0.1.0 --channel stable
fails_with "a beta below the newest record is refused" pg check "$rec" 0.1.5-beta.1
expect_ok pg order 0.1.1 v0.1.0 v0.2.0-beta.1
fails_with "a stable version is not newer than a stable tag" pg order 0.1.0 v0.1.0 v0.0.9
fails_with "a pre-release is compared with all tags" pg order 0.1.5-beta.1 v0.1.0 v0.2.0-beta.1
expect_ok pg order 0.2.0-beta.2 v0.1.0 v0.2.0-beta.1
expect_ok pg order 0.1.0-beta.1
rm -rf "$(dirname "$rec")"

# set-version.sh puts every file back when a step fails (an empty CARGO_HOME: cargo update --offline fails).
if command -v cargo >/dev/null 2>&1; then
  files="Cargo.toml packager.toml packager-macos.toml python/pyproject.toml python/kadakas/__init__.py Cargo.lock THIRD_PARTY_NOTICES.txt"
  # shellcheck disable=SC2086
  before=$(cat $files | cksum)
  empty_home=$(mktemp -d)
  CARGO_HOME="$empty_home" scripts/set-version.sh 9.9.9-beta.9 >/dev/null 2>&1
  status=$?
  # shellcheck disable=SC2086
  after=$(cat $files | cksum)
  rm -rf "$empty_home"
  eq "set-version fails without the crates index" "$([ "$status" != 0 ] && echo yes)" yes
  eq "set-version restores all files after a failure" "$after" "$before"
fi
[ "$fails" = 0 ] || { echo "$fails failed" >&2; exit 1; }
echo "all version script tests passed"
