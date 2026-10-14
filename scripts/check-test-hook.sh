#!/usr/bin/env bash
# Fails if a test-only feature is in the feature tree of a program that we ship.
#
# `kadakas-core` has the feature `fault-injection`: a command can be made to panic, so that tests
# can check that a panic leaves no trace. Only the dev-dependencies of `kadakas-api` turn it on.
# A release program must never have it. The check reads the feature tree of the shipped programs
# (normal and build dependencies, all targets) and looks for the names in HOOKS.
#
#   scripts/check-test-hook.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# Add the name of every new test-only feature here.
HOOKS='fault-injection|test-hook'
PATTERN="feature \"(${HOOKS})\""

shipped=$(cargo tree -e normal,build,features --target all \
  -p kadakas-app -p kadakas-cli -p kadakas-launcher)
if echo "$shipped" | grep -Eq "$PATTERN"; then
  echo "!! a test-only feature is in the feature tree of a shipped program:"
  echo "$shipped" | grep -E "$PATTERN"
  exit 1
fi

# The check must be able to see the hook. With the dev-dependencies of kadakas-api, the tree has
# it. If it does not, the pattern or the name has changed, and the check above sees nothing.
with_dev=$(cargo tree -e normal,build,dev,features --target all -p kadakas-api)
if ! echo "$with_dev" | grep -Eq 'feature "fault-injection"'; then
  echo "!! the check cannot see the feature fault-injection in the tree of kadakas-api (dev)."
  echo "   Update scripts/check-test-hook.sh."
  exit 1
fi

echo "ok: no test-only feature in kadakas-app, kadakas-cli or kadakas-launcher"
