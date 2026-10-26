#!/usr/bin/env bash
#
# Downloads highest-published.json (the record of the highest published version, see
# scripts/published_guard.py) from the release update-manifests into DIR.
#
# Usage: fetch-published-record.sh DIR      (needs `gh` and GH_TOKEN)
# "Missing" is decided from the list of assets: no release, or no asset of that name. Any other
# error (a network error, a 502) stops with exit 1. A failed read must never look like an empty
# record, because the record would then be lowered or erased.
set -euo pipefail
dir="${1:?usage: fetch-published-record.sh DIR}"
mkdir -p "$dir"
err="$(mktemp)"
trap 'rm -f "$err"' EXIT
if ! assets="$(gh release view update-manifests --json assets --jq '.assets[].name' 2>"$err")"; then
  if grep -qi 'release not found' "$err"; then
    echo "There is no release update-manifests yet, so no record."
    exit 0
  fi
  cat "$err" >&2
  echo "Cannot read the release update-manifests. Stopping: a failed read must not look like an empty record." >&2
  exit 1
fi
if grep -qx 'highest-published.json' <<<"$assets"; then
  gh release download update-manifests --pattern highest-published.json --dir "$dir" --clobber
  [ -s "$dir/highest-published.json" ] || { echo "The record is empty." >&2; exit 1; }
  echo "Downloaded the record."
else
  echo "The release has no record yet."
fi
