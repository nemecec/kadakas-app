#!/usr/bin/env bash
# Tests prepare-changelog-release.sh and extract-changelog-section.sh on temporary copies.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export CHANGELOG_DATE=2026-11-01
fails=0
ok() { echo "ok: $1"; }
bad() { echo "FAIL: $1" >&2; fails=$((fails + 1)); }
expect() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

new_file() { # name
  export CHANGELOG="$tmp/$1.md"
  cat > "$CHANGELOG" <<'EOT'
# Changelog

## [Unreleased]

### Added

- A thing.

[Unreleased]: https://example.org/r/commits/main
EOT
}

new_file first
"$here/prepare-changelog-release.sh" 0.1.0-beta.1 >/dev/null
expect "prerelease section with date" "grep -qx '## \[0.1.0-beta.1\] - 2026-11-01' '$CHANGELOG'"
expect "first release link" "grep -qx '\[0.1.0-beta.1\]: https://example.org/r/releases/tag/v0.1.0-beta.1' '$CHANGELOG'"
expect "unreleased link moved" "grep -qx '\[Unreleased\]: https://example.org/r/compare/v0.1.0-beta.1...HEAD' '$CHANGELOG'"
expect "extract body" "[ \"\$('$here/extract-changelog-section.sh' 0.1.0-beta.1)\" = \"\$(printf '### Added\n\n- A thing.')\" ]"
expect "duplicate refused" "! '$here/prepare-changelog-release.sh' 0.1.0-beta.1 2>/dev/null"
expect "empty Unreleased refused" "! '$here/prepare-changelog-release.sh' 0.1.0-beta.2 2>/dev/null"
expect "refused run leaves the file" "! grep -q 'beta.2' '$CHANGELOG'"
printf '\n' > /dev/null
# Add a change, then promote a second beta and the stable version.
awk '/^## \[Unreleased\]/ { print; print ""; print "### Fixed"; print ""; print "- A fix."; next } { print }' "$CHANGELOG" > "$CHANGELOG.x" && mv "$CHANGELOG.x" "$CHANGELOG"
"$here/prepare-changelog-release.sh" 0.1.0-beta.2 >/dev/null
expect "second beta compares with the first" "grep -qx '\[0.1.0-beta.2\]: https://example.org/r/compare/v0.1.0-beta.1...v0.1.0-beta.2' '$CHANGELOG'"
awk '/^## \[Unreleased\]/ { print; print ""; print "### Changed"; print ""; print "- A change."; next } { print }' "$CHANGELOG" > "$CHANGELOG.x" && mv "$CHANGELOG.x" "$CHANGELOG"
"$here/prepare-changelog-release.sh" 0.1.0 >/dev/null
expect "stable after betas compares with the last beta" "grep -qx '\[0.1.0\]: https://example.org/r/compare/v0.1.0-beta.2...v0.1.0' '$CHANGELOG'"
expect "extract stops at the next section" "[ \"\$('$here/extract-changelog-section.sh' 0.1.0)\" = \"\$(printf '### Changed\n\n- A change.')\" ]"
expect "extract of a missing version fails" "! '$here/extract-changelog-section.sh' 9.9.9 2>/dev/null"
expect "bad version refused" "! '$here/prepare-changelog-release.sh' 1.0 2>/dev/null"

# Only headings under Unreleased is empty. Windows line ends are refused.
export CHANGELOG="$tmp/headings.md"
printf '# Changelog\n\n## [Unreleased]\n\n### Added\n\n### Fixed\n\n[Unreleased]: https://example.org/r/commits/main\n' > "$CHANGELOG"
expect "headings only count as empty" "! '$here/prepare-changelog-release.sh' 0.2.0 2>/dev/null"
expect "allow-empty promotes it" "CHANGELOG_ALLOW_EMPTY=true '$here/prepare-changelog-release.sh' 0.2.0 >/dev/null"
export CHANGELOG="$tmp/crlf.md"
printf '# Changelog\r\n\r\n## [Unreleased]\r\n\r\n- A thing.\r\n\r\n[Unreleased]: https://example.org/r/commits/main\r\n' > "$CHANGELOG"
expect "CRLF refused with a message" "out=\$('$here/prepare-changelog-release.sh' 0.2.0 2>&1 || true); [[ \$out == *CRLF* ]]"
# Backslashes in a link reach the file unchanged.
export CHANGELOG="$tmp/back.md"
printf '# Changelog\n\n## [Unreleased]\n\n- A thing.\n\n[Unreleased]: https://example.org/r\\x/compare/v0.1.0...HEAD\n' > "$CHANGELOG"
"$here/prepare-changelog-release.sh" 0.1.1 >/dev/null
expect "backslash kept" "grep -qF 'https://example.org/r\\x/compare/v0.1.0...v0.1.1' '$CHANGELOG'"

# The real file: it parses, and its Unreleased link is in a form the script reads.
export CHANGELOG="$here/../../CHANGELOG.md"
expect "CHANGELOG.md has an [Unreleased] link" "grep -qE '^\[Unreleased\]: https://' '$CHANGELOG'"
[ "$fails" = 0 ] || { echo "$fails test(s) failed" >&2; exit 1; }
echo "all changelog script tests passed"
