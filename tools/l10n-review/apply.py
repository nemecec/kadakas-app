"""Apply accepted suggestions to .ftl files. Rewrites only the value text of a message."""
import os
import subprocess
import tempfile

import fluent


def rebuild_lines(msg, new_raw):
    """Lines that replace the key line and the value lines of `msg` (not the attributes)."""
    parts = new_raw.split("\n")
    pad = " " * msg.indent
    if msg.inline_first:
        out = ["%s = %s" % (msg.key, parts[0])]
        rest = parts[1:]
    else:
        out = ["%s =" % msg.key]
        rest = parts
    for p in rest:
        out.append(pad + p if p.strip() else "")
    return out


def check_replacement(old_target, english, new_raw):
    """Return an error text, or None when the new text is safe."""
    if not new_raw.strip():
        return "The new text is empty."
    new_vars = set(fluent.extract_variables(new_raw))
    en_vars = set(english.variables) if english is not None else set(old_target.variables)
    if new_vars != en_vars:
        missing = sorted(en_vars - new_vars)
        extra = sorted(new_vars - en_vars)
        bits = []
        if missing:
            bits.append("drops " + ", ".join("$" + v for v in missing))
        if extra:
            bits.append("adds " + ", ".join("$" + v for v in extra))
        return "The variables differ from the English text: " + "; ".join(bits) + "."
    if fluent.structure_signature(new_raw) != fluent.structure_signature(old_target.raw):
        return "The selector structure (variant keys, default mark, braces) differs from the current text."
    if not old_target.selector and new_raw.count("\n") != old_target.raw.count("\n"):
        return "The number of lines differs from the current text."
    return None


def plan(target_dir, source_files, items):
    """items: list of dicts {id, file, key, text}. Returns list of change dicts, not yet written."""
    results = []
    cache = {}
    for it in items:
        fn, key, text = it["file"], it["key"], it["text"]
        res = {"id": it["id"], "file": fn, "key": key, "error": None, "old_lines": [],
               "new_lines": [], "line": 0, "new_raw": text}
        results.append(res)
        path = os.path.join(target_dir, fn)
        if fn not in cache:
            if not os.path.isfile(path):
                cache[fn] = None
            else:
                cache[fn] = fluent.parse_file(path)
        parsed = cache[fn]
        if parsed is None:
            res["error"] = "The file %s does not exist." % fn
            continue
        old = next((m for m in parsed.messages if m.key == key), None)
        if old is None:
            res["error"] = "The key is not in the file any more."
            continue
        en = None
        if fn in source_files:
            en = next((m for m in source_files[fn].messages if m.key == key), None)
        if en is None:
            res["error"] = "The English message does not exist."
            continue
        err = check_replacement(old, en, text)
        if err:
            res["error"] = err
            continue
        if text == old.raw:
            res["error"] = "The text is already in the file."
            continue
        res["line"] = old.start
        res["old_lines"] = parsed.lines[old.start - 1:old.value_end]
        res["new_lines"] = rebuild_lines(old, text)
    return results


def git_dirty(repo, target_dir):
    """Names of changed files under target_dir, or None when git cannot tell."""
    try:
        r = subprocess.run(["git", "-C", repo, "status", "--porcelain", "--", target_dir],
                           capture_output=True, text=True, timeout=30, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    return [l[3:] for l in r.stdout.split("\n") if l.strip()]


def write_changes(target_dir, source_files, changes):
    """Write the changes that have no error. Returns {id: error or None}. Verifies each file first."""
    by_file = {}
    for c in changes:
        if not c["error"]:
            by_file.setdefault(c["file"], []).append(c)
    outcome = {c["id"]: c["error"] for c in changes}
    for fn, cs in by_file.items():
        path = os.path.join(target_dir, fn)
        with open(path, encoding="utf-8", newline="") as f:
            text = f.read()
        if "\r" in text:
            for c in cs:
                outcome[c["id"]] = "The file uses CRLF line ends. Not touched."
            continue
        before = fluent.parse_text(text, fn)
        lines = text.split("\n")
        trailing = lines[-1] == ""
        for c in sorted(cs, key=lambda c: -c["line"]):
            old = next(m for m in before.messages if m.key == c["key"])
            lines[old.start - 1:old.value_end] = c["new_lines"]
        new_text = "\n".join(lines)
        after = fluent.parse_text(new_text, fn)
        problem = verify(before, after, source_files.get(fn), {c["key"]: c for c in cs})
        if problem:
            for c in cs:
                outcome[c["id"]] = problem
            continue
        if trailing and not new_text.endswith("\n"):
            new_text += "\n"
        atomic_write(path, new_text)
    return outcome


def verify(before, after, english, changed):
    """The file must parse to the same keys, and the variables must equal the English ones."""
    if [m.key for m in before.messages] != [m.key for m in after.messages]:
        return "The rewritten file has other keys. Not written."
    if [t.key for t in before.terms] != [t.key for t in after.terms]:
        return "The rewritten file has other terms. Not written."
    en = {m.key: m for m in english.messages} if english else {}
    for old, new in zip(before.messages, after.messages):
        if old.key in changed:
            if new.raw != changed[old.key]["new_raw"]:
                return "The text of %s did not read back the same. Not written." % old.key
            if old.key in en and set(new.variables) != set(en[old.key].variables):
                return "The variables of %s differ from the English text. Not written." % old.key
        else:
            if old.raw != new.raw or old.comment != new.comment or old.attributes != new.attributes:
                return "The message %s changed by accident. Not written." % old.key
    return None


def atomic_write(path, text):
    folder = os.path.dirname(path)
    fd, tmp = tempfile.mkstemp(prefix=".ftl-", suffix=".tmp", dir=folder)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise
