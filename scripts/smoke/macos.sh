#!/usr/bin/env bash
# Smoke test of the macOS dmg (PLAT-28): mount it, copy the app to the applications folder,
# check the bundle, start it the first time, start it again, ping the app, remove the app.
#
#   scripts/smoke/macos.sh PACKAGE.dmg VERSION [ARTIFACTS_DIR]
#
# VERSION is the version of the app (0.1.0-beta.1). KADAKAS_SMOKE_APPLICATIONS sets the folder
# that stands for /Applications (the default is /Applications; on your own Mac use a temporary
# folder, so that nothing is installed).
set -euo pipefail
# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

dmg="${1:?usage: macos.sh PACKAGE.dmg VERSION [ARTIFACTS_DIR]}"
version="${2:?usage: macos.sh PACKAGE.dmg VERSION [ARTIFACTS_DIR]}"
artifacts="${3:-}"
[ -f "$dmg" ] || fail "no package $dmg"
dmg="$(cd "$(dirname "$dmg")" && pwd)/$(basename "$dmg")"
apps="${KADAKAS_SMOKE_APPLICATIONS:-/Applications}"
mkdir -p "$apps"

work="$(mktemp -d "${TMPDIR:-/tmp}/kadakas-smoke.XXXXXX")"
mnt="$work/mnt"
mounted=""
installed=""
cleanup() {
  # A failed run must not leave the app in the applications folder.
  if [ -n "$installed" ]; then rm -rf "$installed"; fi
  if [ -n "$mounted" ]; then hdiutil detach "$mnt" -force >/dev/null 2>&1 || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

step "mount $dmg"
mkdir -p "$mnt"
# The dmg carries the license text (license-file in packager-macos.toml): hdiutil asks to agree.
printf 'Y\n' | hdiutil attach -nobrowse -readonly -noverify -mountpoint "$mnt" "$dmg" >/dev/null
mounted=1
src="$(find "$mnt" -maxdepth 1 -name '*.app' | head -1)"
[ -n "$src" ] || fail "the dmg has no .app at its top"
[ -e "$mnt/Applications" ] || warn "the dmg has no link to the Applications folder"
name="$(basename "$src")"

step "copy $name to $apps"
# The run replaces and then removes the app. Never over a real install of a person.
if [ -e "$apps/$name" ] && [ -z "${CI:-}" ]; then
  fail "$apps/$name exists. This script removes it at the end. Set KADAKAS_SMOKE_APPLICATIONS to a temporary folder, or CI=1."
fi
rm -rf "${apps:?}/$name"
installed="$apps/$name"
ditto "$src" "$installed"
hdiutil detach "$mnt" >/dev/null 2>&1 || hdiutil detach "$mnt" -force >/dev/null
mounted=""

# The seal of the installed bundle must be valid (a broken seal shows "damaged" to a person).
step "codesign --verify --deep --strict"
codesign --verify --deep --strict --verbose=2 "$installed" || fail "the installed bundle has an invalid code signature"

# What Gatekeeper says about the bundle. This documents today's expected answer for an ad-hoc
# signed beta build: "rejected" (the source is unnotarized or the developer is unidentified). The
# message must not say that the bundle is broken ("invalid", "damaged", "a sealed resource is
# missing", "code has no resources"): that is the "Kadakas.app is damaged" dialog. spctl does not
# read the quarantine flag, so the answer is the same with or without it. The test still assesses a
# copy that carries the flag, as a browser download does, and never touches the installed bundle.
# With notarization (PLAT-19) the expected answer changes to "accepted".
step "spctl on a quarantined copy"
qcopy="$work/quarantine/$name"
mkdir -p "$work/quarantine"
ditto "$installed" "$qcopy"
xattr -w com.apple.quarantine "0181;$(printf %x "$(date +%s)");Safari;" "$qcopy"
spctl_out="$(spctl --assess --type execute -vv "$qcopy" 2>&1 || true)"
echo "$spctl_out"
printf '%s\n' "$spctl_out" | grep -Eq ': (rejected|accepted)$' || fail "spctl gave no clear answer (rejected or accepted): $spctl_out"
if printf '%s' "$spctl_out" | grep -Eiq 'invalid|damaged|a sealed resource is missing|code has no resources'; then
  fail "Gatekeeper calls the bundle broken: $spctl_out"
fi
rm -rf "$work/quarantine"

plist="$installed/Contents/Info.plist"
need_file "$plist" "Info.plist"
plutil -lint "$plist" >/dev/null || fail "Info.plist is not valid"
exe_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist")"
launcher="$installed/Contents/MacOS/$exe_name"
need_exec "$launcher" "main executable"
[ "$exe_name" = "kadakas-launcher" ] || fail "the main executable is $exe_name, expected kadakas-launcher"
inner="$installed/Contents/MacOS/Kadakas Engine.app"
need_exec "$inner/Contents/MacOS/kadakas-app" "app"
need_exec "$inner/Contents/MacOS/kadakas-modeler" "modeler"
need_exec "$installed/Contents/MacOS/kadakas" "command line tool"
need_file "$inner/Contents/Info.plist" "inner Info.plist"
# The menu entry on macOS is the app in the applications folder: a name, an icon and the .kad type.
/usr/libexec/PlistBuddy -c 'Print :CFBundleName' "$plist" >/dev/null || fail "Info.plist has no CFBundleName"
icon="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$plist" 2>/dev/null || true)"
[ -n "$icon" ] || fail "Info.plist names no icon"
case "$icon" in *.icns) ;; *) icon="$icon.icns" ;; esac
need_file "$installed/Contents/Resources/$icon" "icon"
/usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes' "$plist" >/dev/null || fail "Info.plist declares no document type (.kad)"
for lic in LICENSE-APACHE LICENSE-MIT THIRD_PARTY_NOTICES.txt; do
  need_file "$installed/Contents/Resources/$lic" "license text"
done
[ -n "$("$installed/Contents/MacOS/kadakas" --version)" ] || fail "kadakas --version prints nothing"

run_smoke "$launcher" "$version" "$artifacts" "screencapture -x {out}" "$work/run"

# The program ran three times. The seal must still be valid: a write into the bundle breaks it.
step "codesign --verify --deep --strict after the runs"
codesign --verify --deep --strict --verbose=2 "$installed" || fail "the seal of the bundle broke while the program ran"

step "remove $installed"
rm -rf "$installed"
gone "$installed"
installed=""
step "OK (dmg)"
