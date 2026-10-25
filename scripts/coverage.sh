#!/usr/bin/env bash
# Line coverage per crate with cargo llvm-cov. The rule of docs/dev-guide.md: every library crate
# keeps 95% of its lines or more. The command line crate has a lower bar (see below). The app crate
# (egui and wgpu) is left out: its window code has no headless test.
#
#   scripts/coverage.sh             all crates
#   scripts/coverage.sh print api   only these crates (names without the kadakas- prefix)
#   scripts/coverage.sh shard 2/4   only the crates of one shard (CI nightly runs the four shards in parallel)
#   scripts/coverage.sh --list [shard 2/4]   print the crates and stop (no build)
#
# The shards are balanced by the size of the code and the tests (core, api, ui-core and io are the
# heavy ones). Every shard applies the same bars. A crate in none or in two shards is an error.
#
# Needs: cargo install cargo-llvm-cov, and the rustup component llvm-tools-preview.
set -euo pipefail

LIB_MIN=95
# The CLI is a thin shell around the API. Its integration tests run the real binary. The bar is
# lower because some start-up error paths (a broken terminal, a closed stdout) cannot be reached.
CLI_MIN=90

LIBS=(units l10n sys container file crs plants core props review collab collab-ws render io annot gis ifc x3d meshio step irrigation script script-js plugin-abi plugin site sheet print api update sign-manifest ui-core feedback crash welcome)

SHARD_COUNT=4
shard_crates() {
  case "$1" in
    1) echo "core units l10n sys crs plants annot script script-js plugin-abi sign-manifest crash welcome" ;;
    2) echo "api container file props collab collab-ws render" ;;
    3) echo "ui-core io review gis x3d meshio" ;;
    4) echo "step print site sheet ifc irrigation plugin update feedback cli" ;;
    *) echo "unknown shard $1 (1 to $SHARD_COUNT)" >&2; return 1 ;;
  esac
}

# Every crate of the full list must be in exactly one shard.
check_shards() {
  local all=" ${LIBS[*]} cli " listed="" n c
  for n in $(seq 1 "$SHARD_COUNT"); do listed="$listed $(shard_crates "$n")"; done
  for c in ${LIBS[@]} cli; do
    local count
    count=$(tr ' ' '\n' <<<"$listed" | grep -cx -- "$c" || true)
    if [ "$count" != 1 ]; then echo "!! crate $c is in $count shards (needs 1)" >&2; return 1; fi
  done
  for c in $listed; do
    case "$all" in *" $c "*) ;; *) echo "!! shard crate $c is not in LIBS" >&2; return 1 ;; esac
  done
}

list_only=0
if [ "${1:-}" = "--list" ]; then list_only=1; shift; fi

if [ "${1:-}" = "shard" ]; then
  check_shards
  n="${2:-}"; n="${n%%/*}"
  if ! [[ "$n" =~ ^[0-9]+$ ]]; then echo "usage: scripts/coverage.sh shard N/$SHARD_COUNT" >&2; exit 2; fi
  if [ "${2#*/}" != "$SHARD_COUNT" ]; then echo "the number of shards is $SHARD_COUNT" >&2; exit 2; fi
  read -r -a CRATES <<<"$(shard_crates "$n")"
elif [ "$#" -gt 0 ]; then
  CRATES=("$@")
else
  CRATES=("${LIBS[@]}" cli)
fi

if [ "$list_only" = 1 ]; then
  echo "${CRATES[*]}"
  exit 0
fi

failed=0
for c in "${CRATES[@]}"; do
  min=$LIB_MIN
  if [ "$c" = "cli" ]; then min=$CLI_MIN; fi
  echo "== kadakas-$c (at least ${min}% of lines)"
  # Profile data of an earlier run, or of an older tree, would be counted again and gives wrong
  # totals. Start every crate from clean profile data.
  cargo llvm-cov clean --workspace
  # The script engine needs its feature to be built (and a wasi-sdk, see docs/dev-guide.md).
  features=()
  if [ "$c" = "script-js" ]; then features=(--features engine); fi
  if ! cargo llvm-cov -p "kadakas-$c" ${features[@]+"${features[@]}"} --summary-only --fail-under-lines "$min"; then
    echo "!! kadakas-$c is below ${min}% line coverage"
    failed=1
  fi
done
exit $failed
