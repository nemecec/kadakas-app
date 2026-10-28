#!/usr/bin/env bash
# Signing of the macOS bundles.
#
# Without a seal, macOS says "Kadakas.app is damaged and can't be opened" for a download. The
# programs have only the linker signature, and the bundle seal (Info.plist, resources, nested
# code) is missing. So every bundle is signed, inside-out, one item at a time, never with --deep
# (verify only). The beta builds have no Developer ID yet (PLAT-19): they get an ad-hoc seal. The
# bundle is then valid, and Gatekeeper refuses it only as not notarized (docs/release.md).
#
#   scripts/macos-sign.sh inner  "dist-app/Kadakas Engine.app"   the inner app (also the update archive)
#   scripts/macos-sign.sh outer  dist/Kadakas.app                 the installed bundle
#   scripts/macos-sign.sh verify dist/Kadakas.app                 codesign --verify --deep --strict
#   scripts/macos-sign.sh identity                               prints the signing identity of the config (empty: ad-hoc)
#   scripts/macos-sign.sh hook                                    for before-each-package-command
#
# With a `signing-identity` in packager-macos.toml the items are signed with that Developer ID
# (hardened runtime, secure timestamp). cargo-packager 0.11.8 leaves embedded apps out of its own
# signing, so `inner` must sign the inner app first. The packager then signs the outer bundle
# itself, and `outer` only verifies it. Notarization is not done here (PLAT-19).
# Without a signing identity the hardened runtime is off and there is no timestamp.
#
# Nothing may change a bundle after its last step. `outer` runs through the hook of cargo-packager,
# after the format `app` and before the format `dmg` copies the bundle. So the formats have to be
# `app,dmg` in this order. The hook refuses any other combination.
#
# KADAKAS_SIGN_DRY_RUN=1: the hook checks its arguments and prints what it would do (for the test).
set -euo pipefail

# Paths of the bundles are taken from where the caller stands, then the script goes to the repo root.
bundle="${2:-}"
if [ -n "$bundle" ]; then case "$bundle" in /*) ;; *) bundle="$PWD/$bundle" ;; esac; fi
cd "$(dirname "$0")/.."

config="${PACKAGER_MACOS_CONFIG:-packager-macos.toml}"
identity() { sed -n -E 's/^[[:space:]]*signing-identity[[:space:]]*=[[:space:]]*"([^"]*)".*/\1/p' "$config" 2>/dev/null | head -1; }
ident="$(identity)"

sign() {
  local out
  if [ -n "$ident" ]; then
    out="$(codesign --force --sign "$ident" --options runtime --timestamp "$@" 2>&1)" || { echo "$out" >&2; return 1; }
  else
    out="$(codesign --force --sign - --timestamp=none "$@" 2>&1)" || { echo "$out" >&2; return 1; }
  fi
}

# Signs every Mach-O file directly in Contents/MacOS of a bundle, except the main executable.
# codesign on a main executable signs the whole bundle, and it needs the other programs signed
# first. The signature of the main executable comes with the signature of the bundle.
# Another file there fails the run: it would break the seal.
sign_programs() {
  local f kind main
  main="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$1/Contents/Info.plist")"
  for f in "$1"/Contents/MacOS/*; do
    [ -f "$f" ] || continue
    [ "${f##*/}" != "$main" ] || continue
    # Not `file | grep -q`: with pipefail, grep ending early makes the pipe fail now and then.
    kind="$(file -b "$f")"
    case "$kind" in
      *Mach-O*)
        sign "$f"
        codesign --verify --strict "$f" || { echo "not signed: $f" >&2; return 1; }
        echo "signed: ${f##*/}"
        ;;
      *) echo "not a Mach-O file in Contents/MacOS: ${f##*/}. It would break the seal. Move it to Resources." >&2; return 1 ;;
    esac
  done
}

need_bundle() { [ -d "$bundle" ] || { echo "no bundle: $bundle" >&2; exit 1; }; }

mode="${1:?usage: macos-sign.sh inner|outer|verify|hook [BUNDLE]}"
case "$mode" in
  hook)
    # cargo-packager runs this before every format. The outer bundle is ready after `app`.
    format="${CARGO_PACKAGER_FORMAT:-}"
    formats=",${CARGO_PACKAGER_FORMATS:-},"
    [ -n "$format" ] || { echo "macos-sign.sh hook: CARGO_PACKAGER_FORMAT is empty. Only cargo-packager runs the hook." >&2; exit 1; }
    case "$format" in
      app)
        case "$formats" in
          *,dmg,*) ;;
          *) echo "macos-sign.sh: the format app alone leaves the bundle unsealed. Use --formats app,dmg, or run scripts/macos-sign.sh outer dist/Kadakas.app afterwards." >&2; exit 1 ;;
        esac
        ;;
      dmg)
        case "$formats" in
          *,app,*dmg,*) ;;
          *) echo "macos-sign.sh: the format dmg needs app before it (--formats app,dmg). Else a bundle that is left in dist/ would be sealed and then rebuilt unsealed." >&2; exit 1 ;;
        esac
        if [ "${KADAKAS_SIGN_DRY_RUN:-}" = 1 ]; then echo "would run: outer dist/Kadakas.app"; exit 0; fi
        exec "$0" outer "$PWD/dist/Kadakas.app"
        ;;
    esac
    ;;
  inner)
    need_bundle
    sign_programs "$bundle"
    sign "$bundle"
    codesign --verify --strict "$bundle"
    echo "signed: ${bundle##*/} (${ident:+Developer ID, }${ident:-ad-hoc})"
    ;;
  outer)
    need_bundle
    inner="$bundle/Contents/MacOS/Kadakas Engine.app"
    [ -d "$inner" ] || { echo "no inner app: $inner" >&2; exit 1; }
    # The inner app is signed already (macos-app.sh). Check it first.
    codesign --verify --strict "$inner"
    if [ -n "$ident" ]; then
      echo "signing-identity is set: cargo-packager signed the outer bundle, only verified here"
    else
      sign_programs "$bundle"
      sign "$bundle"
    fi
    codesign --verify --deep --strict --verbose=2 "$bundle"
    codesign -dv "$bundle" 2>&1 | grep -E 'Identifier|Signature|Sealed|Info.plist|flags|CodeDirectory' || true
    ;;
  identity) echo "$ident" ;;
  verify)
    need_bundle
    codesign --verify --deep --strict --verbose=2 "$bundle"
    ;;
  *) echo "unknown mode: $mode" >&2; exit 1 ;;
esac
