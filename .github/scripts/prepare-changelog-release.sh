#!/usr/bin/env bash
#
# Promotes the "Unreleased" section of CHANGELOG.md to a dated release section and updates the
# compare links at the bottom. Adapted from zeroturnaround/zt-zip. The release workflow
# (.github/workflows/prepare-release.yml) runs it. You can run it by hand too.
#
# Usage: prepare-changelog-release.sh VERSION      for example 0.1.0-beta.1
#
# The date is today in UTC. The tag is "v<VERSION>". The file is CHANGELOG.md, or $CHANGELOG
# (the date is $CHANGELOG_DATE: prepare-release.yml takes it once in the plan job and passes it on). CHANGELOG_ALLOW_EMPTY=true promotes an empty section.
#
# The link of [Unreleased] is either ".../compare/<previous tag>...HEAD" or, before the first
# release, ".../commits/main". The new section compares with the previous entry of the file
# (a stable version after betas compares with the last beta). The first release has the link
# ".../releases/tag/v<VERSION>".
set -euo pipefail

version="${1:?usage: prepare-changelog-release.sh VERSION}"
changelog="${CHANGELOG:-CHANGELOG.md}"
date="${CHANGELOG_DATE:-$(date -u +%Y-%m-%d)}"
new_tag="v${version}"
re='^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})(-[0-9a-z]+([.-][0-9a-z]+)*)?$'
[[ "$version" =~ $re ]] || { echo "bad version: $version" >&2; exit 1; }

# Windows line ends would end up in the links. Refuse them with a clear message.
if grep -q $'\r' "$changelog"; then
  echo "$changelog has Windows line ends (CRLF). Convert it to LF first (for example with dos2unix)." >&2
  exit 1
fi

unreleased_link="$(grep -E '^\[Unreleased\]:' "$changelog" || true)"
[ -n "$unreleased_link" ] || { echo "No [Unreleased] link in $changelog" >&2; exit 1; }
base_url="$(printf '%s\n' "$unreleased_link" | sed -E 's#^\[Unreleased\]: (https?://[^ ]+)/(compare|commits)/.*#\1#')"
[ "$base_url" != "$unreleased_link" ] || { echo "Could not parse the [Unreleased] link in $changelog" >&2; exit 1; }
prev_tag=""
case "$unreleased_link" in
  *"/compare/"*"...HEAD") prev_tag="$(printf '%s\n' "$unreleased_link" | sed -E 's#^.*/compare/(.+)\.\.\.HEAD$#\1#')" ;;
esac

# Refuse a version that is in the file already.
if grep -qF "## [${version}]" "$changelog" || grep -qF "[${version}]:" "$changelog"; then
  echo "Version $version is in $changelog already." >&2
  exit 1
fi

# Refuse an empty Unreleased section.
if [ "${CHANGELOG_ALLOW_EMPTY:-false}" != true ] && ! awk '
    /^## \[Unreleased\]/ { in_section = 1; next }
    /^## \[/ || /^\[[^]]+\]:/ { in_section = 0 }
    # A heading (### Added) without a line under it is no content.
    in_section && /[^[:space:]]/ && !/^###/ { found = 1 }
    END { exit found ? 0 : 1 }
  ' "$changelog"; then
  echo "The [Unreleased] section is empty; nothing to release." >&2
  exit 1
fi

# The text goes to awk through the environment: -v would change backslashes.
VERSION="$version" DATE="$date" NEW_TAG="$new_tag" PREV_TAG="$prev_tag" BASE_URL="$base_url" awk '
  BEGIN { version = ENVIRON["VERSION"]; date = ENVIRON["DATE"]; new_tag = ENVIRON["NEW_TAG"]; prev_tag = ENVIRON["PREV_TAG"]; base_url = ENVIRON["BASE_URL"] }
  /^## \[Unreleased\]/ {
    print
    print ""
    print "## [" version "] - " date
    next
  }
  /^\[Unreleased\]:/ {
    print "[Unreleased]: " base_url "/compare/" new_tag "...HEAD"
    if (prev_tag == "") print "[" version "]: " base_url "/releases/tag/" new_tag
    else print "[" version "]: " base_url "/compare/" prev_tag "..." new_tag
    next
  }
  { print }
' "$changelog" > "$changelog.tmp"
mv "$changelog.tmp" "$changelog"
echo "Promoted Unreleased to [$version] - $date (tag $new_tag, previous ${prev_tag:-none})."
