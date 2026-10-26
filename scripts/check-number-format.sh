#!/usr/bin/env bash
# Number format gate (PLAT-26): the UI writes and reads numbers in the number format of the
# person. See scripts/check_number_format.py and docs/welcome.md. Needs python3 only.
#
#   scripts/check-number-format.sh          check
#   scripts/check-number-format.sh --fix    add .ui_number() to the number widgets that lack it
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 -I scripts/check_number_format.py "$@"
