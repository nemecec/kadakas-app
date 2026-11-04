"""Joins the parsed locales, the state and the usage index. No HTTP here."""
import os
import re
import threading
import time

import apply as applymod
import evaluator
import ftl
import prompt as promptmod
import samples as samplemod
import terms
from state import ReviewState, StateError, default_state_path, hash_text, msg_id, STATUSES

def example_for(var):
    return samplemod.default_sample(var)


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
            self.bundles = {self.source: evaluator.Bundle(self.source, src), self.target: evaluator.Bundle(self.target, tgt)}
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
        d = {"raw": m.raw, "hash": hash_text(m.raw), "comment": m.comment, "variables": m.variables, "selector": m.selector,
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
        sugs = []
        en_hash = hash_text(info["en"].raw) if info["en"] else None
        for s in st["suggestions"]:
            loc = s.get("locale", self.target)
            cur = info["en"] if loc == self.source else tg
            s = dict(s, locale=loc)
            s["stale_en"] = bool(loc != self.source and s.get("en_hash") and en_hash and s["en_hash"] != en_hash)
            s["stale_base"] = bool(s.get("base_hash") and cur is not None and s["base_hash"] != hash_text(cur.raw))
            sugs.append(s)
        chosen = st.get("chosen")
        if not any(s["id"] == chosen and s["state"] == "pending" for s in sugs):
            valid = [s for s in sugs if s["state"] == "pending" and not s.get("error") and not s["stale_base"]]
            chosen = valid[-1]["id"] if valid else None
        d = {"id": mid, "file": fn, "key": info["key"], "pos": info["pos"],
             "en": self._msg_dict(info["en"], True), "tr": self._msg_dict(tg),
             "parity": parity, "problems": problems,
             "status": st["status"], "comment": st["comment"], "highlights": hl,
             "highlights_stale": stale, "suggestions": sugs, "chosen": chosen, "en_change": st.get("en_change")}
        if not brief:
            d["chips"] = self.chips(info, self.target)
            d["chips_en"] = self.chips(info, self.source)
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
                "glossary": self.state.glossary(), "statuses": STATUSES, "samples": self.state.samples(),
                "claude": bool(__import__("shutil").which(self.claude_bin))}

    # prompt
    def chips(self, info, locale=None):
        """Exact placeables the owner may insert while editing: the variables, then the
        non-variable placeables (terms, literals, functions) of the current text."""
        locale = locale or self.target
        out = []
        en, tg = info["en"], info["tgt"]
        cur = en if locale == self.source else tg
        base = en if en else tg
        for v in (base.value_variables if base else []):
            out.append("{ $%s }" % v)
        for p in ftl.placeables(cur.raw)[0] if cur else []:
            chip = "{ %s }" % p
            if chip not in out:
                out.append(chip)
        return out

    def call_sites(self, key):
        if self.usage is None:
            return ""
        u = self.usage.lookup(key)
        if not u["uses"]:
            return "No use found in the Rust sources."
        return "Used in: " + ", ".join("%s:%d" % (x["path"], x["line"]) for x in u["uses"]) + "."

    def _current(self, info, locale):
        return info["en"] if locale == self.source else info["tgt"]

    def owner_edit(self, mid, text, base_hash, accept, locale=None):
        """Store the owner's edit of the current text as a suggestion. Returns (item, unchanged)."""
        locale = locale or self.target
        info = self.index[mid]
        cur = self._current(info, locale)
        if info["tgt"] is None or info["en"] is None or cur is None:
            raise StateError("The message is missing in one language.")
        text = text.strip("\n")
        if text == cur.raw:
            return self.item(mid), True
        if base_hash and base_hash != hash_text(cur.raw):
            raise StateError("The text changed in the file since you opened the editor. Cancel and edit again.")
        err = self.validate(mid, text, locale)
        if err:
            raise StateError(err)
        sug = self.add_suggestion(mid, text, "owner edit", "", locale)
        if sug is None:   # the same text is already pending
            sug = next(s for s in self.state.view(mid)["suggestions"]
                       if s["text"] == text and s["state"] == "pending" and s.get("locale", self.target) == locale)
        if accept:
            self.state.decide_suggestion(mid, sug["id"], "accept")
        return self.item(mid), False

    def validate_full(self, mid, text, locale=None):
        """{error, warnings} for a text in a locale. English edits get warnings for removed variables."""
        locale = locale or self.target
        err = self.validate(mid, text, locale)
        warnings = []
        info = self.index[mid]
        if locale == self.source and info["en"] is not None:
            warnings = applymod.variable_warnings(info["en"].value_variables, text)
        return {"error": err, "warnings": warnings}

    def validate(self, mid, text, locale=None):
        """Error text for a suggestion, or None when it is safe to apply."""
        locale = locale or self.target
        info = self.index[mid]
        if info["tgt"] is None or info["en"] is None:
            return "The message is missing in one language."
        if text != text.strip():
            return "The text has a leading or trailing space or line break."
        if locale == self.source:
            return applymod.check_replacement(info["en"], info["en"], text, source_edit=True,
                                              call_sites=self.call_sites(info["key"]))
        return applymod.check_replacement(info["tgt"], info["en"], text)

    def add_suggestion(self, mid, text, source="owner", why="", locale=None):
        locale = locale or self.target
        text = text.strip("\n")
        info = self.index[mid]
        cur = self._current(info, locale)
        base = hash_text(cur.raw) if cur else None
        en_h = hash_text(info["en"].raw) if info["en"] else None
        return self.state.add_suggestion(mid, text, source, why, self.validate(mid, text, locale), base, en_h, locale)

    def edit_suggestion(self, mid, sid, text):
        text = text.strip("\n")
        sug = next((s for s in self.state.view(mid)["suggestions"] if s["id"] == sid), None)
        locale = sug.get("locale", self.target) if sug else self.target
        self.state.edit_suggestion(mid, sid, text, self.validate(mid, text, locale))

    def suggest(self, mid, model="sonnet", timeout=120, more=False, cancel=None, locale=None):
        """Ask Claude. Stores the alternatives as suggestions (invalid ones with the reason)."""
        locale = locale or self.target
        avoid = []
        if more:
            avoid = [s["text"] for s in self.state.view(mid)["suggestions"]
                     if s["state"] == "rejected" and s.get("locale", self.target) == locale]
        prompt = self.make_prompt(mid, avoid, locale)
        alts = promptmod.ask_claude(prompt, self.claude_bin, model, timeout, cancel)
        added = []
        for a in alts:
            s = self.add_suggestion(mid, a["text"], "claude:" + model, a["why"], locale)
            if s:
                added.append(s)
        return added

    def make_prompt(self, mid, avoid=None, locale=None):
        locale = locale or self.target
        d = self.item(mid)
        if d["en"] is None:
            raise ValueError("This key has no English message.")
        neigh = [n for n in d["neighbours"] if not n["here"]]
        if locale == self.source:
            return promptmod.build_english_prompt(self.target, d["en"], d["tr"], d["comment"], neigh,
                                                  self.state.glossary() or [], avoid)
        return promptmod.build_prompt(self.target, d["en"], d["tr"], d["highlights"], d["comment"],
                                      neigh, self.state.glossary() or [], avoid)

    # previews with sample values
    def preview_item(self, mid):
        info = self.index[mid]
        owner = self.state.samples()
        out = {}
        for key, loc, m in (("en", self.source, info["en"]), ("tr", self.target, info["tgt"])):
            out[key] = evaluator.previews(m.raw, self.bundles[loc], owner) if m is not None and "{" in m.raw else None
        return out

    def preview_text(self, mid, locale, text):
        return evaluator.previews(text, self.bundles[locale], self.state.samples())

    def sample_rows(self, mid):
        """Variables of a message with their sample values."""
        info = self.index[mid]
        owner = self.state.samples()
        names = []
        for m, loc in ((info["en"], self.source), (info["tgt"], self.target)):
            if m is not None:
                b = self.bundles[loc]
                for n in evaluator.collect_vars(b.pattern(m.raw), b):
                    if n not in names:
                        names.append(n)
        return [{"name": n, "value": owner.get(n, ""), "default": samplemod.default_sample(n)} for n in names]

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
    def _dirs(self):
        return {self.source: os.path.join(self.locales_root, self.source), self.target: self.target_dir()}

    def accepted_items(self):
        items = []
        for mid, sug in self.state.accepted():
            if mid not in self.index:
                continue
            i = self.index[mid]
            loc = sug.get("locale", self.target)
            if i["tgt"] is None or i["en"] is None:
                continue
            items.append({"id": mid + "|" + loc, "mid": mid, "locale": loc, "file": i["file"], "key": i["key"],
                          "text": sug["text"], "base_hash": sug.get("base_hash"), "en_hash": sug.get("en_hash")})
        return items

    def apply_preview(self):
        pre = self._preview()
        pre.pop("_files")
        return pre

    def _preview(self):
        self.reload()
        files = {self.source: self.src, self.target: self.tgt}
        changes = applymod.plan(self._dirs(), files, self.source, self.accepted_items(), self.call_sites)
        dirty = applymod.git_dirty(self.repo, list(self._dirs().values()))
        return {"changes": changes, "dirty": dirty, "dirty_unknown": dirty is None,
                "digest": applymod.digest(changes), "_files": files}

    def apply(self, expect, confirm_dirty=False):
        """Write what the preview showed. `expect` is the digest of the preview. Any difference refuses."""
        with self.lock:
            pre = self._preview()
            files = pre.pop("_files")
            if pre["digest"] != expect:
                return {"refused": "changed", **pre}
            if (pre["dirty"] or pre["dirty_unknown"]) and not confirm_dirty:
                return {"refused": "dirty", **pre}
            outcome = applymod.write_changes(self._dirs(), files, self.source, pre["changes"])
            for c in pre["changes"]:
                c["error"] = outcome[c["id"]]
            done = [c["id"] for c in pre["changes"] if not c["error"]]
            today = time.strftime("%Y-%m-%d")
            by_mid = {}
            for c in pre["changes"]:
                if not c["error"]:
                    by_mid.setdefault(c["mid"], []).append(c)
            for mid, cs in by_mid.items():
                en = next((c for c in cs if c["locale"] == self.source), None)
                self.state.finish_apply(mid, {c["locale"] for c in cs}, self.source,
                                        en["old_raw"] if en else "", en["new_raw"] if en else "", today)
            self.reload()
            return {"changes": pre["changes"], "applied": done, "dirty": pre["dirty"]}

    # export
    def export_markdown(self):
        L = ["# Translation review: %s" % self.target, ""]
        n = 0
        ok_by_file = {}
        for mid in self.order:
            st = self.state.view(mid)
            if st["status"] == "ok":
                f = self.index[mid]["file"]
                ok_by_file[f] = ok_by_file.get(f, 0) + 1
            if st["status"] in ("unreviewed", "ok"):
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
        if ok_by_file:
            L.append("## Marked OK")
            L.append("")
            L.append("%d messages in total." % sum(ok_by_file.values()))
            for f in sorted(ok_by_file):
                L.append("- %s: %d" % (f, ok_by_file[f]))
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
