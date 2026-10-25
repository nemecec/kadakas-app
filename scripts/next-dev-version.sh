#!/usr/bin/env bash
# Prints the development version that follows a release.
#
#   scripts/next-dev-version.sh RELEASED_VERSION [OVERRIDE]
#
#   0.1.0        -> 0.1.1-dev       (after a stable release: the next patch)
#   0.1.0-beta.1 -> 0.1.0-dev       (after a pre-release: the version that the pre-releases lead to)
# OVERRIDE must end in -dev, be valid and be newer than RELEASED_VERSION. A development version
# is never released, so RELEASED_VERSION must not be one.
# Order: 0.1.0-beta.2 < 0.1.0-dev < 0.1.0 < 0.1.1-dev < 0.1.1 (kadakas_update::Version).
# (After a release candidate, -dev sorts below -rc.N: letters compare as text.)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ver="${1:?usage: next-dev-version.sh RELEASED_VERSION [OVERRIDE]}"
override="${2:-}"
re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9a-z]+([.-][0-9a-z]+)*)?$'
[[ "$ver" =~ $re ]] || { echo "bad version: $ver" >&2; exit 1; }
case "$ver" in *-dev|*-dev.*) echo "$ver is a development version, not a release" >&2; exit 1 ;; esac
core="${ver%%-*}"
if [ -n "$override" ]; then
  [[ "$override" =~ $re ]] || { echo "bad development version: $override" >&2; exit 1; }
  case "$override" in *-dev|*-dev.[0-9]*) ;; *) echo "the development version must end in -dev: $override" >&2; exit 1 ;; esac
  [ "$(python3 -I "$here/version_cmp.py" "$override" "$ver")" = 1 ] || { echo "$override is not newer than $ver" >&2; exit 1; }
  echo "$override"; exit 0
fi
if [ "$core" = "$ver" ]; then
  IFS=. read -r a b c <<<"$core"
  echo "$a.$b.$((c + 1))-dev"
else
  echo "$core-dev"
fi
