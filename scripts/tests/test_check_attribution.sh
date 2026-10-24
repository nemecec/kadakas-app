#!/usr/bin/env bash
# Tests of scripts/check-attribution.sh: the gate must pass on the real tree and fail on bad input.
# Needs git, python3 and cargo (offline). Run: scripts/tests/test_check_attribution.sh
set -uo pipefail
cd "$(dirname "$0")/../.."
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# One run of cargo for all cases.
cargo metadata --format-version 1 --locked --offline > "$tmp/metadata.json" || exit 2
for t in x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu wasm32-unknown-unknown; do
  cargo tree --workspace -e normal,build --target "$t" --prefix none --locked --offline --charset ascii
done | awk '{print $1}' | sort -u > "$tmp/tree.txt"
export ATTR_METADATA="$tmp/metadata.json" ATTR_TREE="$tmp/tree.txt"

fails=0
run() { out="$(python3 -I scripts/check_attribution.py 2>&1)"; code=$?; }
expect_pass() {
  run
  if [[ $code -eq 0 ]]; then echo "ok    $1"; else echo "FAIL  $1 (exit $code)"; echo "$out" | head -20; fails=$((fails + 1)); fi
}
# expect_fail NAME PATTERN: the run must exit 1 and print PATTERN.
expect_fail() {
  run
  if [[ $code -eq 1 ]] && grep -q -- "$2" <<< "$out"; then echo "ok    $1"; else echo "FAIL  $1 (exit $code, wanted: $2)"; echo "$out" | head -20; fails=$((fails + 1)); fi
}
reset() { unset ATTR_TSV ATTR_OPEN ATTR_NOTICES ATTR_ATTRIBUTION ATTR_LICENCES; export ATTR_METADATA="$tmp/metadata.json"; }
real_tsv=docs/attribution-assets.tsv

reset
expect_pass "real tree passes"

# 1. A file without a row.
grep -v '^assets/icons/tabler/' "$real_tsv" > "$tmp/a.tsv"
ATTR_TSV="$tmp/a.tsv" expect_fail "missing row for a font or icon file" "assets/icons/tabler/.*no row"
reset

# A new file in an asset folder (not committed) needs a row.
touch assets/icons/zz-gate-test.svg
expect_fail "new untracked asset file" "zz-gate-test.svg"
rm -f assets/icons/zz-gate-test.svg
touch crates/kadakas-l10n/zz-gate-test.png
expect_fail "new image outside the folders of the table" "zz-gate-test.png"
rm -f crates/kadakas-l10n/zz-gate-test.png
reset

# 3. A stale glob.
{ cat "$real_tsv"; printf 'assets/fonts/**\tGhost font\tNobody\tMIT\thttps://example.org\n'; } > "$tmp/b.tsv"
ATTR_TSV="$tmp/b.tsv" expect_fail "stale glob" "matches no file"
reset

# 2. A missing licence, an unknown licence id, a missing source, UNKNOWN without the open list.
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4=""} {print}' "$real_tsv" > "$tmp/c.tsv"
ATTR_TSV="$tmp/c.tsv" expect_fail "missing licence" "no licence"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4="WTFPL-9"} {print}' "$real_tsv" > "$tmp/d.tsv"
ATTR_TSV="$tmp/d.tsv" expect_fail "unknown licence id" "WTFPL-9"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$5=""} {print}' "$real_tsv" > "$tmp/e.tsv"
ATTR_TSV="$tmp/e.tsv" expect_fail "missing source URL" "no source URL"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4="UNKNOWN"} {print}' "$real_tsv" > "$tmp/f.tsv"
ATTR_TSV="$tmp/f.tsv" expect_fail "UNKNOWN row outside the open list" "not listed in docs/attribution-open.txt"
reset
expect_pass "UNKNOWN rows are only warnings when listed"
ATTR_OPEN=/dev/null expect_fail "open list is empty but rows are UNKNOWN" "not listed"
printf 'nothing/here.png\n' > "$tmp/open2.txt"
cat docs/attribution-open.txt "$tmp/open2.txt" > "$tmp/open3.txt"
ATTR_OPEN="$tmp/open3.txt" expect_fail "open list line without a row" "no UNKNOWN row"
reset

# The table must be in attribution.md.
grep -v 'Tabler Icons' docs/attribution.md > "$tmp/att.md"
ATTR_ATTRIBUTION="$tmp/att.md" expect_fail "row missing from attribution.md" "does not mention"
reset

# 4. Cargo dependencies.
grep -v '^  - ring ' THIRD_PARTY_NOTICES.txt > "$tmp/notices.txt"
ATTR_NOTICES="$tmp/notices.txt" expect_fail "crate missing from THIRD_PARTY_NOTICES.txt" "crate ring ships but"
python3 -I - "$tmp/metadata.json" "$tmp/metadata-nolicence.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
for p in d["packages"]:
    if p["name"] == "ring":
        p["license"] = None
        p["license_file"] = None
json.dump(d, open(sys.argv[2], "w"))
PY
ATTR_METADATA="$tmp/metadata-nolicence.json" expect_fail "crate without licence" "crate ring .* has no licence"
reset

# 5. A third-party header outside the table.
cat > crates/kadakas-l10n/zz-gate-test.txt <<'EOF'
Copyright (c) 2001 Somebody Else
EOF
expect_fail "third-party copyright header" "zz-gate-test.txt"
rm -f crates/kadakas-l10n/zz-gate-test.txt
reset
expect_pass "real tree passes again"

if [[ $fails -ne 0 ]]; then echo "$fails test(s) failed"; exit 1; fi
echo "all tests passed"
