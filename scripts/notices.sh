#!/usr/bin/env bash
# Makes THIRD_PARTY_NOTICES.txt: the licenses of all dependencies that we ship.
# The installers include the file (packager.toml, "resources"). CI checks that it is up to date.
#
#   cargo install cargo-about --locked --features cli
#   scripts/notices.sh
set -euo pipefail
cd "$(dirname "$0")/.."
cargo about generate --workspace --fail about.hbs --output-file THIRD_PARTY_NOTICES.txt
echo "wrote THIRD_PARTY_NOTICES.txt"
