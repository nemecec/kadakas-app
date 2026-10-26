# shellcheck shell=bash
# Helpers for the smoke scripts (linux.sh, macos.sh). Source this file, do not run it.
# See docs/release.md ("Smoke test of the installers").

SMOKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

step() { echo "smoke: $*"; }
warn() { echo "smoke: WARNING: $*" >&2; }
fail() { echo "smoke: FAILED: $*" >&2; exit 1; }

# need_file PATH [WHAT]: the path is a file (or a link to one).
need_file() { [ -f "$1" ] || fail "${2:-file} is missing: $1"; }
# need_exec PATH [WHAT]: the path is an executable file.
need_exec() { need_file "$@"; [ -x "$1" ] || fail "${2:-file} is not executable: $1"; }
# gone PATH: the path does not exist (a dead link counts as existing).
gone() { { [ ! -e "$1" ] && [ ! -L "$1" ]; } || fail "still there after the uninstall: $1"; }

smoke_python() {
  if command -v python3 >/dev/null 2>&1; then echo python3; else echo python; fi
}

# run_smoke LAUNCHER VERSION ARTIFACTS SCREENSHOT_CMD [WORK]
# Runs the starts and the checks of smoke_run.py. SMOKE_RUN_ARGS adds more options.
# python -I keeps the current folder and the PYTHON variables out of the imports.
run_smoke() {
  local launcher="$1" version="$2" artifacts="$3" shot="$4" work="${5:-}"
  local py; py="$(smoke_python)"
  local args=(run --launcher "$launcher" --version "$version")
  [ -z "$artifacts" ] || args+=(--artifacts "$artifacts")
  [ -z "$shot" ] || args+=(--screenshot-cmd "$shot")
  [ -z "$work" ] || args+=(--work "$work")
  # shellcheck disable=SC2086
  "$py" -I "$SMOKE_DIR/smoke_run.py" "${args[@]}" ${SMOKE_RUN_ARGS:-}
}
