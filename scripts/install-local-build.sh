#!/usr/bin/env bash
# Builds Kadakas in release mode and puts the build into the version store of the launcher with
# the label `local` and its git commit (PLAT-17). Auto-update starts it when the channel in
# Preferences > Updates is "Local". Use `kadakas-launcher manage` to start it by hand.
#
#   scripts/install-local-build.sh
#
# With another CARGO_TARGET_DIR the build is found there. KADAKAS_LAUNCHER_ROOT chooses another
# install folder (the same variable as for the launcher).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p kadakas-app -p kadakas-modeler -p kadakas-cli -p kadakas-launcher
target="${CARGO_TARGET_DIR:-target}"
if [ "$(uname -s)" = "Darwin" ]; then
  # macOS: a version is a whole app bundle, so the launcher can start it with its name and icon.
  # (In the target folder, so that `add` finds the commit of the build.)
  out="$target/macos-app"
  scripts/macos-app.sh "$target/release" "$out" >/dev/null
  "$target/release/kadakas-launcher" add "$out"
else
  "$target/release/kadakas-launcher" add "$target/release"
fi
