#!/usr/bin/env bash
# Ad-hoc signing of the macOS bundles (the beta builds have no Developer ID yet, PLAT-19).
#
# Without a seal, macOS says "Kadakas.app is damaged and can't be opened" for a download. The
# programs have only the linker signature, and the bundle seal (Info.plist, resources, nested
# code) is missing. With an ad-hoc seal the bundle is valid. Gatekeeper still refuses it as
# "unidentified developer" (not notarized), and the person opens it on purpose (docs/release.md).
#
#   scripts/macos-sign.sh inner  "dist-app/Kadakas Engine.app"   the inner app (also the update archive)
#   scripts/macos-sign.sh outer  dist/Kadakas.app                 the installed bundle
#   scripts/macos-sign.sh verify dist/Kadakas.app                 codesign --verify --deep --strict
#   scripts/macos-sign.sh hook                                    for before-each-package-command
#
# Order is inside-out, one item at a time, and never with --deep (verify only). The hardened
# runtime is off. Nothing may change the bundle after the last step: `outer` runs through the
# hook of cargo-packager, after the `app` format and before the `dmg` format copies the bundle.
#
# When packager-macos.toml sets a signing-identity, cargo-packager signs and notarizes, and this
# script does nothing (except verify).
set -euo pipefail
cd "$(dirname "$0")/.."

config="${PACKAGER_MACOS_CONFIG:-packager-macos.toml}"
real_identity() { grep -Eq '^[[:space:]]*signing-identity[[:space:]]*=' "$config" 2>/dev/null; }

sign() { codesign --force --sign - --timestamp=none "$@"; }

# Signs every Mach-O file directly in a folder.
sign_programs() {
  local f kind
  for f in "$1"/*; do
    [ -f "$f" ] || continue
    # Not `file | grep -q`: with pipefail, grep ending early makes the pipe fail now and then.
    kind="$(file -b "$f")"
    echo "item: ${f##*/}: $kind"
    # Every executable file is a program. `file` is only the second check.
    if [ -x "$f" ] || [[ "$kind" == *Mach-O* ]]; then
      sign -vvv "$f"
      if ! codesign --verify --strict --verbose=2 "$f"; then
        # Seen on the Intel runner: the sign call succeeded and the check said "not signed at all".
        # Print what is on the file, then take the signature off and sign once more.
        echo "retry: $f" >&2
        ls -l "$f" >&2
        codesign -dvvv "$f" >&2 || true
        codesign --remove-signature "$f" || true
        sleep 2
        sign -vvv "$f"
        codesign --verify --strict --verbose=2 "$f" || { echo "not signed: $f" >&2; return 1; }
      fi
    fi
  done
}

mode="${1:?usage: macos-sign.sh inner|outer|verify|hook [BUNDLE]}"
bundle="${2:-}"
case "$mode" in
  hook)
    # cargo-packager runs this before every format. The outer bundle is ready after `app`.
    if [ "${CARGO_PACKAGER_FORMAT:-}" = "dmg" ]; then
      out="${PACKAGER_OUT_DIR:-dist}"
      exec "$0" outer "$out/Kadakas.app"
    fi
    ;;
  inner)
    [ -d "$bundle" ] || { echo "no bundle: $bundle" >&2; exit 1; }
    if real_identity; then echo "signing-identity is set: cargo-packager signs, skipped"; exit 0; fi
    sign_programs "$bundle/Contents/MacOS"
    sign "$bundle"
    ;;
  outer)
    [ -d "$bundle" ] || { echo "no bundle: $bundle" >&2; exit 1; }
    if real_identity; then echo "signing-identity is set: cargo-packager signs, skipped"; exit 0; fi
    inner="$bundle/Contents/MacOS/Kadakas Engine.app"
    [ -d "$inner" ] || { echo "no inner app: $inner" >&2; exit 1; }
    # The inner app is signed already (macos-app.sh). Check, then sign the loose programs, then the outer bundle.
    codesign --verify --strict "$inner"
    sign_programs "$bundle/Contents/MacOS"
    sign "$bundle"
    codesign --verify --deep --strict --verbose=2 "$bundle"
    codesign -dv "$bundle" 2>&1 | grep -E 'Identifier|Signature|Sealed|Info.plist|flags|CodeDirectory' || true
    ;;
  verify)
    [ -d "$bundle" ] || { echo "no bundle: $bundle" >&2; exit 1; }
    codesign --verify --deep --strict --verbose=2 "$bundle"
    ;;
  *) echo "unknown mode: $mode" >&2; exit 1 ;;
esac
