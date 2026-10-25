#!/usr/bin/env bash
# Makes the changes of a release in the working tree: the version in every place
# (scripts/set-version.sh) and the promotion of the Unreleased section of CHANGELOG.md.
# prepare-release.yml runs it twice (plan and release), and the result must be the same.
#
#   scripts/apply-release.sh VERSION       ALLOW_EMPTY_CHANGELOG=true allows an empty Unreleased section
set -euo pipefail
cd "$(dirname "$0")/.."
ver="${1:?usage: apply-release.sh VERSION}"
export CHANGELOG_ALLOW_EMPTY="${ALLOW_EMPTY_CHANGELOG:-false}"
# The changelog first: it refuses early, before cargo runs.
.github/scripts/prepare-changelog-release.sh "$ver"
scripts/set-version.sh "$ver"
