"""Review state: flags, comments, highlights, suggestions, glossary. One JSON file."""
import json
import os
import tempfile
import threading
import time
import uuid

STATUSES = ["unreviewed", "flagged", "suggested", "accepted", "fixed"]


def default_state_path(target):
    return os.path.join(os.path.expanduser("~"), ".kadakas-l10n-review", target + ".json")


def utf16_len(s):
    return len(s.encode("utf-16-le")) // 2


def utf16_slice(s, start, end):
    """Slice a string by UTF-16 code unit offsets, the way JavaScript counts."""
    b = s.encode("utf-16-le")
    return b[start * 2:end * 2].decode("utf-16-le", errors="replace")


def msg_id(file, key):
    return file + "::" + key


class StateError(ValueError):
    pass


class ReviewState:
    def __init__(self, path, target):
        self.path = path
        self.target = target
        self.lock = threading.RLock()
        self.data = {"version": 1, "target": target, "messages": {}, "glossary": None}
        self.load()

    def load(self):
        try:
            with open(self.path, encoding="utf-8") as f:
                d = json.load(f)
            if isinstance(d, dict) and isinstance(d.get("messages"), dict):
                self.data.update(d)
        except FileNotFoundError:
            pass
        except (OSError, ValueError) as e:
            # Never drop the owner's data silently: keep the broken file aside.
            broken = self.path + ".broken-" + str(int(time.time()))
            try:
                os.replace(self.path, broken)
            except OSError:
                pass
            raise StateError("State file is not valid JSON, moved to %s (%s)" % (broken, e))

    def save(self):
        with self.lock:
            folder = os.path.dirname(self.path)
            os.makedirs(folder, exist_ok=True)
            fd, tmp = tempfile.mkstemp(prefix=".state-", suffix=".tmp", dir=folder)
            try:
                with os.fdopen(fd, "w", encoding="utf-8") as f:
                    json.dump(self.data, f, ensure_ascii=False, indent=1, sort_keys=True)
                    f.flush()
                    os.fsync(f.fileno())
                os.replace(tmp, self.path)
            except BaseException:
                try:
                    os.unlink(tmp)
                except OSError:
                    pass
                raise

    def entry(self, mid):
        return self.data["messages"].get(mid)

    def _ensure(self, mid):
        return self.data["messages"].setdefault(mid, {
            "status": "unreviewed", "comment": "", "highlights": [], "suggestions": []})

    def view(self, mid):
        e = self.entry(mid) or {}
        return {"status": e.get("status", "unreviewed"), "comment": e.get("comment", ""),
                "highlights": e.get("highlights", []), "suggestions": e.get("suggestions", []),
                "text_hash": e.get("text_hash")}

    # status and comment
    def set_status(self, mid, status):
        if status not in STATUSES:
            raise StateError("Unknown status: %s" % status)
        with self.lock:
            self._ensure(mid)["status"] = status
            self._prune(mid)
            self.save()

    def set_comment(self, mid, comment):
        with self.lock:
            e = self._ensure(mid)
            e["comment"] = str(comment)
            self._prune(mid)
            self.save()

    # highlights: offsets are UTF-16 code units of the target raw text
    def add_highlight(self, mid, text, start, end):
        if not (isinstance(start, int) and isinstance(end, int)) or start < 0 or end <= start:
            raise StateError("Bad highlight range")
        if utf16_len(text) < end:
            raise StateError("Highlight is outside the text")
        word = utf16_slice(text, start, end)
        if not word.strip():
            raise StateError("Highlight is empty")
        with self.lock:
            e = self._ensure(mid)
            hs = [h for h in e["highlights"] if h["end"] <= start or h["start"] >= end]
            hs.append({"start": start, "end": end, "text": word})
            hs.sort(key=lambda h: h["start"])
            e["highlights"] = hs
            e["text_hash"] = hash_text(text)
            if e["status"] == "unreviewed":
                e["status"] = "flagged"
            self.save()
            return word

    def remove_highlight(self, mid, start, end):
        with self.lock:
            e = self._ensure(mid)
            e["highlights"] = [h for h in e["highlights"] if not (h["start"] == start and h["end"] == end)]
            self._prune(mid)
            self.save()

    # suggestions
    def add_suggestion(self, mid, text, source="owner", why="", error=None):
        text = text.strip("\n")
        if not text.strip():
            raise StateError("Suggestion is empty")
        with self.lock:
            e = self._ensure(mid)
            if any(s["text"] == text and s["state"] == "pending" for s in e["suggestions"]):
                return None
            s = {"id": uuid.uuid4().hex[:10], "text": text, "source": source, "state": "pending",
                 "why": why, "error": error, "time": int(time.time())}
            e["suggestions"].append(s)
            if e["status"] in ("unreviewed", "flagged"):
                e["status"] = "suggested"
            self.save()
            return s

    def decide_suggestion(self, mid, sid, decision):
        if decision not in ("accept", "reject", "delete", "pending"):
            raise StateError("Unknown decision")
        with self.lock:
            e = self._ensure(mid)
            sug = next((s for s in e["suggestions"] if s["id"] == sid), None)
            if sug is None:
                raise StateError("Unknown suggestion")
            if decision == "accept" and sug.get("error"):
                raise StateError("This suggestion is invalid: %s" % sug["error"])
            if decision == "delete":
                e["suggestions"].remove(sug)
            elif decision == "accept":
                for s in e["suggestions"]:
                    if s["state"] == "accepted":
                        s["state"] = "pending"
                sug["state"] = "accepted"
                e["status"] = "accepted"
            elif decision == "reject":
                sug["state"] = "rejected"
            else:
                sug["state"] = "pending"
            if decision != "accept":
                if not any(s["state"] == "accepted" for s in e["suggestions"]) and e["status"] == "accepted":
                    e["status"] = "suggested"
                if e["status"] == "suggested" and not any(s["state"] == "pending" for s in e["suggestions"]):
                    e["status"] = "flagged"
            self.save()

    def edit_suggestion(self, mid, sid, text, error):
        text = text.strip("\n")
        if not text.strip():
            raise StateError("Suggestion is empty")
        with self.lock:
            e = self._ensure(mid)
            sug = next((s for s in e["suggestions"] if s["id"] == sid), None)
            if sug is None:
                raise StateError("Unknown suggestion")
            sug["text"], sug["error"], sug["source"] = text, error, "edited"
            if sug["state"] == "accepted" and error:
                sug["state"] = "pending"
                e["status"] = "suggested"
            self.save()

    def accepted(self):
        """List of (message id, suggestion) with an accepted suggestion and status accepted."""
        out = []
        for mid, e in self.data["messages"].items():
            if e.get("status") != "accepted":
                continue
            for s in e.get("suggestions", []):
                if s["state"] == "accepted":
                    out.append((mid, s))
        return out

    def mark_fixed(self, mids):
        with self.lock:
            for mid in mids:
                e = self._ensure(mid)
                e["status"] = "fixed"
                e["highlights"] = []
                for s in e["suggestions"]:
                    if s["state"] == "accepted":
                        s["state"] = "applied"
            self.save()

    def _prune(self, mid):
        """Drop an entry that carries no information, so the file stays small."""
        e = self.data["messages"].get(mid)
        if e and e["status"] == "unreviewed" and not e["comment"] and not e["highlights"] \
                and not e["suggestions"]:
            del self.data["messages"][mid]

    # glossary
    def glossary(self):
        return self.data.get("glossary")

    def set_glossary(self, items):
        clean = []
        for it in items:
            en = str(it.get("en", "")).strip()
            tr = str(it.get("tr", "")).strip()
            if en and tr:
                clean.append({"en": en, "tr": tr, "note": str(it.get("note", "")).strip()})
        with self.lock:
            self.data["glossary"] = clean
            self.save()
        return clean

    def flagged_ids(self):
        return [m for m, e in self.data["messages"].items() if e.get("status") != "unreviewed"]


def hash_text(text):
    import hashlib
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:12]
