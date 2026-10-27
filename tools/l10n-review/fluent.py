"""Small Fluent (.ftl) parser for the review tool. Standard library only.

It is not a full Fluent parser. It reads the shapes this repo uses:
group comments, comments, messages, terms, attributes, multi-line values,
selectors, variables.
"""
import os
import re
from dataclasses import dataclass, field, asdict

MSG_RE = re.compile(r"^([A-Za-z][A-Za-z0-9_-]*)[ \t]*=(.*)$")
TERM_RE = re.compile(r"^(-[A-Za-z][A-Za-z0-9_-]*)[ \t]*=(.*)$")
ATTR_RE = re.compile(r"^[ \t]+\.([A-Za-z][A-Za-z0-9_-]*)[ \t]*=(.*)$")
VAR_RE = re.compile(r"\$([A-Za-z][A-Za-z0-9_-]*)")
VARIANT_RE = re.compile(r"^[ \t]*(\*?)\[([^\]]+)\][ \t]*(.*)$")
SELECTOR_RE = re.compile(r"\{\s*([^{}]*?)\s*->")
GROUP_RE = re.compile(r"^##\s*(.*)$")


@dataclass
class Message:
    file: str
    key: str
    start: int            # 1-based line of the key line
    end: int              # 1-based last line of the message (attributes included)
    value_end: int        # 1-based last line of the value (before attributes)
    comment: str = ""
    comment_start: int = 0
    raw: str = ""         # value text, continuation lines dedented
    inline_first: bool = False   # the value starts on the key line
    indent: int = 4       # common indent of the continuation lines
    variables: list = field(default_factory=list)
    selector: str = ""    # selector expression text, "" when none
    variants: list = field(default_factory=list)  # {key, default, text}
    attributes: list = field(default_factory=list)  # {name, raw}
    is_term: bool = False

    def to_dict(self):
        return asdict(self)


@dataclass
class FtlFile:
    name: str
    group: str = ""
    messages: list = field(default_factory=list)
    terms: list = field(default_factory=list)
    lines: list = field(default_factory=list)


def extract_variables(text):
    """Variable names in order of first use, without the dollar sign."""
    seen = []
    for m in VAR_RE.finditer(text):
        if m.group(1) not in seen:
            seen.append(m.group(1))
    return seen


def parse_variants(raw):
    """Return (selector, variants) for the first selector in a raw value."""
    sel = SELECTOR_RE.search(raw)
    selector = sel.group(1) if sel else ""
    variants = []
    for line in raw.split("\n"):
        m = VARIANT_RE.match(line)
        if m:
            variants.append({"key": m.group(2).strip(), "default": bool(m.group(1)),
                             "text": m.group(3).strip()})
        elif variants and line.strip() and not line.strip().startswith("}"):
            variants[-1]["text"] += "\n" + line.strip()
    return selector, variants


def _depth_delta(line):
    return line.count("{") - line.count("}")


def parse_text(text, name="<memory>"):
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines = lines[:-1]
    out = FtlFile(name=name, lines=lines)
    n = len(lines)
    i = 0
    pending = []   # (line number, text) of comment lines directly above
    while i < n:
        line = lines[i]
        if not line.strip():
            pending = []
            i += 1
            continue
        if line.startswith("#"):
            g = GROUP_RE.match(line)
            if g and not line.startswith("###"):
                if not out.group:
                    out.group = g.group(1).strip()
                pending = []
            elif line.startswith("###"):
                pending = []
            else:
                pending.append((i + 1, line[1:].strip()))
            i += 1
            continue
        m = MSG_RE.match(line)
        t = TERM_RE.match(line)
        if not (m or t):
            pending = []
            i += 1
            continue
        mm = m or t
        key, first = mm.group(1), mm.group(2)
        # find the end of the message
        depth = _depth_delta(first)
        j = i + 1
        last = i  # index of last non-blank line of the message
        while j < n:
            ln = lines[j]
            if not ln.strip():
                j += 1
                continue
            if ln[0] in " \t" or depth > 0:
                depth += _depth_delta(ln)
                last = j
                j += 1
                continue
            break
        # split value and attributes
        value_last = i
        attrs = []
        cur = None
        for k in range(i + 1, last + 1):
            am = ATTR_RE.match(lines[k])
            if am and depth_ok(lines, i, k):
                cur = {"name": am.group(1), "parts": [am.group(2).strip()]}
                attrs.append(cur)
            elif cur is not None:
                cur["parts"].append(lines[k])
            elif lines[k].strip():
                value_last = k
        cont = [lines[k] for k in range(i + 1, value_last + 1)]
        indents = [len(c) - len(c.lstrip()) for c in cont if c.strip()]
        indent = min(indents) if indents else 4
        parts = []
        first_s = first.strip()
        if first_s:
            parts.append(first_s)
        for c in cont:
            parts.append(c[indent:] if c.strip() else "")
        raw = "\n".join(parts).strip("\n")
        msg = Message(file=name, key=key, start=i + 1, end=last + 1, value_end=value_last + 1,
                      raw=raw, inline_first=bool(first_s), indent=indent, is_term=bool(t))
        if pending and pending[-1][0] == i:
            msg.comment = "\n".join(p[1] for p in pending)
            msg.comment_start = pending[0][0]
        pending = []
        msg.variables = extract_variables(raw)
        msg.selector, msg.variants = parse_variants(raw)
        for a in attrs:
            ar = "\n".join(p.strip() if idx else p for idx, p in enumerate(a["parts"])).strip()
            msg.attributes.append({"name": a["name"], "raw": ar})
            for v in extract_variables(ar):
                if v not in msg.variables:
                    msg.variables.append(v)
        (out.terms if t else out.messages).append(msg)
        i = last + 1
    return out


def depth_ok(lines, start, k):
    """An attribute line counts only outside an open selector."""
    depth = 0
    for idx in range(start, k):
        text = lines[idx]
        if idx == start:
            text = text.split("=", 1)[1] if "=" in text else text
        depth += _depth_delta(text)
    return depth <= 0


def parse_file(path):
    with open(path, encoding="utf-8", newline="") as f:
        text = f.read()
    return parse_text(text, os.path.basename(path))


def parse_locale(locale_dir):
    """Parse every .ftl file of a locale. Returns {file name: FtlFile}."""
    result = {}
    if not os.path.isdir(locale_dir):
        return result
    for fn in sorted(os.listdir(locale_dir)):
        if fn.endswith(".ftl"):
            result[fn] = parse_file(os.path.join(locale_dir, fn))
    return result


def key_parity(source, target):
    """Compare {file: FtlFile} maps. Returns dict with lists of (file, key)."""
    def keys(loc):
        return {(fn, m.key) for fn, f in loc.items() for m in f.messages}
    s, t = keys(source), keys(target)
    return {"missing": sorted(s - t), "extra": sorted(t - s),
            "missing_files": sorted(set(source) - set(target)),
            "extra_files": sorted(set(target) - set(source))}


def structure_signature(msg_or_raw):
    """Selector expression and variant keys with default marks, for structure checks."""
    raw = msg_or_raw if isinstance(msg_or_raw, str) else msg_or_raw.raw
    selector, variants = parse_variants(raw)
    return (selector, tuple((v["default"], v["key"]) for v in variants),
            raw.count("{") - raw.count("}"))
