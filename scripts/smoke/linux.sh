#!/usr/bin/env bash
# Smoke test of a Linux package (PLAT-28): install, check the files and the menu entry, start it
# the first time, start it again, ping the app, uninstall.
#
#   scripts/smoke/linux.sh deb      PACKAGE.deb      VERSION [ARTIFACTS_DIR]
#   scripts/smoke/linux.sh appimage PACKAGE.AppImage VERSION [ARTIFACTS_DIR]
#
# VERSION is the version of the app (0.1.0-beta.1), not the Debian version (0.1.0~beta.1).
# It needs a display. In CI: `xvfb-run -a scripts/smoke/linux.sh ...` with Mesa software
# rendering (packages xvfb mesa-vulkan-drivers libgl1-mesa-dri imagemagick). The deb path
# installs with sudo and removes the package at the end. On your own machine use a virtual
# machine or a container for it. The AppImage path installs nothing: it extracts the image into a
# work folder. KADAKAS_SMOKE_SUDO overrides the command for root rights (empty: none).
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

kind="${1:?usage: linux.sh deb|appimage PACKAGE VERSION [ARTIFACTS_DIR]}"
package="${2:?usage: linux.sh deb|appimage PACKAGE VERSION [ARTIFACTS_DIR]}"
version="${3:?usage: linux.sh deb|appimage PACKAGE VERSION [ARTIFACTS_DIR]}"
artifacts="${4:-}"
[ -f "$package" ] || fail "no package $package"
package="$(cd "$(dirname "$package")" && pwd)/$(basename "$package")"
[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || fail "no display: run this under xvfb-run -a"

if [ "${KADAKAS_SMOKE_SUDO+set}" = set ]; then SUDO="$KADAKAS_SMOKE_SUDO"
elif [ "$(id -u)" = 0 ]; then SUDO=""
else SUDO="sudo"; fi

work="$(mktemp -d "${TMPDIR:-/tmp}/kadakas-smoke.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

shot=""
if command -v import >/dev/null 2>&1; then shot="import -window root {out}"
elif command -v scrot >/dev/null 2>&1; then shot="scrot -o {out}"
else warn "no screenshot tool (import or scrot): no screenshots"; fi

# check_desktop FILE: a menu entry that starts the launcher and has a name and an icon.
check_desktop() {
  local f="$1"
  need_file "$f" "menu entry"
  grep -q '^Type=Application' "$f" || fail "$f has no Type=Application"
  grep -q '^Name=Kadakas' "$f" || fail "$f has no Name=Kadakas"
  grep -q '^Exec=.*kadakas-launcher' "$f" || fail "$f does not start kadakas-launcher: $(grep '^Exec=' "$f" || true)"
  grep -q '^Icon=.' "$f" || fail "$f has no icon"
  if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$f" || warn "desktop-file-validate complains about $f"
  fi
}

case "$kind" in
  deb)
    command -v dpkg >/dev/null 2>&1 || fail "dpkg is needed"
    name="$(dpkg-deb -f "$package" Package)"
    step "install $package (package $name)"
    # apt installs the dependencies of the package too.
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "$package"
    dpkg -L "$name" > "$work/files.txt"
    for b in kadakas-launcher kadakas-app kadakas-modeler kadakas; do
      need_exec "/usr/bin/$b" "program"
    done
    desktop="$(grep -E '^/usr/share/applications/.*\.desktop$' "$work/files.txt" | head -1 || true)"
    [ -n "$desktop" ] || fail "the package has no menu entry in /usr/share/applications"
    check_desktop "$desktop"
    icon="$(grep -E '^/usr/share/icons/.*\.png$' "$work/files.txt" | head -1 || true)"
    [ -n "$icon" ] || fail "the package has no icon"
    need_file "$icon" "icon"
    for lic in LICENSE-APACHE LICENSE-MIT THIRD_PARTY_NOTICES.txt; do
      grep -q "/$lic\$" "$work/files.txt" || fail "the package does not ship $lic"
    done
    [ -n "$(/usr/bin/kadakas --version)" ] || fail "kadakas --version prints nothing"
    run_smoke /usr/bin/kadakas-launcher "$version" "$artifacts" "$shot" "$work/run"
    step "uninstall $name"
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get remove -y -qq "$name"
    while IFS= read -r f; do
      case "$f" in /usr/bin/* | /usr/share/applications/*) [ -d "$f" ] || gone "$f" ;; esac
    done < "$work/files.txt"
    ;;
  appimage)
    chmod +x "$package"
    step "extract $package"
    mkdir -p "$work/x"
    (cd "$work/x" && "$package" --appimage-extract >/dev/null) || fail "the AppImage does not extract"
    root="$work/x/squashfs-root"
    for b in kadakas-launcher kadakas-app kadakas-modeler kadakas; do
      need_exec "$root/usr/bin/$b" "program"
    done
    desktop="$(find "$root" -maxdepth 4 -name '*.desktop' -not -path '*/proc/*' | head -1 || true)"
    [ -n "$desktop" ] || fail "the AppImage has no .desktop file"
    check_desktop "$desktop"
    ls "$root"/*.png "$root"/usr/share/icons/hicolor/*/apps/*.png >/dev/null 2>&1 || fail "the AppImage has no icon"
    [ -e "$root/AppRun" ] || fail "the AppImage has no AppRun"
    # Run the image itself, without FUSE.
    export APPIMAGE_EXTRACT_AND_RUN=1
    run_smoke "$package" "$version" "$artifacts" "$shot" "$work/run"
    step "remove the extracted files"
    rm -rf "$work/x"
    gone "$work/x"
    ;;
  *) fail "unknown kind $kind (deb or appimage)" ;;
esac
step "OK ($kind)"
