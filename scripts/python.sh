#!/usr/bin/env bash
# Sourced by the version scripts: sets PY to a working Python 3.
#
#   . "$(dirname "${BASH_SOURCE[0]}")/python.sh"; "$PY" -I script.py
#
# $KADAKAS_PYTHON wins (the tests set it to the Python that runs them). Else the first of python3
# and python that runs. On Windows (Git bash) there is often no python3, and a stub of the Store
# may answer in its place, so each candidate must really run.
if [ -n "${KADAKAS_PYTHON:-}" ]; then
  PY="$KADAKAS_PYTHON"
else
  PY=""
  for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
      PY="$candidate"
      break
    fi
  done
fi
if [ -z "$PY" ]; then
  echo "No working Python 3 found (tried python3 and python). Install Python 3.8 or newer, or set KADAKAS_PYTHON." >&2
  exit 1
fi
