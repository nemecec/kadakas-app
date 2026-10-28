#!/usr/bin/env bash
#
# Prints the top of the notes of a release: the table "Which file do I download?", a sentence
# about the other files and, for a pre-release, a note that the installers are not signed.
# The table is made from the real files in FILES_DIR. A row without its file, or an installer
# that no row names, is an error. The workflow puts the notes of CHANGELOG.md below this text.
#
# Usage: release-notes-table.sh FILES_DIR VERSION [REPO TAG]
#   REPO (owner/name) and TAG make the file names links to the download. Without them the names are plain.
set -euo pipefail
dir="${1:?usage: release-notes-table.sh FILES_DIR VERSION [REPO TAG]}"
version="${2:?usage: release-notes-table.sh FILES_DIR VERSION [REPO TAG]}"
repo="${3:-}"
tag="${4:-}"
[ -d "$dir" ] || { echo "no folder: $dir" >&2; exit 1; }

# label|os-arch|ext
rows=(
  "macOS, Apple silicon (M1 and newer)|macos-arm64|dmg"
  "macOS, Intel|macos-x64|dmg"
  "Windows, installer (exe)|windows-x64|exe"
  "Windows, MSI|windows-x64|msi"
  "Linux, AppImage|linux-x64|AppImage"
  "Linux, deb (Debian and Ubuntu)|linux-x64|deb"
)

named=()
echo "## Which file do I download?"
echo
echo "| System | File |"
echo "|---|---|"
for row in "${rows[@]}"; do
  IFS='|' read -r label osarch ext <<< "$row"
  name="Kadakas-$version-$osarch.$ext"
  [ -f "$dir/$name" ] || { echo "missing release file: $name" >&2; exit 1; }
  named+=("$name")
  if [ -n "$repo" ] && [ -n "$tag" ]; then
    cell="[\`$name\`](https://github.com/$repo/releases/download/$tag/$name)"
  else
    cell="\`$name\`"
  fi
  echo "| $label | $cell |"
done
echo
echo "The files named \`kadakas-update-*\` and \`kadakas-web.tar.gz\` are for the launcher and for hosting. Do not download them."

# Every installer in the folder has a row.
while IFS= read -r f; do
  base="${f##*/}"
  case "$base" in
    *.dmg | *.exe | *.msi | *.AppImage | *.deb)
      found=0
      for n in "${named[@]}"; do if [ "$n" = "$base" ]; then found=1; fi; done
      [ "$found" -eq 1 ] || { echo "installer without a row: $base" >&2; exit 1; }
      ;;
  esac
done < <(find "$dir" -type f | sort)

# The section about unsigned builds stays until the installers are signed (PLAT-19). The signing
# state is the signing-identity in packager-macos.toml (UNSIGNED_NOTE=1 or 0 overrides it).
config="${PACKAGER_MACOS_CONFIG:-$(cd "$(dirname "$0")/../.." && pwd)/packager-macos.toml}"
unsigned=1
if grep -Eq '^[[:space:]]*signing-identity[[:space:]]*=' "$config" 2>/dev/null; then unsigned=0; fi
unsigned="${UNSIGNED_NOTE:-$unsigned}"
if [ "$unsigned" = 1 ]; then
  cat <<'EOT'

## Open an unsigned build

The installers are not signed or notarized yet. The first start shows a warning. The app is safe to open when you got it from this page.

- **macOS 15 and newer.** The message says "Apple could not verify Kadakas is free of malware". Press Done. Open System Settings, Privacy & Security, scroll down and press Open Anyway. macOS asks for your password and shows a second dialog. Press Open Anyway there.
- **macOS 14 and older.** The message says "Kadakas cannot be opened because the developer cannot be verified". Right-click the app, choose Open, and press Open. Right-click does not work on macOS 15.
- **Any macOS.** Run `xattr -cr /Applications/Kadakas.app` in the Terminal. This removes the download flag, and the app opens at once.
- **Windows.** SmartScreen says "Windows protected your PC". Press More info, then Run anyway.
EOT
fi
