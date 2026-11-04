"""Apply accepted suggestions to .ftl files. Rewrites only the value text of a message."""
import collections
import hashlib
import os
import re
import subprocess
import tempfile

import ftl
from state import hash_text


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


def variable_warnings(old_raw_vars, new_raw):
    """Warnings for an English edit that removes variables."""
    gone = sorted(set(old_raw_vars) - set(ftl.extract_variables(new_raw)))
    return ["The edit removes %s. Update the target text too." % ", ".join("$" + v for v in gone)] if gone else []


def check_replacement(old_target, english, new_raw, source_edit=False, call_sites=""):
    """Return an error text, or None when the new text is safe.

    With source_edit the text is an edit of the English source. It may drop variables but
    must not add one that the code does not pass.
    """
    if not new_raw.strip():
        return "The new text is empty."
    if new_raw != new_raw.strip():
        return "The text has a leading or trailing space or line break."
    en = english if english is not None else old_target
    new_vars = set(ftl.extract_variables(new_raw))
    en_vars = set(en.value_variables)
    if source_edit:
        added = sorted(new_vars - en_vars)
        if added:
            return ("The edit adds %s. The code does not pass it, so the program would show an error. %s"
                    % (", ".join("$" + v for v in added), call_sites)).strip()
    elif new_vars != en_vars:
        missing = sorted(en_vars - new_vars)
        extra = sorted(new_vars - en_vars)
        bits = []
        if missing:
            bits.append("drops " + ", ".join("$" + v for v in missing))
        if extra:
            bits.append("adds " + ", ".join("$" + v for v in extra))
        return "The variables differ from the English text: " + "; ".join(bits) + "."
    if ftl.brace_balance(new_raw) != 0:
        return "The braces are not balanced."
    found, empty = ftl.placeables(new_raw)
    if empty:
        return "The text has an empty placeable { }."
    old_found, _ = ftl.placeables(old_target.raw)
    c_new, c_old = collections.Counter(found), collections.Counter(old_found)
    if c_new != c_old:
        gone = sorted((c_old - c_new).elements())
        added = sorted((c_new - c_old).elements())
        bits = []
        if gone:
            bits.append("drops " + ", ".join("{ %s }" % g for g in gone))
        if added:
            bits.append("adds " + ", ".join("{ %s }" % g for g in added))
        return "The placeables (terms, literals, functions) differ from the current text: " + "; ".join(bits) + "."
    if ftl.structure_signature(new_raw) != ftl.structure_signature(old_target.raw):
        return "The selector structure (variant keys, default mark, braces) differs from the current text."
    lines = new_raw.split("\n")
    for k, ln in enumerate(lines):
        st = ln.lstrip()
        if st[:1] == "." or (not old_target.selector and st[:1] in ("[", "*")):
            return "A line starts with '%s', which Fluent reads as syntax." % st[:1]
    if old_target.selector:
        old_lines = old_target.raw.split("\n")
        if len(old_lines) != len(lines) or [len(l) - len(l.lstrip()) for l in old_lines] != \
                [len(l) - len(l.lstrip()) for l in lines]:
            return "The lines or the indentation differ from the current selector text."
    elif new_raw.count("\n") != old_target.raw.count("\n"):
        return "The number of lines differs from the current text."
    return None


def line_hash(lines):
    return hashlib.sha1("\n".join(lines).encode("utf-8")).hexdigest()[:12]


def digest(changes):
    """What the preview shows, in a form the apply request sends back."""
    return [{"id": c["id"], "old": line_hash(c["old_lines"]), "new": line_hash(c["new_lines"])}
            for c in changes if not c["error"]]


class _Vars:
    """Stand-in for a message when only its variables matter."""

    def __init__(self, raw):
        self.value_variables = ftl.extract_variables(raw)


def plan(dirs, files, source_locale, items, call_sites=None):
    """Plan the changes. Nothing is written.

    dirs: {locale: folder}. files: {locale: {file name: FtlFile}}, freshly parsed.
    items: dicts {id, mid, locale, file, key, text, base_hash, en_hash}.
    call_sites: optional function key -> text for the error of an added variable.
    Returns one change dict for each item.
    """
    results, by_mid = [], {}
    # English edits first: a target text is then checked against the new English variables.
    ordered = sorted(items, key=lambda it: it["locale"] != source_locale)
    en_new = {}
    for it in ordered:
        loc, fn, key, text = it["locale"], it["file"], it["key"], it["text"]
        res = {"id": it["id"], "mid": it.get("mid", it["id"]), "locale": loc, "file": fn, "key": key,
               "error": None, "old_lines": [], "new_lines": [], "line": 0, "new_raw": text,
               "old_raw": "", "warnings": []}
        results.append(res)
        parsed = (files.get(loc) or {}).get(fn)
        if parsed is None:
            res["error"] = "The file %s does not exist for %s." % (fn, loc)
            continue
        old = next((m for m in parsed.messages if m.key == key), None)
        if old is None:
            res["error"] = "The key is not in the file any more."
            continue
        en_file = (files.get(source_locale) or {}).get(fn)
        en = next((m for m in en_file.messages if m.key == key), None) if en_file else None
        if en is None:
            res["error"] = "The English message does not exist."
            continue
        if it.get("base_hash") and it["base_hash"] != hash_text(old.raw):
            res["error"] = "The text changed since the suggestion was made."
            continue
        if loc != source_locale and it.get("en_hash") and it["en_hash"] != hash_text(en.raw):
            res["error"] = "The English text changed since the suggestion was made."
            continue
        if loc == source_locale:
            sites = call_sites(key) if call_sites else ""
            err = check_replacement(old, old, text, source_edit=True, call_sites=sites)
            res["warnings"] = variable_warnings(old.value_variables, text)
        else:
            eff = _Vars(en_new[(fn, key)]) if (fn, key) in en_new else en
            err = check_replacement(old, eff, text)
        if err:
            res["error"] = err
            continue
        if text == old.raw:
            res["error"] = "The text is already in the file."
            continue
        if loc == source_locale:
            en_new[(fn, key)] = text
        res["line"] = old.start
        res["old_raw"] = old.raw
        res["old_lines"] = parsed.lines[old.start - 1:old.value_end]
        res["new_lines"] = rebuild_lines(old, text)
    # keep the order of the input for the preview
    pos = {it["id"]: n for n, it in enumerate(items)}
    results.sort(key=lambda r: pos[r["id"]])
    return results


def git_dirty(repo, dirs):
    """Names of changed files under the given folders, or None when git cannot tell."""
    try:
        r = subprocess.run(["git", "-C", repo, "status", "--porcelain", "--"] + list(dirs),
                           capture_output=True, text=True, timeout=30, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    return [l[3:] for l in r.stdout.split("\n") if l.strip()]


def write_changes(dirs, files, source_locale, changes):
    """Write the changes that have no error. Returns {id: error or None}.

    Every file is checked before it is written. dirs: {locale: folder}. files: {locale: {name: FtlFile}}
    as parsed before the changes.
    """
    groups = {}
    for c in changes:
        if not c["error"]:
            groups.setdefault((c["locale"], c["file"]), []).append(c)
    outcome = {c["id"]: c["error"] for c in changes}
    en_new = {(c["file"], c["key"]): c["new_raw"] for c in changes
              if c["locale"] == source_locale and not c["error"]}
    for (loc, fn), cs in groups.items():
        path = os.path.join(dirs[loc], fn)
        with open(path, encoding="utf-8", newline="") as f:
            text = f.read()
        if "\r" in text:
            for c in cs:
                outcome[c["id"]] = "The file uses CRLF line ends. Not touched."
            continue
        before = ftl.parse_text(text, fn)
        bad = False
        lines = text.split("\n")
        trailing = lines[-1] == ""
        for c in sorted(cs, key=lambda c: -c["line"]):
            old = next(m for m in before.messages if m.key == c["key"])
            if lines[old.start - 1:old.value_end] != c["old_lines"]:
                outcome[c["id"]] = "The file changed since the preview. Not written."
                bad = True
                break
            lines[old.start - 1:old.value_end] = c["new_lines"]
        if bad:
            continue
        new_text = "\n".join(lines)
        after = ftl.parse_text(new_text, fn)
        en_file = (files.get(source_locale) or {}).get(fn)
        problem = verify(before, after, en_file, {c["key"]: c for c in cs}, loc == source_locale, en_new)
        if problem:
            for c in cs:
                outcome[c["id"]] = problem
            continue
        old_junk, new_junk = official_junk(text), official_junk(new_text)
        if old_junk is not None and new_junk is not None and new_junk > old_junk:
            for c in cs:
                outcome[c["id"]] = "The official Fluent parser rejects the result. Not written."
            continue
        if trailing and not new_text.endswith("\n"):
            new_text += "\n"
        atomic_write(path, new_text)
    return outcome


def official_junk(text):
    """Number of Junk entries by the official parser, or None when fluent.syntax is not installed."""
    try:
        from fluent.syntax import parse
        from fluent.syntax.ast import Junk
    except ImportError:
        return None
    return sum(isinstance(e, Junk) for e in parse(text).body)


def verify(before, after, english, changed, is_source, en_new):
    """The file must parse to the same keys, and the variables must fit the English ones."""
    if [m.key for m in before.messages] != [m.key for m in after.messages]:
        return "The rewritten file has other keys. Not written."
    if [t.key for t in before.terms] != [t.key for t in after.terms]:
        return "The rewritten file has other terms. Not written."
    en = {m.key: m for m in english.messages} if english else {}
    for old, new in zip(before.messages, after.messages):
        if old.key in changed:
            if new.raw != changed[old.key]["new_raw"]:
                return "The text of %s did not read back the same. Not written." % old.key
            if is_source:
                if not set(new.value_variables) <= set(old.value_variables):
                    return "The English text of %s gained a variable. Not written." % old.key
            else:
                want = set(ftl.extract_variables(en_new[(new.file, old.key)])) if (new.file, old.key) in en_new \
                    else set(en[old.key].value_variables) if old.key in en else None
                if want is not None and set(new.value_variables) != want:
                    return "The variables of %s differ from the English text. Not written." % old.key
        else:
            if old.raw != new.raw or old.comment != new.comment or old.attributes != new.attributes:
                return "The message %s changed by accident. Not written." % old.key
    return None


def atomic_write(path, text):
    folder = os.path.dirname(path)
    mode = os.stat(path).st_mode & 0o7777
    fd, tmp = tempfile.mkstemp(prefix=".ftl-", suffix=".tmp", dir=folder)
    try:
        os.chmod(tmp, mode)
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
