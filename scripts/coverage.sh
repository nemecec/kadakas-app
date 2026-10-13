#!/usr/bin/env bash
# Line coverage per crate with cargo llvm-cov. The rule of docs/dev-guide.md: every library crate
# keeps 95% of its lines or more. The command line crate has a lower bar (see below). The app crate
# (egui and wgpu) is left out: its window code has no headless test.
#
#   scripts/coverage.sh             all crates
#   scripts/coverage.sh print api   only these crates (names without the kadakas- prefix)
#
# Needs: cargo install cargo-llvm-cov, and the rustup component llvm-tools-preview.
set -euo pipefail

LIB_MIN=95
# The CLI is a thin shell around the API. Its integration tests run the real binary. The bar is
# lower because some start-up error paths (a broken terminal, a closed stdout) cannot be reached.
CLI_MIN=90

LIBS=(units l10n sys crs plants core props review collab collab-ws render io annot gis ifc x3d irrigation script plugin-abi plugin site sheet print api update sign-manifest ui-core)

if [ "$#" -gt 0 ]; then
  CRATES=("$@")
else
  CRATES=("${LIBS[@]}" cli)
fi

failed=0
for c in "${CRATES[@]}"; do
  min=$LIB_MIN
  if [ "$c" = "cli" ]; then min=$CLI_MIN; fi
  echo "== kadakas-$c (at least ${min}% of lines)"
  if ! cargo llvm-cov -p "kadakas-$c" --summary-only --fail-under-lines "$min"; then
    echo "!! kadakas-$c is below ${min}% line coverage"
    failed=1
  fi
done
exit $failed
