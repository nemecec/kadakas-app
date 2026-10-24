"""Helper of scripts/check-attribution.sh (PLAT-21). Run it with python3 -I.

Reads the table docs/attribution-assets.tsv and checks it against the repository.
Standard library only. See docs/dev-guide.md, "Add an asset".

Environment (the shell script and the tests set these):
  ATTR_ROOT        repository root (default: the parent of this folder)
  ATTR_TSV         asset table
  ATTR_LICENCES    allow-list of licence ids
  ATTR_OPEN        list of files that may have the licence UNKNOWN
  ATTR_ATTRIBUTION docs/attribution.md
  ATTR_NOTICES     THIRD_PARTY_NOTICES.txt
  ATTR_METADATA    output of cargo metadata (JSON file)
  ATTR_TREE        crate names that ship, one per line (from cargo tree)
  GITHUB_ACTIONS   "true" prints ::warning annotations for the UNKNOWN rows
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.environ.get("ATTR_ROOT") or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def path(env, default):
    return os.environ.get(env) or os.path.join(ROOT, default)


TSV = path("ATTR_TSV", "docs/attribution-assets.tsv")
LICENCES = path("ATTR_LICENCES", "docs/attribution-licences.txt")
OPEN = path("ATTR_OPEN", "docs/attribution-open.txt")
ATTRIBUTION = path("ATTR_ATTRIBUTION", "docs/attribution.md")
NOTICES = path("ATTR_NOTICES", "THIRD_PARTY_NOTICES.txt")

# Files with these extensions are assets wherever they are.
ASSET_EXT = {
    "svg", "png", "jpg", "jpeg", "gif", "ico", "icns", "ttf", "ttc", "otf", "woff", "woff2",
    "ftl", "csv", "json", "geojson", "dxf", "dwg", "ifc", "bcf", "bcfzip", "xsd", "wasm", "wat",
    "kad", "plist", "pat", "lin", "shx", "hdr", "exr", "ktx2", "webp", "bmp", "tif", "tiff",
    "gltf", "glb", "obj", "stl", "pdf", "xml", "bin", "dat", "wgsl", "glsl", "spv",
    "js", "css", "html",
}
# Every file in a folder with one of these names is an asset (except Rust and Markdown files).
ASSET_DIRS = {
    "assets", "data", "testdata", "fixtures", "locales", "profiles", "icons", "fonts", "web",
    "static", "res", "resources", "shaders", "vendor", "third_party", "third-party",
}
# In these folders Rust and Markdown files are vendored code too, so they need a row.
VENDOR_DIRS = {"vendor", "third_party", "third-party", "web"}
# include_bytes! and include_str! targets with these extensions are our own code or text.
EMBED_SKIP_EXT = {"rs", "md", "py"}
EMBED_SKIP_FILES = {"THIRD_PARTY_NOTICES.txt"}
# Header scan (check 5): files that are ours by definition.
HEADER_SKIP_PREFIX = ("docs/", "scripts/", "target/")
HEADER_SKIP_FILES = {"LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.txt", "about.hbs"}
HEADER_RE = re.compile(
    r"copyright\s*(?:\(c\)|©)|copyright\s+(?:\d{4}|by\b)|licensed under|spdx-license-identifier",
    re.IGNORECASE,
)
# A header line that names the project or the project licence is ours.
HEADER_OWN = re.compile(r"kadakas|apache-2\.0 or mit|mit or apache-2\.0", re.IGNORECASE)
HEADER_BYTES = 4096
OWN = "Kadakas project"
UNKNOWN_DIRS = {"tests", "testdata", "fixtures"}

failures = []
warnings = []


def fail(section, text):
    failures.append((section, text))


def read_lines(p):
    if not os.path.exists(p):
        fail("setup", "missing file: %s" % os.path.relpath(p, ROOT))
        return []
    out = []
    with open(p, encoding="utf-8", errors="surrogateescape") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.strip() and not line.lstrip().startswith("#"):
                out.append(line)
    return out


def list_files():
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT, capture_output=True, check=True,
    ).stdout.decode("utf-8", errors="surrogateescape")
    files = []
    for f in sorted(set(out.split("\0"))):
        if not f:
            continue
        full = os.path.join(ROOT, f)
        if os.path.islink(full):
            real = os.path.realpath(full)
            root = os.path.realpath(ROOT)
            if real != root and not real.startswith(root + os.sep):
                fail("symlink", "%s: a symbolic link to a place outside the repository" % f)
                continue
        if os.path.isfile(full):
            files.append(f)
    return files


def glob_re(pattern):
    i, out = 0, ""
    while i < len(pattern):
        c = pattern[i]
        if pattern.startswith("**/", i):
            out += "(?:.*/)?"
            i += 3
            continue
        if pattern.startswith("**", i):
            out += ".*"
            i += 2
            continue
        if c == "*":
            out += "[^/]*"
        elif c == "?":
            out += "[^/]"
        else:
            out += re.escape(c)
        i += 1
    return re.compile("^" + out + "$")


COLUMNS = ("path", "name", "author", "licence", "source", "use")


def parse_tsv():
    rows = []
    for line in read_lines(TSV):
        cols = line.split("\t")
        if len(cols) != len(COLUMNS):
            fail("table", "docs/attribution-assets.tsv: a row needs %d tab separated columns, got %d: %r" % (len(COLUMNS), len(cols), line[:70]))
            continue
        rows.append(dict(zip(COLUMNS, [c.strip() for c in cols])))
    return rows


def parse_licence(expr):
    """Parse an SPDX expression. Return (list of (id, exception or None), error or None)."""
    toks = re.findall(r"\(|\)|[^\s()]+", expr)
    pos = [0]
    ops = ("OR", "AND", "WITH")

    def peek():
        return toks[pos[0]] if pos[0] < len(toks) else None

    def take():
        t = peek()
        pos[0] += 1
        return t

    def term():
        t = take()
        if t is None:
            raise ValueError("the expression ends where an id is needed")
        if t == "(":
            r = expr_()
            if take() != ")":
                raise ValueError("a bracket is not closed")
            return r
        if t == ")" or t in ops:
            raise ValueError("unexpected %r" % t)
        if peek() == "WITH":
            take()
            e = take()
            if e is None or e in ops or e in "()":
                raise ValueError("WITH needs an exception id")
            return [(t, e)]
        return [(t, None)]

    def expr_():
        out = term()
        while peek() in ("OR", "AND"):
            take()
            out = out + term()
        return out

    if not toks:
        return [], "the licence is empty"
    try:
        r = expr_()
        if peek() is not None:
            raise ValueError("unexpected %r" % peek())
    except ValueError as e:
        return [], str(e)
    return r, None


def load_licences():
    """id -> kind: '' (normal), 'test-only' or 'exception'."""
    out = {}
    for line in read_lines(LICENCES):
        parts = line.split()
        out[parts[0]] = parts[1] if len(parts) > 1 else ""
    return out


def licence_problems(expr, allowed, ships):
    ids, err = parse_licence(expr)
    if err:
        return ["licence %r: %s" % (expr, err)]
    probs = []
    for lid, exc in ids:
        kind = allowed.get(lid)
        if kind is None:
            probs.append("licence id %r is not in docs/attribution-licences.txt" % lid)
        elif kind == "exception":
            probs.append("%r is an exception, use it as `<licence> WITH %s`" % (lid, lid))
        elif kind == "test-only" and ships:
            probs.append("licence %r is allowed for test-only rows only" % lid)
        if exc is not None:
            if allowed.get(exc) is None:
                probs.append("exception id %r is not in docs/attribution-licences.txt" % exc)
            elif allowed[exc] != "exception":
                probs.append("%r is not marked `exception` in docs/attribution-licences.txt" % exc)
    return probs


def is_path_row(row):
    return not row["path"].startswith(("crate:", "ext:"))


def rule_ext_dir(f):
    parts = f.split("/")
    base = parts[-1]
    ext = base.rsplit(".", 1)[-1].lower() if "." in base else ""
    if "proptest-regressions" in parts or base.endswith(".proptest-regressions"):
        return False
    if ext in ASSET_EXT:
        return True
    in_dir = [p for p in parts[:-1] if p in ASSET_DIRS]
    if in_dir and not base.startswith(".git"):
        if ext in ("rs", "md") and not any(p in VENDOR_DIRS for p in in_dir):
            return False
        return True
    return False


def crate_dir(rel):
    d = os.path.dirname(os.path.join(ROOT, rel))
    while d.startswith(ROOT) and d != ROOT:
        if os.path.exists(os.path.join(d, "Cargo.toml")):
            return d
        d = os.path.dirname(d)
    return None


def embedded_targets(files):
    """Paths named by include_bytes! and include_str! with a literal path."""
    found = set()
    for f in files:
        if not f.endswith(".rs"):
            continue
        try:
            text = open(os.path.join(ROOT, f), encoding="utf-8").read()
        except (OSError, UnicodeDecodeError):
            continue
        for m in re.finditer(r"include_(?:bytes|str)!\s*\(", text):
            depth, i, in_str = 1, m.end(), False
            while i < len(text) and depth:
                c = text[i]
                if in_str:
                    if c == "\\":
                        i += 1
                    elif c == '"':
                        in_str = False
                elif c == '"':
                    in_str = True
                elif c == "(":
                    depth += 1
                elif c == ")":
                    depth -= 1
                i += 1
            body = text[m.end():i - 1]
            if "$" in body:
                continue
            lits = re.findall(r'"((?:[^"\\]|\\.)*)"', body)
            if "env!" in body:
                if "CARGO_MANIFEST_DIR" not in body:
                    continue
                lits = [x for x in lits if x != "CARGO_MANIFEST_DIR"]
                base = crate_dir(f)
                if base is None:
                    continue
                target = os.path.join(base, "".join(lits).lstrip("/"))
            else:
                target = os.path.join(os.path.dirname(os.path.join(ROOT, f)), "".join(lits))
            target = os.path.normpath(target)
            if target.startswith(ROOT + os.sep) and os.path.isfile(target):
                found.add(os.path.relpath(target, ROOT))
    return found


def header_hits(files):
    hits = []
    for f in files:
        if f.startswith(HEADER_SKIP_PREFIX) or f in HEADER_SKIP_FILES:
            continue
        try:
            with open(os.path.join(ROOT, f), "rb") as fh:
                head = fh.read(HEADER_BYTES)
        except OSError:
            continue
        if b"\0" in head:
            continue
        for line in head.decode("utf-8", errors="replace").splitlines():
            if HEADER_RE.search(line) and not HEADER_OWN.search(line):
                hits.append(f)
                break
    return hits


def table_lines(rows):
    out = ["| Item | Path | Author | Licence | Source | Use |", "|---|---|---|---|---|---|"]
    for r in rows:
        out.append("| %s | `%s` | %s | %s | %s | %s |" % (r["name"], r["path"], r["author"], r["licence"], r["source"], r["use"]))
    return out


def table_block(text):
    lines = text.split("\n")
    for i, l in enumerate(lines):
        if l.startswith("| Item | Path | Author | Licence | Source | Use |"):
            j = i
            while j < len(lines) and lines[j].startswith("|"):
                j += 1
            return lines[i:j]
    return []


def notice_has(notices, name):
    return re.search(r"(?<![\w-])" + re.escape(name) + r"(?![\w-])", notices) is not None


def annotation(file, msg):
    esc = msg.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    f = file.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A").replace(":", "%3A").replace(",", "%2C")
    return "::warning file=%s::%s" % (f, esc)


def under_unknown_dir(p):
    return any(seg in UNKNOWN_DIRS for seg in p.split("/")[:-1])


def main():
    try:
        sys.stdout.reconfigure(errors="backslashreplace")
    except AttributeError:
        pass
    rows = parse_tsv()
    if "--table" in sys.argv:
        print("\n".join(table_lines(rows)))
        return 0

    files = list_files()
    fileset = set(files)
    allowed = load_licences()
    open_lines = read_lines(OPEN)
    open_files = [l.split("\t")[0].strip() for l in open_lines]

    meta_path = os.environ.get("ATTR_METADATA")
    packages = []
    if meta_path and os.path.exists(meta_path):
        packages = json.load(open(meta_path, encoding="utf-8"))["packages"]
    else:
        fail("setup", "ATTR_METADATA is not set: run scripts/check-attribution.sh")
    crate_names = {p["name"] for p in packages}

    # Check 2: the rows themselves.
    seen_keys = set()
    unknown_rows = []
    for r in rows:
        where = "row %r (%s)" % (r["path"], r["name"])
        key = (r["path"], r["name"])
        if key in seen_keys:
            fail("row", "%s: the same path and name are in the table twice" % where)
        seen_keys.add(key)
        if r["path"].startswith("::") or r["name"].startswith("::"):
            fail("row", "%s: a path or name must not start with '::'" % where)
        if not r["name"] or not r["author"]:
            fail("row", "%s: name and author must not be empty" % where)
        if r["use"] not in ("ships", "test"):
            fail("row", "%s: the column use is `ships` or `test`, got %r" % (where, r["use"]))
        ships = r["use"] != "test"
        unknown = r["licence"] == "UNKNOWN" or r["source"] == "UNKNOWN" or r["author"] == "UNKNOWN"
        if not r["licence"]:
            fail("row", "%s: no licence" % where)
        elif r["licence"] != "UNKNOWN":
            for p in licence_problems(r["licence"], allowed, ships):
                fail("row", "%s: %s" % (where, p))
        if not r["source"]:
            fail("row", "%s: no source URL" % where)
        elif r["source"] != "UNKNOWN" and not re.match(r"https?://\S+$", r["source"]):
            fail("row", "%s: the source must be an http(s) URL, got %r" % (where, r["source"]))
        if r["path"].startswith("ext:") and not re.match(r"ext:[A-Za-z0-9._-]+$", r["path"]):
            fail("row", "%s: an ext: id has letters, digits, dot, dash and underscore only" % where)
        if unknown:
            if not (is_path_row(r) and under_unknown_dir(r["path"])):
                fail("row", "%s: UNKNOWN is only allowed for a path under tests/, testdata/ or fixtures/" % where)
            else:
                unknown_rows.append(r)

    # Check 3: stale rows. Also build the matchers.
    matchers = []
    for r in rows:
        p = r["path"]
        if p.startswith("crate:"):
            if p[6:] not in crate_names:
                fail("stale", "row %r: the crate %r is not in cargo metadata" % (p, p[6:]))
        elif not p.startswith("ext:"):
            rx = glob_re(p)
            matchers.append((rx, r))
            if not any(rx.match(f) for f in files):
                fail("stale", "row %r (%s): the path or glob matches no file" % (p, r["name"]))

    # The open list: exact files, no duplicates, each matched by an UNKNOWN row.
    seen = set()
    for o in open_files:
        if re.search(r"[*?\[\]]", o):
            fail("open", "docs/attribution-open.txt: %r is a glob, name the exact file" % o)
        if o in seen:
            fail("open", "docs/attribution-open.txt: %r is listed twice" % o)
        seen.add(o)
        if o not in fileset:
            fail("open", "docs/attribution-open.txt: %r is not a file of the repository (remove the line)" % o)
    unknown_files = {}
    for r in unknown_rows:
        rx = glob_re(r["path"])
        for f in files:
            if rx.match(f):
                unknown_files[f] = r
                if f not in seen:
                    fail("open", "%s: UNKNOWN row %r matches this file but docs/attribution-open.txt does not name it" % (f, r["path"]))
    for o in sorted(seen - set(unknown_files)):
        if o in fileset and not re.search(r"[*?\[\]]", o):
            fail("open", "docs/attribution-open.txt: %r is matched by no UNKNOWN row (remove the line)" % o)
    gha = os.environ.get("GITHUB_ACTIONS") == "true"
    for f in sorted(unknown_files):
        if f in seen:
            r = unknown_files[f]
            msg = "licence, source or author UNKNOWN (%s)" % r["name"]
            warnings.append("%s: %s" % (f, msg))
            if gha:
                warnings.append(annotation(f, msg))

    def row_cover(f):
        return [r for rx, r in matchers if rx.match(f)]

    # Check 1: files that need a row.
    need = {}
    for f in files:
        if rule_ext_dir(f):
            need[f] = "asset file (extension or folder)"
    for f in embedded_targets(files):
        base = f.rsplit("/", 1)[-1]
        ext = base.rsplit(".", 1)[-1] if "." in base else ""
        if ext not in EMBED_SKIP_EXT and base not in EMBED_SKIP_FILES:
            need.setdefault(f, "embedded with include_bytes! or include_str!")
    hits = set(header_hits(files))
    for f in hits:
        need.setdefault(f, "has a copyright, licence or SPDX header")
    for f in sorted(need):
        cover = row_cover(f)
        if not cover:
            fail("missing", "%s: %s, no row in docs/attribution-assets.tsv" % (f, need[f]))
        elif f in hits and all(r["author"].startswith(OWN) for r in cover):
            fail("missing", "%s: has a third-party header but only a row of own work covers it" % f)

    # Check 6: attribution.md, the notices and the licence list.
    try:
        att = open(ATTRIBUTION, encoding="utf-8").read()
    except OSError:
        att = ""
        fail("setup", "missing file: docs/attribution.md")
    try:
        notices = open(NOTICES, encoding="utf-8").read()
    except OSError:
        notices = ""
        fail("setup", "missing file: THIRD_PARTY_NOTICES.txt")
    if table_block(att) != table_lines(rows):
        fail("doc", "the asset table in docs/attribution.md is not the output of scripts/check-attribution.sh --table (paste the new table)")
    for r in rows:
        third = not r["author"].startswith(OWN)
        if third and r["use"] == "ships" and not notice_has(notices, r["name"]):
            fail("notices", "THIRD_PARTY_NOTICES.txt does not name %r (add it to docs/notices-extra.txt and run scripts/notices.sh)" % r["name"])

    # Check 4: Cargo dependencies.
    for p in packages:
        if not p.get("license") and not p.get("license_file"):
            fail("crate", "crate %s %s has no licence in cargo metadata" % (p["name"], p["version"]))
    tree_path = os.environ.get("ATTR_TREE")
    if tree_path and os.path.exists(tree_path):
        shipped = {l.strip() for l in open(tree_path, encoding="utf-8") if l.strip()}
        ours = {p["name"] for p in packages if p.get("source") is None}
        listed = set(re.findall(r"^\s+- (\S+) \S+", notices, re.M))
        for name in sorted(shipped - ours - listed):
            fail("crate", "crate %s ships but is not in THIRD_PARTY_NOTICES.txt (run scripts/notices.sh)" % name)
    else:
        fail("setup", "ATTR_TREE is not set: run scripts/check-attribution.sh")

    for w in warnings:
        print(w if w.startswith("::") else "WARN  " + w)
    if failures:
        by = {}
        for s, t in failures:
            by.setdefault(s, []).append(t)
        print("")
        for s in sorted(by):
            print("FAIL  [%s] %d problem(s)" % (s, len(by[s])))
            for t in by[s]:
                print("      - " + t)
        print("\nattribution check failed: %d problem(s). See docs/dev-guide.md, 'Add an asset'." % len(failures))
        return 1
    print("attribution check passed: %d rows, %d files checked, %d UNKNOWN row(s) covering %d file(s)." % (len(rows), len(files), len(unknown_rows), len(unknown_files)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
