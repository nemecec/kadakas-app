# Kadakas behavior tests

Run from the repo root: `uv run --with pytest pytest tests/behavior` (or `python3 -m venv tests/behavior/.venv && tests/behavior/.venv/bin/pip install -r tests/behavior/requirements.txt && tests/behavior/.venv/bin/pytest tests/behavior`).
The suite builds `kadakas-cli` first; set `KADAKAS_SKIP_BUILD=1` to reuse `target/debug/kadakas`, or `KADAKAS_BIN` to point at another binary.
Known bugs are marked `xfail(strict=True)` with the reason; when one is fixed, its test reports XPASS and must have its marker removed.

The BCF tests (`test_issues.py`) check every XML file of a BCF export against the buildingSMART XSD files in `bcf_schemas/`. The check runs in a child process with `uv run --with lxml` (`bcf_check.py`), like the IFC tests use `--with ifcopenshell`; the test is skipped when `uv` is not installed. `bcf_cases/` holds three official BCF 2.1 test files (license in the folder).
