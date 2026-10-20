#!/usr/bin/env bash
# Fails if a test-only feature is in the feature tree of a program that we ship.
#
# `kadakas-core` has the feature `fault-injection`: a command can be made to panic, so that tests
# can check that a panic leaves no trace. Only the dev-dependencies of `kadakas-api` turn it on.
# `testkit` (kadakas-collab, kadakas-plugin) gives tests a fake server or a plain wasm compiler.
# A release program must never have it. The check reads the feature tree of the shipped programs
# (normal and build dependencies, all targets) and looks for the names in HOOKS.
#
#   scripts/check-test-hook.sh
set -euo pipefail
# Here-strings, not `echo | grep -q`: with pipefail, grep -q exits at the first match, echo then
# fails on the closed pipe, and the match would count as a miss.
cd "$(dirname "$0")/.."

# Add the name of every new test-only feature here.
HOOKS='fault-injection|test-hook|testkit'
PATTERN="feature \"(${HOOKS})\""

shipped=$(cargo tree -e normal,build,features --target all \
  -p kadakas-app -p kadakas-modeler -p kadakas-cli -p kadakas-launcher)
if grep -Eq "$PATTERN" <<<"$shipped"; then
  echo "!! a test-only feature is in the feature tree of a shipped program:"
  grep -E "$PATTERN" <<<"$shipped"
  exit 1
fi

# The check must be able to see the hook. With the dev-dependencies of kadakas-api, the tree has
# it. If it does not, the pattern or the name has changed, and the check above sees nothing.
with_dev=$(cargo tree -e normal,build,dev,features --target all -p kadakas-api)
if ! grep -Eq 'feature "fault-injection"' <<<"$with_dev"; then
  echo "!! the check cannot see the feature fault-injection in the tree of kadakas-api (dev)."
  echo "   Update scripts/check-test-hook.sh."
  exit 1
fi

echo "ok: no test-only feature in kadakas-app, kadakas-modeler, kadakas-cli or kadakas-launcher"
