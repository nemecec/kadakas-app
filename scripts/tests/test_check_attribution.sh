#!/usr/bin/env bash
# Tests of scripts/check-attribution.sh: the gate must pass on the real tree and fail on bad input.
# Needs git, python3 and cargo (offline). Run: scripts/tests/test_check_attribution.sh
# The tests that add files to the tree remove them again, also after an interrupt (trap).
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp="$(mktemp -d)"
zz_files=(assets/icons/zz-gate-test.svg crates/kadakas-l10n/zz-gate-test.png crates/kadakas-l10n/zz-gate-test.txt)
cleanup() {
  rm -rf "$tmp"
  rm -f "${zz_files[@]}"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Pure Python tests (expressions, globs, include_bytes!, mini repositories).
python3 -W ignore -I scripts/tests/test_check_attribution_unit.py

# One run of cargo for all cases. A failing cargo command stops the test.
scripts/attribution-cargo-data.sh "$tmp"
export ATTR_METADATA="$tmp/metadata.json" ATTR_TREE="$tmp/tree.txt"

# From here a failing case must not stop the script.
set +e
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
expect_out() {
  if grep -q -- "$2" <<< "$out"; then echo "ok    $1"; else echo "FAIL  $1 (wanted: $2)"; echo "$out" | head -20; fails=$((fails + 1)); fi
}
reset() { unset ATTR_TSV ATTR_OPEN ATTR_NOTICES ATTR_ATTRIBUTION ATTR_LICENCES GITHUB_ACTIONS; }
real_tsv=docs/attribution-assets.tsv
# The UNKNOWN rows of today. A new one needs a deliberate change of this number.
expected_unknown_rows=0

reset
expect_pass "real tree passes"
expect_out "number of UNKNOWN rows is pinned" "$expected_unknown_rows UNKNOWN row(s)"
# A synthetic UNKNOWN row (the real tree has none) is printed as a warning and as an annotation.
awk -F'\t' 'BEGIN{OFS="\t"} /^crates\/kadakas-render\/testdata\/red_blue.jpg/ {$3="UNKNOWN"; $4="UNKNOWN"; $5="UNKNOWN"} {print}' "$real_tsv" > "$tmp/u.tsv"
{ cat docs/attribution-open.txt; echo "crates/kadakas-render/testdata/red_blue.jpg"; } > "$tmp/u.open"
# The doc table must match the synthetic table, so make a copy of attribution.md with its new table.
ATTR_TSV="$tmp/u.tsv" scripts/check-attribution.sh --table | grep '^|' > "$tmp/u.table"
python3 -I - "$tmp/u.table" "$tmp/u.att" <<'PY'
import sys
doc = open("docs/attribution.md").read().split("\n")
tab = open(sys.argv[1]).read().rstrip("\n").split("\n")
i = next(k for k, l in enumerate(doc) if l.startswith("## Asset list"))
a = next(k for k in range(i, len(doc)) if doc[k].startswith("| Item"))
b = a
while b < len(doc) and doc[b].startswith("|"):
    b += 1
doc[a:b] = tab
open(sys.argv[2], "w").write("\n".join(doc))
PY
export ATTR_TSV="$tmp/u.tsv" ATTR_OPEN="$tmp/u.open" ATTR_ATTRIBUTION="$tmp/u.att"
run
expect_out "UNKNOWN rows are printed as warnings" "^WARN  crates/kadakas-render/testdata/red_blue.jpg"
expect_out "a new UNKNOWN row is counted" "1 UNKNOWN row(s)"
GITHUB_ACTIONS=true run
expect_out "GitHub annotations for UNKNOWN rows" "^::warning file=crates/kadakas-render/testdata/red_blue.jpg::"
reset

# 1. A file without a row.
grep -v '^assets/icons/tabler/' "$real_tsv" > "$tmp/a.tsv"
ATTR_TSV="$tmp/a.tsv" expect_fail "missing row for an icon folder" "assets/icons/tabler/.*no row"
touch assets/icons/zz-gate-test.svg
expect_fail "new untracked asset file" "zz-gate-test.svg"
rm -f assets/icons/zz-gate-test.svg
touch crates/kadakas-l10n/zz-gate-test.png
expect_fail "new image outside the folders of the table" "zz-gate-test.png"
rm -f crates/kadakas-l10n/zz-gate-test.png

# 3. A stale glob.
{ cat "$real_tsv"; printf 'assets/fonts/**\tGhost font\tNobody\tMIT\thttps://example.org\ttest\n'; } > "$tmp/b.tsv"
ATTR_TSV="$tmp/b.tsv" expect_fail "stale glob" "matches no file"

# 2. Licence, source and the UNKNOWN rules.
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4=""} {print}' "$real_tsv" > "$tmp/c.tsv"
ATTR_TSV="$tmp/c.tsv" expect_fail "missing licence" "no licence"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4="WTFPL-9"} {print}' "$real_tsv" > "$tmp/d.tsv"
ATTR_TSV="$tmp/d.tsv" expect_fail "unknown licence id" "WTFPL-9"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4="MIT OR"} {print}' "$real_tsv" > "$tmp/d2.tsv"
ATTR_TSV="$tmp/d2.tsv" expect_fail "dangling operator in a licence" "the expression ends"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$5=""} {print}' "$real_tsv" > "$tmp/e.tsv"
ATTR_TSV="$tmp/e.tsv" expect_fail "missing source URL" "no source URL"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$4="UNKNOWN"} {print}' "$real_tsv" > "$tmp/f.tsv"
ATTR_TSV="$tmp/f.tsv" expect_fail "UNKNOWN outside tests, testdata and fixtures" "only allowed for a path under tests/"
awk -F'\t' 'BEGIN{OFS="\t"} /^assets\/icons\/tabler/ {$6="test"} {print}' "$real_tsv" > "$tmp/g.tsv"
ATTR_TSV="$tmp/g.tsv" run
awk -F'\t' 'BEGIN{OFS="\t"} /^tests\/behavior\/bcf_cases/ {$6="ships"} {print}' "$real_tsv" > "$tmp/h.tsv"
ATTR_TSV="$tmp/h.tsv" expect_fail "CC-BY-ND-4.0 on a shipped row" "test-only"
: > "$tmp/open.txt"
ATTR_TSV="$tmp/u.tsv" ATTR_ATTRIBUTION="$tmp/u.att" ATTR_OPEN="$tmp/open.txt" expect_fail "empty open list" "does not name it"
{ cat "$tmp/u.open"; echo "crates/kadakas-render/testdata/red_blue.jpg"; } > "$tmp/open3.txt"
ATTR_TSV="$tmp/u.tsv" ATTR_ATTRIBUTION="$tmp/u.att" ATTR_OPEN="$tmp/open3.txt" expect_fail "duplicate line in the open list" "listed twice"
{ cat docs/attribution-open.txt; echo "nothing/here.png"; } > "$tmp/open4.txt"
ATTR_OPEN="$tmp/open4.txt" expect_fail "open list line without a file" "not a file"
reset

# 6. attribution.md must hold the table.
grep -v 'Tabler Icons' docs/attribution.md > "$tmp/att.md"
ATTR_ATTRIBUTION="$tmp/att.md" expect_fail "row missing from attribution.md" "is not the output of"

# 4. Cargo dependencies and notices.
grep -v '^  - ring ' THIRD_PARTY_NOTICES.txt > "$tmp/notices.txt"
ATTR_NOTICES="$tmp/notices.txt" expect_fail "crate missing from THIRD_PARTY_NOTICES.txt" "crate ring ships but"
sed 's/wasi-libc/wasi-lib-c/g' THIRD_PARTY_NOTICES.txt > "$tmp/notices2.txt"
ATTR_NOTICES="$tmp/notices2.txt" expect_fail "shipped ext: row missing from the notices" "does not name 'wasi-libc'"
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

# 5. A third-party header outside the table.
printf 'Copyright (c) 2001 Somebody Else\n' > crates/kadakas-l10n/zz-gate-test.txt
expect_fail "third-party copyright header" "zz-gate-test.txt"
printf 'copyright 2001 somebody else\n' > crates/kadakas-l10n/zz-gate-test.txt
expect_fail "lowercase header without (c)" "zz-gate-test.txt"
rm -f crates/kadakas-l10n/zz-gate-test.txt

reset
expect_pass "real tree passes again"

if [[ $fails -ne 0 ]]; then echo "$fails test(s) failed"; exit 1; fi
echo "all tests passed"
