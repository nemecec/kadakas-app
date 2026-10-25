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
#   THIRD_PARTY_NOTICES.txt  (scripts/notices.sh; the workspace crates are private, so the file has no
#                             version and this only confirms it; needs cargo-about)
# The workspace crates have path dependencies without a `version`, so nothing else changes.
# The Windows (WiX) and Debian forms are made when the packager runs (scripts/packager-version.sh).
# Only the labels alpha.N, beta.N, rc.N and dev can be mapped to every package form. Others are refused.
#
# Needs the crates index for `cargo update --offline`: run `cargo fetch` first (a fresh checkout).
# Nothing else needs the network. If any step fails, all files are put back as they were.
# At the end the script runs scripts/check-version.sh. Commit the changed files.
set -euo pipefail
cd "$(dirname "$0")/.."

ver="${1:?usage: set-version.sh VERSION (for example 0.1.0-beta.1)}"
python3 -I scripts/version_cmp.py valid "$ver" || exit 1
# These fail early when a package form is not possible.
pep=$(scripts/packager-version.sh pep440 "$ver")
scripts/packager-version.sh wix "$ver" >/dev/null
scripts/packager-version.sh deb "$ver" >/dev/null

files=(Cargo.toml packager.toml packager-macos.toml python/pyproject.toml python/kadakas/__init__.py Cargo.lock THIRD_PARTY_NOTICES.txt)
backup=$(mktemp -d)
for f in "${files[@]}"; do mkdir -p "$backup/$(dirname "$f")"; cp -p "$f" "$backup/$f"; done
restore() {
  local status=$?
  if [ "$status" != 0 ]; then
    for f in "${files[@]}"; do cp -p "$backup/$f" "$f"; done
    echo "set-version: failed, all files are as they were" >&2
  fi
  rm -rf "$backup"
  exit "$status"
}
trap restore EXIT

# Replaces the first line that starts with KEY in FILE (inside SECTION when given).
set_line() { # file section regex replacement
  local file="$1" section="$2" key="$3" new="$4" tmp
  tmp=$(mktemp)
  NEW="$new" awk -v section="$section" -v key="$key" '
    !done && (section == "" || insec) && $0 ~ key { print ENVIRON["NEW"]; done = 1; next }
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
