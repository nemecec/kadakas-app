#!/usr/bin/env bash
#
# Gives the installers that cargo-packager wrote one file name pattern:
#   Kadakas-<version>-<os>-<arch>.<ext>
# <version> is the exact workspace version (0.1.0-beta.1). <os> is macos, windows or linux.
# <arch> is arm64 or x64. The version inside the package keeps the form of its format
# (WiX numbers, Debian tilde). Only the file name changes. A dash replaces the tilde in the name
# of the .deb, because GitHub turns a tilde into a dot.
# The update archives (kadakas-update-*.tar.gz) are not touched.
#
# Usage: rename-installers.sh DIR VERSION OS ARCH     (exit 1 when DIR holds no installer)
set -euo pipefail
dir="${1:?usage: rename-installers.sh DIR VERSION OS ARCH}"
version="${2:?usage: rename-installers.sh DIR VERSION OS ARCH}"
os="${3:?usage: rename-installers.sh DIR VERSION OS ARCH}"
arch="${4:?usage: rename-installers.sh DIR VERSION OS ARCH}"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "bad version: $version" >&2; exit 1; }
case "$os" in macos | windows | linux) ;; *) echo "bad os: $os" >&2; exit 1 ;; esac
case "$arch" in arm64 | x64) ;; *) echo "bad arch: $arch" >&2; exit 1 ;; esac
[ -d "$dir" ] || { echo "no folder: $dir" >&2; exit 1; }

count=0
for ext in dmg exe msi AppImage deb; do
  files=()
  while IFS= read -r f; do files+=("$f"); done < <(find "$dir" -maxdepth 1 -type f -name "*.$ext" | sort)
  [ "${#files[@]}" -gt 0 ] || continue
  [ "${#files[@]}" -eq 1 ] || { echo "more than one .$ext file in $dir: ${files[*]}" >&2; exit 1; }
  new="$dir/Kadakas-$version-$os-$arch.$ext"
  if [ "${files[0]}" != "$new" ]; then mv "${files[0]}" "$new"; fi
  echo "${files[0]##*/} -> ${new##*/}"
  count=$((count + 1))
done
[ "$count" -gt 0 ] || { echo "no installer in $dir" >&2; exit 1; }
