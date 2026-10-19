#!/usr/bin/env bash
# Makes the app bundle `Kadakas.app` of the macOS release (PLAT-16): the app itself, the inner
# bundle of the installed outer bundle (which has the launcher), and the update archive.
#
#   scripts/macos-app.sh RELEASE_DIR OUT_DIR [VERSION]
#
# RELEASE_DIR has the programs (`kadakas-app`, `kadakas`), for example target/release.
# OUT_DIR/Kadakas.app is made there. VERSION defaults to the version of the workspace.
#
#   Kadakas.app/Contents/Info.plist            packaging/macos/App-Info.plist
#   Kadakas.app/Contents/MacOS/kadakas-app     the main executable
#   Kadakas.app/Contents/MacOS/kadakas         the command line tool (MCP server, renderer)
#   Kadakas.app/Contents/Resources/            icon and license texts
#
# The launcher is not in it: it is the main executable of the outer bundle, which only the
# installer changes. The update archive is this bundle as a `.tar.gz`:
#
#   COPYFILE_DISABLE=1 tar -czf kadakas-update-macos-aarch64.tar.gz -C OUT_DIR Kadakas.app
set -euo pipefail
cd "$(dirname "$0")/.."

src="${1:?usage: macos-app.sh RELEASE_DIR OUT_DIR [VERSION]}"
out="${2:?usage: macos-app.sh RELEASE_DIR OUT_DIR [VERSION]}"
version="${3:-$(awk '/^\[workspace.package\]/{f=1;next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' Cargo.toml)}"
[ -n "$version" ] || { echo "no version" >&2; exit 1; }

app="$out/Kadakas.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$src/kadakas-app" "$app/Contents/MacOS/kadakas-app"
install -m 755 "$src/kadakas" "$app/Contents/MacOS/kadakas"
cp packaging/icons/icon.icns "$app/Contents/Resources/icon.icns"
# The license texts go with every program that is handed out, an update too.
for f in THIRD_PARTY_NOTICES.txt docs/attribution.md LICENSE-APACHE LICENSE-MIT; do
  if [ -f "$f" ]; then cp "$f" "$app/Contents/Resources/"; fi
done
sed "s/@VERSION@/$version/g" packaging/macos/App-Info.plist > "$app/Contents/Info.plist"
# Check that the plist is valid (plutil is part of macOS).
if command -v plutil >/dev/null 2>&1; then plutil -lint "$app/Contents/Info.plist" >/dev/null; fi
echo "$app (version $version)"
