#!/usr/bin/env bash
# Attribution gate (PLAT-21). Fails when a shipped asset, dataset, vendored file or Cargo
# dependency has no entry in docs/attribution-assets.tsv or THIRD_PARTY_NOTICES.txt.
#
#   scripts/check-attribution.sh           run all checks
#   scripts/check-attribution.sh --table   print the asset table as Markdown for docs/attribution.md
#
# Needs git, python3 and cargo. It reads Cargo data with --locked --offline: run `cargo fetch --locked`
# once before (CI does). Tests set ATTR_METADATA and ATTR_TREE and skip cargo.
set -euo pipefail
cd "$(dirname "$0")/.."
here="$(pwd)"
if [[ "${1:-}" != "--table" && -z "${ATTR_METADATA:-}" ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  "$here/scripts/attribution-cargo-data.sh" "$tmp"
  export ATTR_METADATA="$tmp/metadata.json" ATTR_TREE="$tmp/tree.txt"
fi
# No exec: the trap must run.
code=0
python3 -I "$here/scripts/check_attribution.py" "$@" || code=$?
exit "$code"
