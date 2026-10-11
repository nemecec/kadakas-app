#!/usr/bin/env bash
# Builds the example plug-ins (plugins/raised-planter and plugins/stone-row) for
# wasm32-unknown-unknown and copies the modules to the test fixtures of kadakas-plugin. Tests use
# the committed .wasm files, so they do not need the wasm target. Run this script after you change
# an example or the plug-in ABI, and commit the new .wasm files.
#
#   rustup target add wasm32-unknown-unknown
#   scripts/build-plugin-fixtures.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# The plug-ins have their own lock files and build folder, apart from the workspace.
target="${CARGO_TARGET_DIR:-$PWD/target}/plugin-fixtures"
fixtures=crates/kadakas-plugin/tests/fixtures
mkdir -p "$fixtures"

# crate folder : module file name
for entry in raised-planter:raised_planter stone-row:stone_row; do
  dir="${entry%%:*}"
  name="${entry##*:}"
  CARGO_TARGET_DIR="$target" cargo build --release --locked \
    --manifest-path "plugins/$dir/Cargo.toml" --target wasm32-unknown-unknown
  cp "$target/wasm32-unknown-unknown/release/$name.wasm" "$fixtures/$name.wasm"
  ls -l "$fixtures/$name.wasm"
done
