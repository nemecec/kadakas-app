"""Helper of scripts/check-attribution.sh (PLAT-21). Run it with python3 -I.

Reads the table docs/attribution-assets.tsv and checks it against the repository.
Standard library only. See docs/dev-guide.md, "Add an asset".

Environment (the shell script and the tests set these):
  ATTR_ROOT        repository root (default: the parent of this folder)
  ATTR_TSV         asset table
  ATTR_LICENCES    allow-list of licence ids
  ATTR_OPEN        list of rows that may have the licence UNKNOWN
  ATTR_ATTRIBUTION docs/attribution.md
  ATTR_NOTICES     THIRD_PARTY_NOTICES.txt
  ATTR_METADATA    output of cargo metadata (JSON file)
  ATTR_TREE        crate names that ship, one per line (from cargo tree)
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
    "svg", "png", "jpg", "jpeg", "gif", "ico", "icns", "ttf", "otf", "woff", "woff2",
    "ftl", "csv", "json", "geojson", "dxf", "dwg", "ifc", "bcf", "bcfzip", "xsd",
    "wasm", "wat", "kad", "plist",
}
# Every file in a folder with one of these names is an asset (except Rust and Markdown files).
ASSET_DIRS = {
    "assets", "data", "testdata", "fixtures", "locales", "profiles", "icons",
    "vendor", "third_party", "third-party",
}
# include_bytes! and include_str! targets with these extensions are our own code or text.
EMBED_SKIP_EXT = {"rs", "md", "py"}
EMBED_SKIP_FILES = {"THIRD_PARTY_NOTICES.txt"}
# Header scan (check 5): files that are ours by definition.
HEADER_SKIP_PREFIX = ("docs/", "scripts/", "target")
HEADER_SKIP_FILES = {"LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.txt", "about.hbs"}
HEADER_RE = re.compile(rb"Copyright \(c\)|SPDX-License-Identifier")
HEADER_BYTES = 4096
OWN = "Kadakas project"

failures = []
warnings = []


def fail(section, text):
    failures.append((section, text))


def read_lines(p):
    if not os.path.exists(p):
        fail("setup", "missing file: %s" % os.path.relpath(p, ROOT))
        return []
    out = []
    with open(p, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.strip() and not line.lstrip().startswith("#"):
                out.append(line)
    return out


def list_files():
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT, capture_output=True, check=True,
    ).stdout.decode("utf-8")
    files = [f for f in out.split("\0") if f and os.path.isfile(os.path.join(ROOT, f))]
    return sorted(set(files))


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


def parse_tsv():
    rows = []
    for n, line in enumerate(read_lines(TSV), 1):
        cols = line.split("\t")
        if len(cols) != 5:
            fail("table", "docs/attribution-assets.tsv: a row needs 5 tab separated columns, got %d: %r" % (len(cols), line[:70]))
            continue
        cols = [c.strip() for c in cols]
        rows.append(dict(path=cols[0], name=cols[1], author=cols[2], licence=cols[3], source=cols[4]))
    return rows


def licence_ids(expr):
    expr = expr.replace("(", " ").replace(")", " ")
    return [t for t in re.split(r"\s+(?:OR|AND|WITH)\s+|\s+", expr.strip()) if t and t not in ("OR", "AND", "WITH")]


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
    if any(p in ASSET_DIRS for p in parts[:-1]) and ext not in ("rs", "md") and base not in (".gitignore", ".gitkeep"):
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
        if HEADER_RE.search(head):
            hits.append(f)
    return hits


def main():
    table_mode = "--table" in sys.argv
    rows = parse_tsv()
    if table_mode:
        print("| Item | Path | Author | Licence | Source |")
        print("|---|---|---|---|---|")
        for r in rows:
            print("| %s | `%s` | %s | %s | %s |" % (r["name"], r["path"], r["author"], r["licence"], r["source"]))
        return 0

    files = list_files()
    allowed = set(read_lines(LICENCES))
    allowed = {a.split()[0] for a in allowed}
    open_rows = {l.split("\t")[0].strip() for l in read_lines(OPEN)}

    # Metadata of Cargo.
    meta_path = os.environ.get("ATTR_METADATA")
    packages = []
    if meta_path and os.path.exists(meta_path):
        packages = json.load(open(meta_path, encoding="utf-8"))["packages"]
    else:
        fail("setup", "ATTR_METADATA is not set: run scripts/check-attribution.sh")
    crate_names = {p["name"] for p in packages}

    # Check 2: the rows themselves.
    unknown_rows = []
    seen_open = set()
    for r in rows:
        where = "row %r (%s)" % (r["path"], r["name"])
        if not r["name"] or not r["author"]:
            fail("row", "%s: name and author must not be empty" % where)
        unknown = r["licence"] == "UNKNOWN"
        if not r["licence"]:
            fail("row", "%s: no licence" % where)
        elif not unknown:
            for lid in licence_ids(r["licence"]):
                if lid not in allowed:
                    fail("row", "%s: licence id %r is not in docs/attribution-licences.txt" % (where, lid))
        if not r["source"]:
            fail("row", "%s: no source URL" % where)
        elif r["source"] != "UNKNOWN" and not re.match(r"https?://\S+$", r["source"]):
            fail("row", "%s: the source must be an http(s) URL, got %r" % (where, r["source"]))
        elif r["source"] == "UNKNOWN" and not unknown:
            fail("row", "%s: source UNKNOWN is only valid together with licence UNKNOWN" % where)
        if unknown or r["source"] == "UNKNOWN":
            if r["path"] in open_rows:
                warnings.append("%s: licence or source UNKNOWN, listed in docs/attribution-open.txt" % where)
                seen_open.add(r["path"])
            else:
                fail("row", "%s: licence UNKNOWN and not listed in docs/attribution-open.txt" % where)
            unknown_rows.append(r)
    for p in sorted(open_rows - seen_open):
        fail("open", "docs/attribution-open.txt lists %r but no UNKNOWN row has that path (remove the line)" % p)

    # Check 3: stale rows. Also build the matchers.
    matchers = []
    for r in rows:
        p = r["path"]
        if p.startswith("crate:"):
            if p[6:] not in crate_names:
                fail("stale", "row %r: the crate %r is not in cargo metadata" % (p, p[6:]))
        elif p.startswith("ext:"):
            if not re.match(r"ext:[A-Za-z0-9._-]+$", p):
                fail("row", "row %r: an ext: id has letters, digits, dot, dash and underscore only" % p)
        else:
            rx = glob_re(p)
            matchers.append((rx, p))
            if not any(rx.match(f) for f in files):
                fail("stale", "row %r (%s): the path or glob matches no file" % (p, r["name"]))

    def covered(f):
        return any(rx.match(f) for rx, _ in matchers)

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
    # Check 5: third-party header.
    for f in header_hits(files):
        need[f] = "has a Copyright (c) or SPDX-License-Identifier header"
    for f in sorted(need):
        if not covered(f):
            fail("missing", "%s: %s, no row in docs/attribution-assets.tsv" % (f, need[f]))

    # Check 6: attribution.md and the licence list.
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
    said = set()
    for r in rows:
        for what in ("name", "path"):
            if r[what] not in att and (what, r[what]) not in said:
                said.add((what, r[what]))
                fail("doc", "docs/attribution.md does not mention the %s %r (run scripts/check-attribution.sh --table)" % (what, r[what]))
        third = not r["author"].startswith(OWN) and r["licence"] != "UNKNOWN"
        test_only = r["path"].startswith(("tests/", "ext:")) or "/tests/" in r["path"]
        if third and not test_only and r["name"] not in notices:
            fail("notices", "THIRD_PARTY_NOTICES.txt does not mention %r (add it to docs/notices-extra.txt and run scripts/notices.sh)" % r["name"])

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
        print("WARN  " + w)
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
    print("attribution check passed: %d rows, %d files checked, %d open warning(s)." % (len(rows), len(files), len(warnings)))
    return 0


sys.exit(main())
