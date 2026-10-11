#!/usr/bin/env bash
# Builds the example plug-ins (plugins/raised-planter and plugins/stone-row) for
# wasm32-unknown-unknown and copies the modules to the test fixtures of kadakas-plugin. Tests use
# the committed .wasm files, so they do not need the wasm target. Run this script after you change
# an example or the plug-in ABI, and commit the new .wasm files.
#
#   rustup target add wasm32-unknown-unknown
#   scripts/build-plugin-fixtures.sh            build and copy the modules
#   scripts/build-plugin-fixtures.sh --check    build and compare with the committed modules
#
# The modules must not hold the path of the person who built them (a home folder with a user
# name). The script remaps the folder of the repository and the home folder to fixed names, and
# it fails if a module still holds a path of a home folder. The build is reproducible: the same
# source and the same compiler give the same bytes on any computer.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=build
if [ "${1:-}" = "--check" ]; then mode=check; fi

# The plug-ins have their own lock files and build folder, apart from the workspace.
target="${CARGO_TARGET_DIR:-$PWD/target}/plugin-fixtures"
fixtures=crates/kadakas-plugin/tests/fixtures
mkdir -p "$fixtures"

# The home folder first and the repository second: when two prefixes match, rustc takes the last.
remap="--remap-path-prefix=${HOME}=/build-home --remap-path-prefix=${PWD}=/build-repo"
if [ -n "${CARGO_HOME:-}" ]; then remap="--remap-path-prefix=${CARGO_HOME}=/build-cargo $remap"; fi
if [ -n "${RUSTUP_HOME:-}" ]; then remap="--remap-path-prefix=${RUSTUP_HOME}=/build-rustup $remap"; fi

status=0
# crate folder : module file name
for entry in raised-planter:raised_planter stone-row:stone_row; do
  dir="${entry%%:*}"
  name="${entry##*:}"
  RUSTFLAGS="$remap" CARGO_TARGET_DIR="$target" cargo build --release --locked \
    --manifest-path "plugins/$dir/Cargo.toml" --target wasm32-unknown-unknown
  built="$target/wasm32-unknown-unknown/release/$name.wasm"
  # No path of a home folder in the module.
  if grep -aqE '/Users/|/home/|[A-Za-z]:\\Users\\' "$built"; then
    echo "error: $name.wasm holds the path of a home folder" >&2
    status=1
  fi
  if [ "$mode" = check ]; then
    if cmp -s "$built" "$fixtures/$name.wasm"; then
      echo "ok: $name.wasm is up to date"
    else
      echo "error: $fixtures/$name.wasm differs from a fresh build. Run scripts/build-plugin-fixtures.sh" >&2
      status=1
    fi
  else
    cp "$built" "$fixtures/$name.wasm"
    ls -l "$fixtures/$name.wasm"
  fi
done
exit $status
