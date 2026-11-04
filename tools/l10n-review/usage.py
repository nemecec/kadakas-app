"""Index of where message keys appear in the Rust sources."""
import os
import re

LIT_RE = re.compile(r'"([a-z][a-z0-9]*(?:-[a-z0-9]+)*)"')
# format!("prefix-{x}-suffix") style key names built at run time
DYN_RE = re.compile(r'"([a-z][a-z0-9-]*-)\{[^}"]*\}([a-z0-9-]*)"')
SKIP_DIRS = {"target", "target-shared", ".git", "node_modules"}


class UsageIndex:
    def __init__(self, repo, keys):
        self.repo = repo
        self.keys = set(keys)
        self.exact = {}     # key -> [(relpath, line)]
        self.dynamic = []   # (prefix, suffix, relpath, line)
        self._lines = {}
        self.files = 0
        self._build()

    def _build(self):
        root = os.path.join(self.repo, "crates")
        for dirpath, dirs, files in os.walk(root):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for fn in files:
                if not fn.endswith(".rs"):
                    continue
                full = os.path.join(dirpath, fn)
                rel = os.path.relpath(full, self.repo)
                try:
                    with open(full, encoding="utf-8", errors="replace") as f:
                        lines = f.read().split("\n")
                except OSError:
                    continue
                self.files += 1
                for no, line in enumerate(lines, 1):
                    if '"' not in line:
                        continue
                    for m in LIT_RE.finditer(line):
                        k = m.group(1)
                        if k in self.keys:
                            self.exact.setdefault(k, []).append((rel, no))
                    for m in DYN_RE.finditer(line):
                        if len(m.group(1)) >= 8:
                            self.dynamic.append((m.group(1), m.group(2), rel, no))

    def lookup(self, key, limit=5, context=1):
        """Up to `limit` uses of a key, each with a snippet of 2*context+1 lines."""
        hits = [(p, n, False) for p, n in self.exact.get(key, [])]
        total = len(hits)
        if len(hits) < limit:
            for pre, suf, p, n in self.dynamic:
                if key.startswith(pre) and key.endswith(suf) and len(key) > len(pre) + len(suf):
                    hits.append((p, n, True))
        total = len(hits)
        out = []
        for p, n, dyn in hits[:limit]:
            out.append({"path": p, "line": n, "dynamic": dyn,
                        "snippet": self._snippet(p, n, context)})
        return {"total": total, "uses": out}

    def _snippet(self, rel, line, context):
        lines = self._lines.get(rel)
        if lines is None:
            try:
                with open(os.path.join(self.repo, rel), encoding="utf-8", errors="replace") as f:
                    lines = f.read().split("\n")
            except OSError:
                lines = []
            self._lines[rel] = lines
        lo, hi = max(1, line - context), min(len(lines), line + context)
        return "\n".join("%d: %s" % (i, lines[i - 1]) for i in range(lo, hi + 1))
