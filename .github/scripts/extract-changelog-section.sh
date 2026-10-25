#!/usr/bin/env bash
#
# Prints the body of one version in CHANGELOG.md (without the "## [x]" line and without the
# link lines). The release workflow gives it to the GitHub release as the notes.
#
# Usage: extract-changelog-section.sh VERSION      (exit 1 when the section is missing or empty)
set -euo pipefail
version="${1:?usage: extract-changelog-section.sh VERSION}"
changelog="${CHANGELOG:-CHANGELOG.md}"
out="$(awk -v head="## [${version}]" '
  index($0, head) == 1 { on = 1; next }
  /^## \[/ || /^\[[^]]+\]: / { on = 0 }
  on { print }
' "$changelog")"
# Remove empty lines at the start and at the end.
out="$(printf '%s\n' "$out" | awk 'NF { f = 1 } f { print }' | awk '{ l[NR] = $0 } END { n = NR; while (n > 0 && l[n] !~ /[^[:space:]]/) n--; for (i = 1; i <= n; i++) print l[i] }')"
[ -n "$out" ] || { echo "No section for $version in $changelog" >&2; exit 1; }
printf '%s\n' "$out"
