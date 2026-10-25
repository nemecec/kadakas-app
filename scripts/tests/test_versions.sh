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
eq "wix beta" "$(pv wix 0.1.0-beta.1)" 0.1.0.10001
eq "wix dev" "$(pv wix 0.1.1-dev)" 0.1.1.15000
eq "wix rc" "$(pv wix 0.1.0-rc.2)" 0.1.0.20002
eq "wix alpha" "$(pv wix 0.1.0-alpha.3)" 0.1.0.3
eq "deb beta" "$(pv deb 0.1.0-beta.1)" '0.1.0~beta.1'
eq "deb dev" "$(pv deb 0.1.1-dev)" '0.1.1~dev'
eq "deb stable" "$(pv deb 0.1.0)" 0.1.0
eq "pep440 stable" "$(pv pep440 0.1.0)" 0.1.0
eq "pep440 beta" "$(pv pep440 0.1.0-beta.1)" 0.1.0b1
eq "pep440 dev" "$(pv pep440 0.1.1-dev)" 0.1.1.dev0
eq "pep440 rc" "$(pv pep440 0.1.0-rc.1)" 0.1.0rc1
eq "semver" "$(pv semver 0.1.0-beta.1)" 0.1.0-beta.1
fails_with "wix refuses a free label" scripts/packager-version.sh wix 0.1.0-x.1
fails_with "pep440 refuses a free label" scripts/packager-version.sh pep440 0.1.0-local.g1
fails_with "bad version" scripts/packager-version.sh deb 1.0
# The order of the WiX numbers is the order of the versions.
a=$(pv wix 0.1.0-alpha.1 | cut -d. -f4); b=$(pv wix 0.1.0-beta.9 | cut -d. -f4); d=$(pv wix 0.1.0-dev | cut -d. -f4); r=$(pv wix 0.1.0-rc.1 | cut -d. -f4)
eq "wix order" "$([ "$a" -lt "$b" ] && [ "$b" -lt "$d" ] && [ "$d" -lt "$r" ] && echo yes)" yes
nd() { scripts/next-dev-version.sh "$@" 2>/dev/null; }
eq "next after stable" "$(nd 0.1.0)" 0.1.1-dev
eq "next after beta" "$(nd 0.1.0-beta.2)" 0.1.0-dev
eq "next after 1.9.9" "$(nd 1.9.9)" 1.9.10-dev
eq "next override" "$(nd 0.1.0 0.2.0-dev)" 0.2.0-dev
fails_with "override must end in dev" scripts/next-dev-version.sh 0.1.0 0.2.0-beta.1
fails_with "override must be newer" scripts/next-dev-version.sh 0.1.0 0.0.9-dev
fails_with "released version is not dev" scripts/next-dev-version.sh 0.1.1-dev
vc() { python3 -I scripts/version_cmp.py "$@"; }
eq "beta1 < beta2" "$(vc 0.1.0-beta.1 0.1.0-beta.2)" -1
eq "beta2 < dev" "$(vc 0.1.0-beta.2 0.1.0-dev)" -1
eq "dev < release" "$(vc 0.1.0-dev 0.1.0)" -1
eq "release < next dev" "$(vc 0.1.0 0.1.1-dev)" -1
[ "$fails" = 0 ] || { echo "$fails failed" >&2; exit 1; }
echo "all version script tests passed"
