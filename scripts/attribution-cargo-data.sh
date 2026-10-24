#!/usr/bin/env bash
# Writes metadata.json (cargo metadata) and tree.txt (names of the crates that ship) into the folder
# given as the first argument. Used by check-attribution.sh and its tests. Offline and locked.
# The targets come from about.toml, the same list that cargo-about uses for THIRD_PARTY_NOTICES.txt.
set -euo pipefail
cd "$(dirname "$0")/.."
out="$1"
targets="$(awk '/^targets *= *\[/ {f=1; next} f && /\]/ {exit} f {gsub(/[",[:space:]]/, ""); if ($0 != "") print}' about.toml)"
if [[ -z "$targets" ]]; then
  echo "no targets found in about.toml" >&2
  exit 1
fi
cargo metadata --format-version 1 --locked --offline > "$out/metadata.json"
: > "$out/tree.raw"
for t in $targets; do
  # No dev dependencies: they do not ship.
  cargo tree --workspace -e normal,build --target "$t" --prefix none --locked --offline --charset ascii >> "$out/tree.raw" || exit 1
done
awk '{print $1}' "$out/tree.raw" | sort -u > "$out/tree.txt"
rm -f "$out/tree.raw"
[[ -s "$out/tree.txt" ]] || { echo "cargo tree gave no crates" >&2; exit 1; }
