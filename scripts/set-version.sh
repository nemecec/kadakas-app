#!/usr/bin/env bash
# Sets the version of Kadakas in every place that carries it.
#
#   scripts/set-version.sh VERSION        for example 0.1.0-beta.1, then 0.1.0
#
# The workspace version in Cargo.toml is the one truth. This script changes:
#   Cargo.toml               [workspace.package] version
#   packager.toml, packager-macos.toml   version
#   python/pyproject.toml, python/kadakas/__init__.py   the PEP 440 form (0.1.0-beta.1 -> 0.1.0b1)
#   Cargo.lock               (cargo update --workspace --offline)
#   THIRD_PARTY_NOTICES.txt  (scripts/notices.sh; the workspace crates are private, so the file has no version, and this only confirms it)
# The workspace crates have path dependencies without a `version`, so nothing else changes.
# The Windows (WiX) and Debian forms are made when the packager runs (scripts/packager-version.sh).
# No network. At the end the script runs scripts/check-version.sh. Commit the changed files.
set -euo pipefail
cd "$(dirname "$0")/.."

ver="${1:?usage: set-version.sh VERSION (for example 0.1.0-beta.1)}"
# The same pattern as the tag check in update-manifest.yml, and the limits of kadakas_update::Version.
re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9a-z]+([.-][0-9a-z]+)*)?$'
[[ "$ver" =~ $re ]] || { echo "bad version: $ver" >&2; exit 1; }
[ "${#ver}" -le 40 ] || { echo "version longer than 40 characters" >&2; exit 1; }
# These fail early when a package form is not possible.
pep=$(scripts/packager-version.sh pep440 "$ver")
scripts/packager-version.sh wix "$ver" >/dev/null
scripts/packager-version.sh deb "$ver" >/dev/null

# Replaces the first line that starts with KEY in FILE (inside SECTION when given).
set_line() { # file section regex replacement
  local file="$1" section="$2" key="$3" new="$4" tmp
  tmp=$(mktemp)
  awk -v section="$section" -v key="$key" -v new="$new" '
    !done && (section == "" || insec) && $0 ~ key { print new; done = 1; next }
    /^\[/ { insec = (section != "" && $0 == section) }
    { print }
    END { if (!done) exit 3 }' "$file" > "$tmp" || { rm -f "$tmp"; echo "no version line in $file" >&2; exit 1; }
  cat "$tmp" > "$file"; rm -f "$tmp"
}

set_line Cargo.toml "[workspace.package]" '^version *=' "version = \"$ver\""
set_line packager.toml "" '^version *=' "version = \"$ver\""
set_line packager-macos.toml "" '^version *=' "version = \"$ver\""
set_line python/pyproject.toml "[project]" '^version *=' "version = \"$pep\""
set_line python/kadakas/__init__.py "" '^__version__ *=' "__version__ = \"$pep\""

cargo update --workspace --offline
if command -v cargo-about >/dev/null 2>&1; then
  scripts/notices.sh
else
  echo "cargo-about is not installed: run scripts/notices.sh yourself (cargo install cargo-about --locked --features cli)" >&2
fi
scripts/check-version.sh
