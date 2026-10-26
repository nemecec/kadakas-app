#!/usr/bin/env bash
#
# Finds the commit that CI tested for a release: the newest commit among the last 20 on the
# first-parent line of HEAD that has a completed run of ci.yml. HEAD itself often has none (a
# commit with [skip ci], or the commit of the release bot, which does not start workflows).
#
# Usage: find-tested-commit.sh [REF]        (default HEAD; needs `gh` and a full clone)
# Prints "tested=<sha>" and "run=<url>" on success. Exit 1 when:
#   - the run on the tested commit did not succeed, or CI still runs on HEAD;
#   - there is no run in the last 20 commits;
#   - a commit after the tested one changed a source file. Only *.md files outside docs/ and the
#     issue templates do not count: tests and installers read files in docs/ (include_str!,
#     packager resources). A commit of the release bot ("Release ..." and "Set next development
#     version ...") does not count when it changed only version files.
#   - A merge commit counts with the files that it changed against its first parent.
# The list of the untested commits goes to stderr, so the plan job shows it.
set -euo pipefail

head_sha="$(git rev-parse "${1:-HEAD}")"
tested="" url=""
for c in $(git rev-list --first-parent -n 20 "$head_sha"); do
  run="$(gh run list --workflow ci.yml --event push --branch main --commit "$c" --limit 1 --json status,conclusion,url --jq '.[0] // empty')"
  [ -n "$run" ] || continue
  status="$(jq -r .status <<<"$run")"
  concl="$(jq -r .conclusion <<<"$run")"
  if [ "$status" != completed ]; then
    if [ "$c" = "$head_sha" ]; then
      echo "CI is still running on $c ($(jq -r .url <<<"$run")). Wait for it." >&2
      exit 1
    fi
    continue
  fi
  if [ "$concl" != success ]; then
    echo "CI on $c is $concl ($(jq -r .url <<<"$run")). Fix it, or start with skip_ci_check." >&2
    exit 1
  fi
  tested="$c"
  url="$(jq -r .url <<<"$run")"
  break
done
if [ -z "$tested" ]; then
  echo "No completed CI run in the last 20 commits before $head_sha. Wait for CI, or start with skip_ci_check." >&2
  exit 1
fi

if [ "$tested" != "$head_sha" ]; then
  echo "CI tested $tested ($url). These commits came after it:" >&2
  git log --format='  %h %an: %s' "$tested..$head_sha" >&2
  bad=""
  # The files that a release commit of the bot may change.
  version_files='^(Cargo\.toml|Cargo\.lock|packager\.toml|packager-macos\.toml|python/pyproject\.toml|python/kadakas/__init__\.py|THIRD_PARTY_NOTICES\.txt|CHANGELOG\.md)$'
  for c in $(git rev-list --first-parent "$tested..$head_sha"); do
    author="$(git log -1 --format=%ae "$c")"
    subject="$(git log -1 --format=%s "$c")"
    # A merge commit: compare with the first parent (diff-tree shows nothing for a merge).
    if git rev-parse -q --verify "$c^1" >/dev/null; then
      changed="$(git diff --name-only "$c^1" "$c")"
    else
      changed="$(git diff-tree --no-commit-id --name-only -r --root "$c")"
    fi
    if [[ "$author" == *github-actions* ]] && [[ "$subject" == "Release "* || "$subject" == "Set next development version "* ]]; then
      if ! grep -Ev "$version_files" <<<"$changed" | grep -q .; then continue; fi
    fi
    files="$(grep -Ev '(^|/)[^/]*\.md$|^\.github/ISSUE_TEMPLATE/' <<<"$changed" || true)"
    # A markdown file in docs/ counts: tests and installers read it.
    docs_md="$(grep -E '^docs/.*\.md$' <<<"$changed" || true)"
    files="$(printf '%s\n%s\n' "$files" "$docs_md" | grep . || true)"
    if [ -n "$files" ]; then bad+="$(git log -1 --format='  %h %s' "$c")"$'\n'; fi
  done
  if [ -n "$bad" ]; then
    echo "These commits changed source files and no CI run tested them:" >&2
    printf '%s' "$bad" >&2
    echo "Wait for CI on the head of main, or start with skip_ci_check." >&2
    exit 1
  fi
fi
echo "tested=$tested"
echo "run=$url"
