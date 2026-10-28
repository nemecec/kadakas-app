#!/usr/bin/env bash
# Tests the argument handling of scripts/macos-sign.sh (the hook and the signing identity).
# No codesign: the hook runs with KADAKAS_SIGN_DRY_RUN=1.
set -uo pipefail
here="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
ok() { echo "ok: $1"; }
bad() { echo "FAIL: $1" >&2; fails=$((fails + 1)); }
hook() { # FORMAT FORMATS -> sets out and rc
  out="$(KADAKAS_SIGN_DRY_RUN=1 CARGO_PACKAGER_FORMAT="$1" CARGO_PACKAGER_FORMATS="$2" "$here/scripts/macos-sign.sh" hook 2>&1)"; rc=$?
}
expect_ok() { # label format formats text
  hook "$2" "$3"
  if [ "$rc" -eq 0 ] && [[ "$out" == *"$4"* ]]; then ok "$1"; else bad "$1 (rc=$rc, out=$out)"; fi
}
expect_fail() { # label format formats text
  hook "$2" "$3"
  if [ "$rc" -ne 0 ] && [[ "$out" == *"$4"* ]]; then ok "$1"; else bad "$1 (rc=$rc, out=$out)"; fi
}
expect_ok "app,dmg: dmg signs the outer bundle" dmg "app,dmg" "would run: outer dist/Kadakas.app"
expect_ok "app,dmg: the hook before app does nothing" app "app,dmg" ""
expect_fail "dmg alone is refused" dmg "dmg" "needs app before it"
expect_fail "dmg before app is refused" dmg "dmg,app" "needs app before it"
expect_fail "app alone is refused" app "app" "leaves the bundle unsealed"
expect_fail "no format is refused" "" "" "Only cargo-packager"
expect_ok "other formats are not touched" nsis "nsis" ""

# The signing identity is read from the config.
cfg="$tmp/p.toml"
printf '[macos]\n# signing-identity = "Commented"\nminimum-system-version = "11.0"\n' > "$cfg"
id_of() { PACKAGER_MACOS_CONFIG="$1" "$here/scripts/macos-sign.sh" identity; }
[ -z "$(id_of "$cfg")" ] && ok "a commented identity is no identity" || bad "a commented identity is no identity"
printf '[macos]\nsigning-identity = "Developer ID Application: X (ABC)"\n' > "$cfg"
[ "$(id_of "$cfg")" = "Developer ID Application: X (ABC)" ] && ok "identity is read" || bad "identity is read"
[ -z "$(id_of "$here/packager-macos.toml")" ] && ok "packager-macos.toml has no identity yet" || bad "packager-macos.toml has an identity"

[ "$fails" -eq 0 ] || { echo "$fails failed" >&2; exit 1; }
echo "all passed"
