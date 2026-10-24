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
  cargo metadata --format-version 1 --locked --offline > "$tmp/metadata.json"
  # The crates that ship: no dev dependencies, the five targets of about.toml.
  for t in x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu wasm32-unknown-unknown; do
    cargo tree --workspace -e normal,build --target "$t" --prefix none --locked --offline --charset ascii
  done | awk '{print $1}' | sort -u > "$tmp/tree.txt"
  export ATTR_METADATA="$tmp/metadata.json" ATTR_TREE="$tmp/tree.txt"
fi
exec python3 -I "$here/scripts/check_attribution.py" "$@"
