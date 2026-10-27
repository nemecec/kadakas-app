#!/usr/bin/env bash
# Makes the app bundle `Kadakas Engine.app` of the macOS release (PLAT-16): the app itself, the inner
# bundle of the installed outer bundle (which has the launcher), and the update archive.
#
#   scripts/macos-app.sh RELEASE_DIR OUT_DIR [VERSION]
#
# RELEASE_DIR has the programs (`kadakas-app`, `kadakas-modeler`, `kadakas`), for example target/release.
# OUT_DIR/Kadakas Engine.app is made there. VERSION defaults to the version of the workspace.
#
#   Kadakas Engine.app/Contents/Info.plist            packaging/macos/App-Info.plist
#   Info.plist (for the outer bundle)                packaging/macos/Info.plist
#   Kadakas Engine.app/Contents/MacOS/kadakas-app     the main executable
#   Kadakas Engine.app/Contents/MacOS/kadakas-modeler the 3D modeler (a UI flavor of the launcher)
#   Kadakas Engine.app/Contents/MacOS/kadakas         the command line tool (MCP server, renderer)
#   Kadakas Engine.app/Contents/Resources/            icon and license texts
#
# The launcher is not in it: it is the main executable of the outer bundle, which only the
# installer changes. The update archive is this bundle as a `.tar.gz`:
#
#   COPYFILE_DISABLE=1 tar -czf kadakas-update-macos-aarch64.tar.gz -C OUT_DIR "Kadakas Engine.app"
set -euo pipefail
cd "$(dirname "$0")/.."

src="${1:?usage: macos-app.sh RELEASE_DIR OUT_DIR [VERSION]}"
out="${2:?usage: macos-app.sh RELEASE_DIR OUT_DIR [VERSION]}"
version="${3:-$(awk '/^\[workspace.package\]/{f=1;next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' Cargo.toml)}"
[ -n "$version" ] || { echo "no version" >&2; exit 1; }

app="$out/Kadakas Engine.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$src/kadakas-app" "$app/Contents/MacOS/kadakas-app"
install -m 755 "$src/kadakas" "$app/Contents/MacOS/kadakas"
# The 3D modeler (PLAT-11) is a UI flavor next to the app: the launcher finds it in the bundle.
install -m 755 "$src/kadakas-modeler" "$app/Contents/MacOS/kadakas-modeler"
cp packaging/icons/icon.icns "$app/Contents/Resources/icon.icns"
# The license texts go with every program that is handed out, an update too.
for f in THIRD_PARTY_NOTICES.txt docs/attribution.md LICENSE-APACHE LICENSE-MIT; do
  if [ -f "$f" ]; then cp "$f" "$app/Contents/Resources/"; fi
done
# CFBundleShortVersionString is digits and dots only (Apple): the version without the pre-release
# part. CFBundleVersion is a build number, also digits and dots (the date and time, like the one
# that cargo-packager writes; BUILD_NUMBER sets it). The full version is in the key KadakasVersion.
build="${BUILD_NUMBER:-$(date -u +%Y%m%d.%H%M%S)}"
[[ "$build" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] || { echo "BUILD_NUMBER must be digits and dots: $build" >&2; exit 1; }
sed_version=(-e "s/@VERSION@/$version/g" -e "s/@CORE@/${version%%-*}/g" -e "s/@BUILD@/$build/g")
sed "${sed_version[@]}" packaging/macos/App-Info.plist > "$app/Contents/Info.plist"
# The Info.plist for the outer bundle (packager-macos.toml: info-plist-path).
sed "${sed_version[@]}" packaging/macos/Info.plist > "$out/Info.plist"
# Check that the plist is valid (plutil is part of macOS).
if command -v plutil >/dev/null 2>&1; then plutil -lint "$app/Contents/Info.plist" "$out/Info.plist" >/dev/null; fi
# Ad-hoc seal, inside-out (scripts/macos-sign.sh). The update archive is made after this, and the
# packager copies the signed bundle into the outer one. Do not change a file in it after this line.
if command -v codesign >/dev/null 2>&1; then scripts/macos-sign.sh inner "$app"; fi
echo "$app (version $version)"
