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
#   - a commit after the tested one changed a source file. Docs, *.md files, issue and PR
#     templates, and the commits of the release bot ("Release ..." and "Set next development
#     version ...": they change only version files) do not count.
# The list of the untested commits goes to stderr, so the plan job shows it.
set -euo pipefail

head_sha="$(git rev-parse "${1:-HEAD}")"
tested="" url=""
for c in $(git rev-list --first-parent -n 20 "$head_sha"); do
  run="$(gh run list --workflow ci.yml --commit "$c" --limit 1 --json status,conclusion,url --jq '.[0] // empty')"
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
  for c in $(git rev-list --first-parent "$tested..$head_sha"); do
    author="$(git log -1 --format=%ae "$c")"
    subject="$(git log -1 --format=%s "$c")"
    if [[ "$author" == *github-actions* ]] && [[ "$subject" == "Release "* || "$subject" == "Set next development version "* ]]; then
      continue
    fi
    files="$(git diff-tree --no-commit-id --name-only -r --root "$c" | grep -Ev '^docs/|\.md$|^\.github/ISSUE_TEMPLATE/|^\.github/pull_request_template\.md$' || true)"
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
