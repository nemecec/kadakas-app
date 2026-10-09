# Cadektor behavior tests

Run from the repo root: `uv run --with pytest pytest tests/behavior` (or `python3 -m venv tests/behavior/.venv && tests/behavior/.venv/bin/pip install -r tests/behavior/requirements.txt && tests/behavior/.venv/bin/pytest tests/behavior`).
The suite builds `cadektor-cli` first; set `CADEKTOR_SKIP_BUILD=1` to reuse `target/debug/cadektor`, or `CADEKTOR_BIN` to point at another binary.
Known bugs are marked `xfail(strict=True)` with the reason; when one is fixed, its test reports XPASS and must have its marker removed.
