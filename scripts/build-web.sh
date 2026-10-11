#!/usr/bin/env bash
# Builds the browser version of Kadakas into dist-web/.
#   scripts/build-web.sh           release build (small, slow to build)
#   scripts/build-web.sh --serve   dev build and a local server on http://127.0.0.1:8080
set -euo pipefail
cd "$(dirname "$0")/.."

rustup target add wasm32-unknown-unknown
if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk (once)..."
  cargo install trunk --locked
fi

if [ "${1:-}" = "--serve" ]; then
  exec trunk serve
fi
# KADAKAS_WEB_FEATURES: cargo features of the app crate, for example `test-hook` (the browser
# test of kadakas-server turns it on; a normal web build has no test hook).
if [ -n "${KADAKAS_WEB_FEATURES:-}" ]; then
  trunk build --release --features "$KADAKAS_WEB_FEATURES"
else
  trunk build --release
fi
echo "Done. Serve the dist-web folder with any static web server, for example:"
echo "  python3 -m http.server -d dist-web 8080"
