#!/usr/bin/env bash
# Checks that every place that carries the version of Kadakas has the workspace version.
# The workspace version (Cargo.toml, [workspace.package]) is the truth. Change it with scripts/set-version.sh.
# Needs only bash and awk. No cargo, no network. CI runs it (job `version`), and release.yml runs it.
#
#   scripts/check-version.sh
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/python.sh

bad=0
fail() { echo "version: $*" >&2; bad=1; }

# Value of the first line "KEY = "value"" (inside SECTION when given).
value_of() { # file section key
  awk -v section="$2" -v key="$3" '
    /^\[/ { insec = (section != "" && $0 == section); if (section != "") next }
    (section == "" || insec) && $0 ~ "^" key " *= *\"" { v = $0; sub(/^[^"]*"/, "", v); sub(/".*$/, "", v); print v; exit }' "$1"
}

ws=$(value_of Cargo.toml "[workspace.package]" version)
[ -n "$ws" ] || { echo "version: no workspace version in Cargo.toml" >&2; exit 1; }
echo "workspace version: $ws"

# The version must be a tag the release workflows accept, and one that kadakas_update::Version reads.
"$PY" -I scripts/version_cmp.py valid "$ws" || fail "$ws is not a version that the release workflows accept"

# Package forms. An error here means the pre-release label cannot be mapped.
pep=$(scripts/packager-version.sh pep440 "$ws") || { fail "no PEP 440 form of $ws"; pep=""; }
scripts/packager-version.sh wix "$ws" >/dev/null || fail "no WiX (MSI) form of $ws"
scripts/packager-version.sh deb "$ws" >/dev/null || fail "no Debian form of $ws"

want() { # place found expected
  [ "$2" = "$3" ] || fail "$1 has \"$2\", expected \"$3\""
}
want "packager.toml" "$(value_of packager.toml "" version)" "$ws"
want "packager-macos.toml" "$(value_of packager-macos.toml "" version)" "$ws"
if [ -n "$pep" ]; then
  want "python/pyproject.toml" "$(value_of python/pyproject.toml "[project]" version)" "$pep"
  want "python/kadakas/__init__.py" "$(sed -n 's/^__version__ *= *"\(.*\)"$/\1/p' python/kadakas/__init__.py | head -1)" "$pep"
fi

# Every crate takes the workspace version, and a path dependency has no other version.
for f in $(git ls-files '*Cargo.toml'); do
  # The plug-ins are separate packages with their own version (their manifests, not the app).
  case "$f" in Cargo.toml|plugins/*) continue ;; esac
  if grep -Eq '^version *=' "$f"; then fail "$f has its own version (use version.workspace = true)"; fi
  if grep -E 'path *= *"' "$f" | grep -E 'version *= *"' | grep -vq "version = \"=\?$ws\""; then
    fail "$f has a path dependency with another version requirement"
  fi
done

# Cargo.lock: every workspace crate (an entry without a source) has the workspace version.
lock=$(awk -v ws="$ws" '
  /^\[\[package\]\]/ { name = ""; ver = ""; src = 0; next }
  /^name = / { name = $3; gsub(/"/, "", name) }
  /^version = / { ver = $3; gsub(/"/, "", ver) }
  /^source = / { src = 1 }
  /^$/ { if (name != "" && !src && ver != ws) print name " " ver; name = "" }
  END { if (name != "" && !src && ver != ws) print name " " ver }' Cargo.lock)
if [ -n "$lock" ]; then fail "Cargo.lock has other versions (run cargo update --workspace --offline): $(echo "$lock" | head -3 | tr '\n' ';')"; fi

# THIRD_PARTY_NOTICES.txt lists no workspace crate (they are private: about.toml ignores them), so it has no version.
if grep -Eq '^  - kadakas[a-z0-9-]* [0-9]' THIRD_PARTY_NOTICES.txt; then
  fail "THIRD_PARTY_NOTICES.txt lists a workspace crate with a version (run scripts/notices.sh)"
fi

# The macOS bundle: Info.plist templates take the version from macos-app.sh, never a fixed one.
for plist in packaging/macos/App-Info.plist packaging/macos/Info.plist; do
  if grep -Eq '<string>[0-9]+\.[0-9]+\.[0-9]+[^<]*</string>' "$plist"; then
    fail "$plist has a fixed version (use @VERSION@ and @BUILD@)"
  fi
done

if [ "$bad" = 0 ]; then echo "version: all places agree ($ws; python $pep; msi $(scripts/packager-version.sh wix "$ws"); deb $(scripts/packager-version.sh deb "$ws"))"; fi
exit "$bad"
