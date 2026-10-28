"""Heuristic term consistency. Not exact: it only counts words that occur together."""
import math
import re
from collections import Counter

WORD_RE = re.compile(r"[^\W\d_]{4,}", re.U)
STOP = set("""this that with from have will your when then than they them their there these those
into onto over under after before while where which what also only each other some more most such
cannot does done not are was were been being can could would should must may might
you the and for are but all any one two new""".split())
STEM = 5


def _strip(raw):
    # remove placeholders and variants syntax, so only words stay
    raw = re.sub(r"\{[^{}]*->", " ", raw)
    raw = re.sub(r"\$[A-Za-z][\w-]*", " ", raw)
    raw = re.sub(r"\*?\[[^\]]*\]", " ", raw)
    return raw


def words(raw):
    return [w.lower() for w in WORD_RE.findall(_strip(raw))]


def en_words(raw):
    return {w for w in words(raw) if w not in STOP}


def tr_stems(raw):
    return {w[:STEM] for w in words(raw)}


def _close(a, b):
    """Stems that are one form of the same word: same first 4 letters or edit distance 1."""
    if a[:4] == b[:4]:
        return True
    if abs(len(a) - len(b)) > 1:
        return False
    if len(a) == len(b):
        return sum(x != y for x, y in zip(a, b)) <= 1
    short, long_ = (a, b) if len(a) < len(b) else (b, a)
    return any(long_[:i] + long_[i + 1:] == short for i in range(len(long_)))


def _merge(cands):
    """Merge close stems. cands: [(dice, stem, count)] best first. Returns [(stem, [stems])]."""
    groups = []
    for dice, stem, c in cands:
        for g in groups:
            if any(_close(stem, x) for x in g[1]):
                g[1].append(stem)
                break
        else:
            groups.append((stem, [stem]))
    return groups


def find_inconsistent(pairs, min_msgs=3, limit=200):
    """pairs: list of (id, english raw, target raw). Returns list of English words with several renderings."""
    en_sets = []
    tr_sets = []
    ids = []
    for mid, en, tr in pairs:
        if not tr:
            continue
        ids.append(mid)
        en_sets.append(en_words(en))
        tr_sets.append(tr_stems(tr))
    n_en = Counter()
    n_tr = Counter()
    by_en = {}
    for i, (e, t) in enumerate(zip(en_sets, tr_sets)):
        for w in e:
            n_en[w] += 1
            by_en.setdefault(w, []).append(i)
        for s in t:
            n_tr[s] += 1
    results = []
    for w, idxs in by_en.items():
        if len(idxs) < min_msgs or len(idxs) > 400:
            continue
        co = Counter()
        for i in idxs:
            for s in tr_sets[i]:
                co[s] += 1
        cands = []
        for s, c in co.items():
            if c < 2:
                continue
            dice = 2.0 * c / (len(idxs) + n_tr[s])
            if dice >= 0.25:
                cands.append((dice, s, c))
        cands.sort(reverse=True)
        cands = cands[:8]
        if len(cands) < 2:
            continue
        # two renderings count as different when many messages have one without the other
        groups = []
        for lead, stems in _merge(cands):
            ms = [i for i in idxs if any(x in tr_sets[i] for x in stems)]
            groups.append((lead, ms))
        groups.sort(key=lambda g: -len(g[1]))   # the widest rendering first
        top = groups[0]
        others = []
        for s, ms in groups[1:]:
            top_stems = [x for _, x, _ in cands if _close(x, top[0])] or [top[0]]
            only = [i for i in ms if not any(x in tr_sets[i] for x in top_stems)]
            if len(only) >= 2:
                others.append((s, only))
        if not others:
            continue
        renderings = [{"stem": top[0], "count": len(top[1]), "ids": [ids[i] for i in top[1][:5]]}]
        for s, only in others:
            renderings.append({"stem": s, "count": len(only), "ids": [ids[i] for i in only[:5]]})
        score = sum(r["count"] for r in renderings[1:]) * math.log(len(idxs) + 1)
        results.append({"word": w, "messages": len(idxs), "renderings": renderings, "score": score})
    results.sort(key=lambda r: -r["score"])
    return results[:limit]


def glossary_warnings(pairs, glossary):
    """Messages whose English text has a glossary term but whose target text lacks the fixed rendering."""
    out = []
    for g in glossary:
        en_term = g["en"].lower()
        stems = [w[:max(3, len(w) - 1)] for w in words(g["tr"])]
        if not stems:
            continue
        pat = re.compile(r"(?<![a-z])" + re.escape(en_term) + r"(?:s|es)?(?![a-z])")
        missing = []
        total = 0
        for mid, en, tr in pairs:
            if not tr or not pat.search(en.lower()):
                continue
            total += 1
            tl = tr.lower()
            if not all(s in tl for s in stems):
                missing.append(mid)
        out.append({"en": g["en"], "tr": g["tr"], "messages": total, "missing": missing[:50],
                    "missing_count": len(missing)})
    return out
