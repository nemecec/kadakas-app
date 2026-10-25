#!/usr/bin/env bash
# Prints the form of a Kadakas version that one kind of package needs.
#
#   scripts/packager-version.sh FORMAT VERSION
#
# VERSION is the workspace version, for example 0.1.0 or 0.1.0-beta.2 (a dash starts the pre-release part).
# FORMAT is one of:
#   semver  the version as it is (the app, the manifest, NSIS, AppImage, the macOS bundles, the tag)
#   pep440  the Python form: 0.1.0-beta.2 -> 0.1.0b2 (alpha -> a, beta -> b, rc -> rc), 0.1.1-dev -> 0.1.1.dev0
#   wix     the MSI form. WiX takes only numbers: major.minor.patch[.build], and the build is at most 65535.
#           The pre-release becomes the build number: alpha.N -> N, beta.N -> 10000+N, dev -> 15000,
#           rc.N -> 20000+N (N below 10000), so 0.1.0-beta.2 -> 0.1.0.10002. This is the order of
#           the versions: alpha < beta < dev < rc < the release (which has no fourth number). Windows Installer ignores the fourth number when it
#           compares versions: 0.1.0-beta.2 and 0.1.0 look the same to it.
#   deb     the Debian form: 0.1.0-beta.2 -> 0.1.0~beta.2, 0.1.1-dev -> 0.1.1~dev. A tilde sorts before the empty text, so the beta is
#           older than 0.1.0. A dash would start the Debian revision, and 0.1.0-beta.2 would sort after 0.1.0.
# A pre-release part that is not alpha.N, beta.N, rc.N or dev is an error for pep440 and wix.
set -euo pipefail

fmt="${1:?usage: packager-version.sh FORMAT VERSION}"
ver="${2:?usage: packager-version.sh FORMAT VERSION}"

core="${ver%%-*}"
pre=""
if [ "$core" != "$ver" ]; then pre="${ver#*-}"; fi
re_core='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
[[ "$core" =~ $re_core ]] || { echo "bad version: $ver" >&2; exit 1; }

label="" num=""
if [ -n "$pre" ]; then
  re_pre='^(alpha|beta|rc)\.(0|[1-9][0-9]{0,3})$'
  if [[ "$pre" =~ $re_pre ]]; then label="${BASH_REMATCH[1]}"; num="${BASH_REMATCH[2]}"; fi
  if [ "$pre" = dev ]; then label=dev; num=0; fi
fi
need_label() { [ -z "$pre" ] || [ -n "$label" ] || { echo "the pre-release part \"$pre\" must be alpha.N, beta.N, rc.N or dev for $fmt" >&2; exit 1; }; }

case "$fmt" in
  semver) echo "$ver" ;;
  deb) if [ -n "$pre" ]; then echo "$core~$pre"; else echo "$core"; fi ;;
  pep440)
    need_label
    case "$label" in
      "") echo "$core" ;;
      alpha) echo "${core}a$num" ;;
      beta) echo "${core}b$num" ;;
      dev) echo "$core.dev0" ;;
      rc) echo "${core}rc$num" ;;
    esac ;;
  wix)
    need_label
    case "$label" in
      "") echo "$core" ;;
      alpha) echo "$core.$num" ;;
      beta) echo "$core.$((10000 + num))" ;;
      dev) echo "$core.15000" ;;
      rc) echo "$core.$((20000 + num))" ;;
    esac ;;
  *) echo "unknown format: $fmt" >&2; exit 1 ;;
esac
