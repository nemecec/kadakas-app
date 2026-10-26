#!/usr/bin/env python3
"""Number format gate (PLAT-26). Every number that the UI shows or reads follows the number
format of the person (decimal symbol and digit grouping), through one helper:

* a `DragValue` or a `Slider` of egui has a unit extension (`.length()`, `.degrees()`, ...) or
  `.ui_number()` (crates/kadakas-ui-number), which writes the number in the format of
  the person and reads both `.` and `,`;
* a decimal number in a label goes through `kadakas_units::num` and its relatives, not through
  `format!("{:.2}", x)`.

The scan covers the UI crates, outside `#[cfg(test)]` code. A site that is right for another
reason is in scripts/number-format-allow.txt, one line each: `path<TAB>text<TAB>reason`, where the
text is a part of the line or of the widget expression. A line of the list that matches nothing
fails the check (so the list does not grow stale).

    scripts/check-number-format.sh          check
    scripts/check-number-format.sh --fix    add .ui_number() to the widgets that lack it
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATES = [
    "kadakas-app",
    "kadakas-modeler",
    "kadakas-view3d",
    "kadakas-ui-core",
    "kadakas-launcher",
    "kadakas-update",
    "kadakas-welcome",
]
ALLOW = ROOT / "scripts" / "number-format-allow.txt"
IMPORT = "use kadakas_ui_number::UiNumber;"

# A widget that has one of these in its builder chain is fine.
MARKERS = (
    ".length(",
    ".length_m(",
    ".paper_mm(",
    ".degrees(",
    ".direction(",
    ".per_area(",
    ".area_m2(",
    ".volume_m3(",
    ".ui_number(",
    "custom_formatter",
    "show_value(false)",
)
WIDGET = re.compile(r"\b(?:egui::)?(DragValue|Slider)::new\(")
# A macro that writes text, and a placeholder with a precision in its text: `{:.2}`, `{x:.10}`,
# `{:>6.1}`, `{:.prec$}`, `{:.*}`. A precision of 0 writes a whole number and is fine.
MACRO = re.compile(r"\b(?:format|write|writeln|print|println|eprint|eprintln|panic)!\(")
PLACEHOLDER = re.compile(
    r"\{(?:[A-Za-z_][\w.\[\]]*)?:[^{}]*\.(?:[1-9]\d*|[A-Za-z_]\w*\$|\*)[^{}]*\}"
)


def code_part(text):
    """The text without the test module at the end of a file."""
    m = re.search(r"^#\[cfg\(test\)\]\s*$", text, re.M)
    return text[: m.start()] if m else text


def skip_group(text, i):
    """`i` is at an opening bracket; the index after its closing bracket."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [pairs[text[i]]]
    i += 1
    while i < len(text) and stack:
        c = text[i]
        if c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c in pairs:
            stack.append(pairs[c])
        elif c == stack[-1]:
            stack.pop()
        i += 1
    return i


def chain_end(text, i):
    """The end of `DragValue::new(...).a().b()` that starts with the arguments at `i`."""
    i = skip_group(text, i)
    while True:
        m = re.match(r"\s*\.\s*[a-z_0-9]+\s*(\()?", text[i:])
        if not m:
            return i
        i += m.end()
        if m.group(1):
            i = skip_group(text, i - 1)


def widgets(text):
    """(start, end, chain) of each widget in the code part of `text`."""
    out = []
    for m in WIDGET.finditer(text):
        end = chain_end(text, m.end() - 1)
        out.append((m.start(), end, text[m.start() : end]))
    return out


def raw_floats(code):
    """(position, text of the macro call) for each text macro (format!, write!, println!, ...)
    that has a placeholder with a precision."""
    out = []
    for m in MACRO.finditer(code):
        end = skip_group(code, m.end() - 1)
        call = code[m.start() : end]
        if PLACEHOLDER.search(call):
            out.append((m.start(), call))
    return out


def line_of(text, pos):
    return text.count("\n", 0, pos) + 1


def load_allow():
    rules = []
    if ALLOW.exists():
        for n, line in enumerate(ALLOW.read_text(encoding="utf-8").splitlines(), 1):
            if not line.strip() or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) != 3 or not all(p.strip() for p in parts):
                sys.exit(f"{ALLOW.name}:{n}: a line is `path<TAB>text<TAB>reason`")
            rules.append([parts[0].strip(), parts[1].strip(), parts[2].strip(), False, n])
    return rules


def allowed(rules, path, snippet):
    for r in rules:
        if r[0] == path and r[1] in snippet:
            r[3] = True
            return True
    return False


def files():
    for crate in CRATES:
        for p in sorted((ROOT / "crates" / crate / "src").rglob("*.rs")):
            name = p.name
            if name.startswith("tests") or "/tests/" in p.as_posix():
                continue
            yield p


def with_import(text):
    if IMPORT in text:
        return text
    m = re.search(r"^use .*;\s*$", text, re.M)
    pos = m.end() if m else 0
    return text[:pos] + "\n" + IMPORT + text[pos:]


def main(argv):
    fix = "--fix" in argv
    rules = load_allow()
    problems = []
    for p in files():
        rel = p.relative_to(ROOT).as_posix()
        if rel.startswith("crates/kadakas-ui-number/") or rel.endswith("units_ui.rs"):
            continue
        text = p.read_text(encoding="utf-8")
        code = code_part(text)
        if fix:
            new = code
            for start, end, chain in reversed(widgets(code)):
                if any(k in chain for k in MARKERS) or allowed(rules, rel, chain):
                    continue
                new = new[:end] + ".ui_number()" + new[end:]
            if new != code:
                p.write_text(with_import(new) + text[len(code) :], encoding="utf-8")
            continue
        for start, end, chain in widgets(code):
            if any(k in chain for k in MARKERS) or allowed(rules, rel, chain):
                continue
            problems.append(
                f"{rel}:{line_of(code, start)}: a number widget without `.ui_number()` or a unit "
                "extension (it writes a point and reads only a point)"
            )
        for pos, text in raw_floats(code):
            line_start = code.rfind("\n", 0, pos) + 1
            line = code[line_start : code.find("\n", pos)]
            if line.strip().startswith("//") or allowed(rules, rel, line) or allowed(rules, rel, text):
                continue
            problems.append(
                f"{rel}:{line_of(code, pos)}: a decimal number with a fixed precision in a "
                "format string (`{:.2}`, `{:>6.1}`, `{:.prec$}`); use kadakas_units::num "
                "(or the allow list, with a reason)"
            )
    if not fix:
        for r in rules:
            if not r[3]:
                problems.append(f"{ALLOW.name}:{r[4]}: matches nothing (remove the line)")
    if problems:
        print("\n".join(problems))
        print(f"number format check failed: {len(problems)} problem(s). See docs/welcome.md.")
        return 1
    if not fix:
        print("number format check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
