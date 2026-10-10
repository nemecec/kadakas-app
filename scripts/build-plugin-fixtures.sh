#!/usr/bin/env bash
# Builds the example plug-in (plugins/raised-planter) for wasm32-unknown-unknown and copies the
# module to the test fixtures of kadakas-plugin. Tests use the committed .wasm file, so they do
# not need the wasm target. Run this script after you change the example or the plug-in ABI,
# and commit the new .wasm file.
#
#   rustup target add wasm32-unknown-unknown
#   scripts/build-plugin-fixtures.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# The plug-in has its own lock file and build folder, apart from the workspace.
target="${CARGO_TARGET_DIR:-$PWD/target}/plugin-fixtures"
fixtures=crates/kadakas-plugin/tests/fixtures
mkdir -p "$fixtures"

CARGO_TARGET_DIR="$target" cargo build --release --locked \
  --manifest-path plugins/raised-planter/Cargo.toml --target wasm32-unknown-unknown
cp "$target/wasm32-unknown-unknown/release/raised_planter.wasm" "$fixtures/raised_planter.wasm"
ls -l "$fixtures/raised_planter.wasm"
