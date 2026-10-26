#!/usr/bin/env bash
# Tests find-tested-commit.sh in a temporary repository with a fake `gh`.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
ok() { echo "ok: $1"; }
bad() { echo "FAIL: $1" >&2; fails=$((fails + 1)); }

mkdir -p "$tmp/bin" "$tmp/runs" "$tmp/repo"
# The fake gh prints the JSON that the file runs/<sha> holds. No file: no run.
cat > "$tmp/bin/gh" <<'EOT'
#!/usr/bin/env bash
sha=""
while [ $# -gt 0 ]; do if [ "$1" = "--commit" ]; then sha="$2"; fi; shift; done
if [ -f "$FAKE_RUNS/$sha" ]; then cat "$FAKE_RUNS/$sha"; fi
EOT
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH" FAKE_RUNS="$tmp/runs"

cd "$tmp/repo"
git init -q -b main
git config user.name tester; git config user.email tester@example.org
commit() { # message file [author-email]
  mkdir -p "$(dirname "$2")"; echo "$RANDOM" >> "$2"; git add "$2"
  git -c user.email="${3:-tester@example.org}" commit -q -m "$1"; git rev-parse HEAD
}
run() { echo "{\"status\":\"$2\",\"conclusion\":\"$3\",\"url\":\"https://example.org/run/$1\"}" > "$tmp/runs/$1"; }
expect() { # label expected-exit
  local label="$1" want="$2" got=0
  "$here/find-tested-commit.sh" > "$tmp/out" 2> "$tmp/err" || got=$?
  if [ "$got" = "$want" ]; then ok "$label"; else bad "$label (exit $got, want $want): $(cat "$tmp/err")"; fi
}

a=$(commit "source" src/a.rs); run "$a" completed success
expect "HEAD has a green run" 0
commit "readme only [skip ci]" README.md >/dev/null
expect "a markdown file outside docs after a green commit" 0
grep -q "tested=$a" "$tmp/out" && ok "reports the tested commit" || bad "reports the tested commit"
commit "docs [skip ci]" docs/keyboard-shortcuts.md >/dev/null
expect "a file in docs/ counts (tests and installers read it)" 1
git reset -q --hard HEAD~1
c=$(commit "source [skip ci]" src/b.rs)
expect "a source change without CI is refused" 1
grep -q "src/b\|source \[skip ci\]" "$tmp/err" && ok "lists the untested commit" || bad "lists the untested commit"
commit "Set next development version 0.1.1-dev" Cargo.toml github-actions[bot]@users.noreply.github.com >/dev/null
run "$c" completed success
expect "bot commit after a green source commit" 0
commit "Set next development version 0.1.2-dev" src/evil.rs github-actions[bot]@users.noreply.github.com >/dev/null
expect "a bot-looking commit that changes a source file is refused" 1
git reset -q --hard HEAD~1
e=$(commit "source" src/c.rs); run "$e" completed failure
expect "a red run is refused" 1
run "$e" in_progress null
expect "CI still running on HEAD is refused" 1
run "$e" completed success
commit "Release 0.1.0" Cargo.toml github-actions[bot]@users.noreply.github.com >/dev/null
expect "the release commit of the bot is not a source change" 0
# A merge commit brings a source change in on a side branch: no CI run tested it.
git checkout -q -b side
commit "side source" src/side.rs >/dev/null
git checkout -q main
git merge -q --no-ff side -m "Merge side [skip ci]"
expect "a source change through a merge is refused" 1
git reset -q --hard HEAD~1
git branch -q -D side
git checkout -q -b docside
commit "side readme" README.md >/dev/null
git checkout -q main
git merge -q --no-ff docside -m "Merge docside"
expect "a merge of a markdown change is fine" 0
git reset -q --hard HEAD~1
rm -f "$tmp/runs/"*
expect "no run at all is refused" 1
[ "$fails" = 0 ] || { echo "$fails test(s) failed" >&2; exit 1; }
echo "all find-tested-commit tests passed"
