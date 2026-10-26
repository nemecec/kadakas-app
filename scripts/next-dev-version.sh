#!/usr/bin/env bash
# Prints the development version that follows a release.
#
#   scripts/next-dev-version.sh RELEASED_VERSION [OVERRIDE]
#
#   0.1.0        -> 0.1.1-dev       (after a stable release: the next patch)
#   0.1.0-beta.1 -> 0.1.0-dev       (after a pre-release: the version that the pre-releases lead to)
# OVERRIDE must be exactly X.Y.Z-dev. The result (computed or OVERRIDE) must be newer than
# RELEASED_VERSION, else the script fails. A development version is never released, so
# RELEASED_VERSION must not be one.
# Order: 0.1.0-beta.2 < 0.1.0-dev < 0.1.0 < 0.1.1-dev < 0.1.1 (kadakas_update::Version).
# After a release candidate the computed 0.1.0-dev is older than 0.1.0-rc.1 (letters compare as
# text), so the script fails and asks for an OVERRIDE, for example 0.2.0-dev.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ver="${1:?usage: next-dev-version.sh RELEASED_VERSION [OVERRIDE]}"
override="${2:-}"
python3 -I "$here/version_cmp.py" valid "$ver" || exit 1
if [[ "$ver" == *-* ]] && [[ "${ver#*-}" =~ ^dev(\.|$) ]]; then echo "$ver is a development version, not a release" >&2; exit 1; fi
core="${ver%%-*}"
if [ -n "$override" ]; then
  python3 -I "$here/version_cmp.py" valid "$override" || exit 1
  re_dev='^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})-dev$'
  [[ "$override" =~ $re_dev ]] || { echo "the development version must be X.Y.Z-dev: $override" >&2; exit 1; }
  next="$override"
elif [ "$core" = "$ver" ]; then
  IFS=. read -r a b c <<<"$core"
  next="$a.$b.$((c + 1))-dev"
else
  next="$core-dev"
fi
if [ "$(python3 -I "$here/version_cmp.py" "$next" "$ver")" != 1 ]; then
  echo "the development version $next is not newer than $ver. Give an explicit next development version (for example the next minor, X.Y.0-dev)." >&2
  exit 1
fi
echo "$next"
