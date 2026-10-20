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
"$target/release/kadakas-launcher" add "$target/release"
