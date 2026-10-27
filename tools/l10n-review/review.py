"""Joins the parsed locales, the state and the usage index. No HTTP here."""
import os
import re
import threading

import apply as applymod
import ftl
import prompt as promptmod
import terms
from state import ReviewState, default_state_path, hash_text, msg_id, STATUSES

EXAMPLES = {
    "count": "3", "n": "3", "total": "12", "used": "2", "step": "4", "id": "7", "index": "2",
    "name": "Hedge 1", "names": "Hedge 1, Path", "version": "0.1.0", "channel": "alpha", "kind": "line",
    "error": "disk is full", "e": "disk is full", "path": "/Users/ann/garden.kad",
    "file": "garden.kad", "key": "Ctrl+Z", "layer": "Planting", "signature": "a1b2c3", "free": "2",
    "blocked": "1", "px": "16", "mm": "250", "m": "12.5", "area": "42 m2",
}


def example_for(var):
    if var in EXAMPLES:
        return EXAMPLES[var]
    for k, v in EXAMPLES.items():
        if var.endswith(k):
            return v
    return "..."


class Review:
    """One target locale of one repo."""

    def __init__(self, repo, target, source="en-US", state_path=None, claude_bin="claude"):
        self.repo = os.path.abspath(repo)
        self.target = target
        self.source = source
        self.claude_bin = claude_bin
        self.locales_root = os.path.join(self.repo, "crates", "kadakas-l10n", "locales")
        self.lock = threading.RLock()
        self._sig = None
        self.state = ReviewState(state_path or default_state_path(target), target)
        if self.state.glossary() is None:
            self.state.data["glossary"] = [dict(g) for g in promptmod.SEED_GLOSSARY]
            # Seeding is saved only with the first real change.
        self.src = {}
        self.tgt = {}
        self.usage = None
        self.reload()

    def target_dir(self):
        return os.path.join(self.locales_root, self.target)

    def _signature(self):
        sig = []
        for d in (os.path.join(self.locales_root, self.source), self.target_dir()):
            try:
                for fn in sorted(os.listdir(d)):
                    if fn.endswith(".ftl"):
                        st = os.stat(os.path.join(d, fn))
                        sig.append((d, fn, st.st_mtime_ns, st.st_size))
            except OSError:
                pass
        return sig

    def reload(self, force=True):
        """Parse the locales again. The new maps are built first and swapped in under the lock."""
        sig = self._signature()
        if not force and sig == self._sig:
            return
        src = ftl.parse_locale(os.path.join(self.locales_root, self.source))
        tgt = ftl.parse_locale(self.target_dir())
        parity = ftl.key_parity(src, tgt)
        with self.lock:
            self.src, self.tgt, self.parity = src, tgt, parity
            self._build_order()
            self._terms_cache = None
            self._sig = sig

    def _build_order(self):
        self.order = []      # ids in file/key order
        self.index = {}      # id -> dict(file, key, en, tgt, pos)
        files = sorted(set(self.src) | set(self.tgt))
        for fn in files:
            sf, tf = self.src.get(fn), self.tgt.get(fn)
            tmap = {m.key: m for m in (tf.messages if tf else [])}
            seen = set()
            keys = []
            for m in (sf.messages if sf else []):
                keys.append((m.key, m, tmap.get(m.key)))
                seen.add(m.key)
            for m in (tf.messages if tf else []):
                if m.key not in seen:
                    keys.append((m.key, None, m))
            for pos, (key, en, tg) in enumerate(keys):
                mid = msg_id(fn, key)
                self.index[mid] = {"file": fn, "key": key, "en": en, "tgt": tg, "pos": pos, "keys": keys}
                self.order.append(mid)

    def build_usage(self):
        import usage
        keys = {self.index[m]["key"] for m in self.order}
        self.usage = usage.UsageIndex(self.repo, keys)

    @staticmethod
    def locales_available(repo, source="en-US"):
        root = os.path.join(repo, "crates", "kadakas-l10n", "locales")
        try:
            return sorted(d for d in os.listdir(root)
                          if d != source and os.path.isdir(os.path.join(root, d)))
        except OSError:
            return []

    def _msg_dict(self, m, with_examples=False):
        if m is None:
            return None
        d = {"raw": m.raw, "comment": m.comment, "variables": m.variables, "selector": m.selector,
             "variants": m.variants, "attributes": m.attributes, "start": m.start, "end": m.end}
        if with_examples:
            d["examples"] = {v: example_for(v) for v in m.variables}
        return d

    def item(self, mid, brief=False):
        with self.lock:
            return self._item(mid, brief)

    def _item(self, mid, brief=False):
        info = self.index[mid]
        st = self.state.view(mid)
        tg = info["tgt"]
        hl = st["highlights"]
        stale = False
        if hl and (tg is None or st["text_hash"] != hash_text(tg.raw)):
            hl, stale = [], True
        fn = info["file"]
        group_en = self.src[fn].group if fn in self.src else ""
        parity = ("missing" if tg is None else "extra" if info["en"] is None else "ok")
        problems = []
        if info["en"] is not None and tg is not None:
            if set(info["en"].variables) != set(tg.variables):
                problems.append("variables differ")
            if ftl.structure_signature(info["en"].raw)[:2] != ftl.structure_signature(tg.raw)[:2]:
                problems.append("selector differs")
        d = {"id": mid, "file": fn, "key": info["key"], "pos": info["pos"],
             "en": self._msg_dict(info["en"], True), "tr": self._msg_dict(tg),
             "parity": parity, "problems": problems,
             "status": st["status"], "comment": st["comment"], "highlights": hl,
             "highlights_stale": stale, "suggestions": st["suggestions"]}
        if not brief:
            keys = info["keys"]
            p = info["pos"]
            d["neighbours"] = [{"key": k, "en": (e.raw if e else ""), "tr": (t.raw if t else ""),
                                "here": k == info["key"]}
                               for k, e, t in keys[max(0, p - 3):p + 4]]
            d["group_en"] = group_en
            d["group_tr"] = self.tgt[fn].group if fn in self.tgt else ""
        return d

    def list(self):
        with self.lock:
            return self._list()

    def _list(self):
        files = []
        for fn in sorted(set(self.src) | set(self.tgt)):
            files.append({"name": fn, "group": self.src[fn].group if fn in self.src else "",
                          "group_tr": self.tgt[fn].group if fn in self.tgt else ""})
        items = [self._item(m, brief=True) for m in self.order]
        counts = {s: 0 for s in STATUSES}
        for it in items:
            counts[it["status"]] += 1
        return {"target": self.target, "source": self.source,
                "locales": self.locales_available(self.repo, self.source),
                "files": files, "messages": items, "counts": counts,
                "total": len(items), "parity": {
                    "missing": [{"file": f, "key": k} for f, k in self.parity["missing"]],
                    "extra": [{"file": f, "key": k} for f, k in self.parity["extra"]],
                    "missing_files": self.parity["missing_files"],
                    "extra_files": self.parity["extra_files"]},
                "glossary": self.state.glossary(), "statuses": STATUSES,
                "claude": bool(__import__("shutil").which(self.claude_bin))}

    # prompt
    def validate(self, mid, text):
        """Error text for a suggestion, or None when it is safe to apply."""
        info = self.index[mid]
        if info["tgt"] is None or info["en"] is None:
            return "The message is missing in one language."
        if text != text.strip():
            return "The text has a leading or trailing space or line break."
        return applymod.check_replacement(info["tgt"], info["en"], text)

    def add_suggestion(self, mid, text, source="owner", why=""):
        text = text.strip("\n")
        info = self.index[mid]
        base = hash_text(info["tgt"].raw) if info["tgt"] else None
        en_h = hash_text(info["en"].raw) if info["en"] else None
        return self.state.add_suggestion(mid, text, source, why, self.validate(mid, text), base, en_h)

    def edit_suggestion(self, mid, sid, text):
        text = text.strip("\n")
        self.state.edit_suggestion(mid, sid, text, self.validate(mid, text))

    def suggest(self, mid, model="sonnet", timeout=120, more=False, cancel=None):
        """Ask Claude. Stores the alternatives as suggestions (invalid ones with the reason)."""
        avoid = []
        if more:
            avoid = [s["text"] for s in self.state.view(mid)["suggestions"] if s["state"] == "rejected"]
        prompt = self.make_prompt(mid, avoid)
        alts = promptmod.ask_claude(prompt, self.claude_bin, model, timeout, cancel)
        added = []
        for a in alts:
            s = self.add_suggestion(mid, a["text"], "claude:" + model, a["why"])
            if s:
                added.append(s)
        return added

    def make_prompt(self, mid, avoid=None):
        d = self.item(mid)
        if d["en"] is None:
            raise ValueError("This key has no English message.")
        neigh = [n for n in d["neighbours"] if not n["here"]]
        return promptmod.build_prompt(self.target, d["en"], d["tr"], d["highlights"], d["comment"],
                                      neigh, self.state.glossary() or [], avoid)

    # terms
    def term_report(self):
        if self._terms_cache is None:
            pairs = [(m, i["en"].raw if i["en"] else "", i["tgt"].raw if i["tgt"] else "")
                     for m in self.order for i in [self.index[m]] if i["en"]]
            self._terms_cache = {
                "heuristic": True,
                "inconsistent": terms.find_inconsistent(pairs),
                "glossary": terms.glossary_warnings(pairs, self.state.glossary() or [])}
        return self._terms_cache

    # apply
    def accepted_items(self):
        items = []
        for mid, sug in self.state.accepted():
            if mid in self.index and self.index[mid]["tgt"] is not None:
                i = self.index[mid]
                items.append({"id": mid, "file": i["file"], "key": i["key"], "text": sug["text"],
                              "base_hash": sug.get("base_hash"), "en_hash": sug.get("en_hash")})
        return items

    def apply_preview(self):
        self.reload()
        changes = applymod.plan(self.target_dir(), self.src, self.accepted_items())
        dirty = applymod.git_dirty(self.repo, self.target_dir())
        return {"changes": changes, "dirty": dirty, "dirty_unknown": dirty is None,
                "digest": applymod.digest(changes)}

    def apply(self, expect, confirm_dirty=False):
        """Write what the preview showed. `expect` is the digest of the preview. Any difference refuses."""
        with self.lock:
            pre = self.apply_preview()
            if pre["digest"] != expect:
                return {"refused": "changed", **pre}
            if (pre["dirty"] or pre["dirty_unknown"]) and not confirm_dirty:
                return {"refused": "dirty", **pre}
            outcome = applymod.write_changes(self.target_dir(), self.src, pre["changes"])
            for c in pre["changes"]:
                c["error"] = outcome[c["id"]]
            done = [c["id"] for c in pre["changes"] if not c["error"]]
            self.state.mark_fixed(done)
            self.reload()
            return {"changes": pre["changes"], "applied": done, "dirty": pre["dirty"]}

    # export
    def export_markdown(self):
        L = ["# Translation review: %s" % self.target, ""]
        n = 0
        for mid in self.order:
            st = self.state.view(mid)
            if st["status"] == "unreviewed":
                continue
            n += 1
            it = self.item(mid, brief=True)
            L.append("## %s `%s`" % (it["file"], it["key"]))
            L.append("")
            L.append("- Status: %s" % st["status"])
            L.append("- English: %s" % _one(it["en"]["raw"] if it["en"] else ""))
            L.append("- Current: %s" % _one(it["tr"]["raw"] if it["tr"] else "(missing)"))
            if it["highlights"]:
                L.append("- Weird words: " + ", ".join("\"%s\"" % h["text"] for h in it["highlights"]))
            if st["comment"]:
                L.append("- Comment: %s" % _one(st["comment"]))
            for s in st["suggestions"]:
                L.append("- Suggestion (%s, %s): %s" % (s["source"], s["state"], _one(s["text"])))
            L.append("")
        if n == 0:
            L.append("No flagged messages.")
            L.append("")
        gl = self.state.glossary() or []
        if gl:
            L.append("## Glossary")
            L.append("")
            for g in gl:
                L.append("- %s -> %s%s" % (g["en"], g["tr"], (" (%s)" % g["note"]) if g["note"] else ""))
            L.append("")
        return "\n".join(L)


def _one(s):
    return re.sub(r"\s*\n\s*", " / ", s.strip())
