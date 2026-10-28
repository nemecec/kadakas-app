#!/usr/bin/env python3
"""Local review tool for translations. Binds to 127.0.0.1 only. Standard library only."""
import argparse
import hmac
import json
import os
import secrets
import signal
import subprocess
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import prompt as promptmod  # noqa: E402
from review import Review  # noqa: E402
from state import StateError, default_state_path  # noqa: E402

STATIC = {"/": ("index.html", "text/html; charset=utf-8"),
          "/app.js": ("app.js", "text/javascript; charset=utf-8"),
          "/editing.js": ("editing.js", "text/javascript; charset=utf-8"),
          "/offsets.js": ("offsets.js", "text/javascript; charset=utf-8"),
          "/style.css": ("style.css", "text/css; charset=utf-8")}


class Job:
    """Runs `claude -p` for a list of messages, one after another."""

    def __init__(self, review, ids, model, timeout, more):
        self.review, self.ids, self.model, self.timeout, self.more = review, ids, model, timeout, more
        self.done = 0
        self.current = None
        self.started = time.time()
        self.item_started = self.started
        self.status = "running"
        self.errors = {}
        self.finished_ids = []
        self.cancelled = False
        self.handle = promptmod.Handle()
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        try:
            for mid in self.ids:
                if self.cancelled:
                    break
                self.current = mid
                self.item_started = time.time()
                try:
                    self.review.suggest(mid, self.model, self.timeout, self.more, self.handle)
                except promptmod.AskError as e:
                    if not self.cancelled:
                        self.errors[mid] = {"message": str(e), "code": e.code, "stderr": e.stderr}
                except Exception as e:  # keep the queue going
                    self.errors[mid] = {"message": "Unexpected error: %s" % e, "code": None, "stderr": ""}
                self.done += 1
                self.finished_ids.append(mid)
        finally:
            self.current = None
            self.status = "cancelled" if self.cancelled else "done"

    def cancel(self):
        self.cancelled = True
        self.handle.cancel()

    def info(self):
        now = time.time()
        return {"status": self.status, "done": self.done, "total": len(self.ids), "current": self.current,
                "elapsed": int(now - self.started),
                "item_elapsed": int(now - self.item_started) if self.current else 0,
                "errors": self.errors, "finished": self.finished_ids, "model": self.model,
                "started": int(self.started * 1000)}


class App:
    def __init__(self, repo, target, state_path=None, claude_bin="claude", timeout=120):
        self.repo = os.path.abspath(repo)
        self.default_target = target
        self.state_path = state_path
        self.claude_bin = claude_bin
        self.timeout = timeout
        self.reviews = {}
        self.lock = threading.Lock()
        self.job_lock = threading.Lock()
        self.job = None
        self.token = secrets.token_hex(16)
        self.review(target)

    def review(self, target):
        with self.lock:
            if target not in self.reviews:
                path = self.state_path if target == self.default_target and self.state_path else None
                r = Review(self.repo, target, state_path=path, claude_bin=self.claude_bin)
                r.build_usage()
                self.reviews[target] = r
            return self.reviews[target]

    def start_job(self, rv, ids, model, more):
        with self.job_lock:
            if self.job and self.job.status == "running":
                return None
            self.job = Job(rv, ids, model, self.timeout, more)
            return self.job

    def stop_job(self):
        with self.job_lock:
            job = self.job
        if job:
            job.cancel()
            job.thread.join(6)
        promptmod.kill_all()


MAX_BODY = 5_000_000
POST_OPS = ("accept", "reject", "delete", "pending", "add", "edit")


def need(body, key, kind):
    """A field of the right type, or a StateError (HTTP 400) that names the field."""
    v = body.get(key)
    ok = isinstance(v, kind) and not (kind is int and isinstance(v, bool))
    if not ok:
        raise StateError("Field '%s' must be %s" % (key, {str: "a string", int: "a whole number", list: "a list"}[kind]))
    return v


class Handler(BaseHTTPRequestHandler):
    server_version = "l10n-review"
    app = None

    def log_message(self, fmt, *args):
        if os.environ.get("L10N_REVIEW_LOG"):
            super().log_message(fmt, *args)

    def send(self, code, body, ctype="application/json; charset=utf-8"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body, ensure_ascii=False)
        data = body.encode("utf-8") if isinstance(body, str) else body
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        self.wfile.write(data)

    def origins(self):
        port = self.server.server_address[1]
        return ("http://127.0.0.1:%d" % port, "http://localhost:%d" % port)

    def host_ok(self):
        return (self.headers.get("Host") or "") in [o[len("http://"):] for o in self.origins()]

    def origin_ok(self):
        origin = self.headers.get("Origin")
        return origin is None or origin in self.origins()

    def token_ok(self):
        sent = self.headers.get("X-L10n-Token") or ""
        return hmac.compare_digest(sent.encode("utf-8"), self.app.token.encode("utf-8"))

    def do_GET(self):
        self.handle_request("GET")

    def do_POST(self):
        self.handle_request("POST")

    def do_OPTIONS(self):
        # No CORS: a preflight is never answered with permission.
        self.send(405, {"error": "Method not allowed"})

    def handle_request(self, method):
        if not self.host_ok():
            return self.send(403, {"error": "Forbidden host"})
        if not self.origin_ok():
            return self.send(403, {"error": "Forbidden origin"})
        u = urllib.parse.urlparse(self.path)
        q = {k: v[0] for k, v in urllib.parse.parse_qs(u.query).items()}
        try:
            if u.path in STATIC and method == "GET":
                name, ctype = STATIC[u.path]
                with open(os.path.join(HERE, "static", name), "rb") as f:
                    data = f.read()
                if name == "index.html":
                    data = data.replace(b"__TOKEN__", self.app.token.encode("ascii"))
                return self.send(200, data, ctype)
            if not u.path.startswith("/api/"):
                return self.send(404, {"error": "Not found"})
            if not self.token_ok():
                return self.send(403, {"error": "Missing or wrong token"})
            body = {}
            if method == "POST":
                ctype = (self.headers.get("Content-Type") or "").split(";")[0].strip().lower()
                if ctype != "application/json":
                    return self.send(403, {"error": "Content-Type must be application/json"})
                try:
                    n = max(0, int(self.headers.get("Content-Length") or 0))
                except ValueError:
                    return self.send(400, {"error": "Bad Content-Length"})
                if n > MAX_BODY:
                    return self.send(413, {"error": "Too large"})
                raw = self.rfile.read(n) if n else b""
                try:
                    body = json.loads(raw.decode("utf-8")) if raw else {}
                except ValueError:
                    return self.send(400, {"error": "Bad JSON"})
                if not isinstance(body, dict):
                    return self.send(400, {"error": "The JSON body must be an object"})
            target = q.get("target") or body.get("target") or self.app.default_target
            if not isinstance(target, str) or (target != self.app.default_target and
                                               target not in Review.locales_available(self.app.repo)):
                return self.send(400, {"error": "Unknown locale"})
            rv = self.app.review(target)
            return self.route(method, u.path[5:], q, body, rv)
        except StateError as e:
            return self.send(400, {"error": str(e)})
        except Exception as e:
            return self.send(500, {"error": "%s: %s" % (type(e).__name__, e)})

    def route(self, method, path, q, body, rv):
        app = self.app
        mid = body.get("id") if method == "POST" else q.get("id")
        if mid is not None:
            if not isinstance(mid, str):
                raise StateError("Field 'id' must be a string")
            if mid not in rv.index:
                return self.send(404, {"error": "Unknown message: %s" % mid})
        if method == "GET":
            if path == "list":
                rv.reload(force=False)
                return self.send(200, rv.list())
            if path in ("item", "usage", "prompt") and mid is None:
                raise StateError("Field 'id' is required")
            if path == "item":
                return self.send(200, rv.item(mid))
            if path == "usage":
                return self.send(200, rv.usage.lookup(rv.index[mid]["key"]))
            if path == "terms":
                return self.send(200, rv.term_report())
            if path == "export":
                return self.send(200, rv.export_markdown(), "text/markdown; charset=utf-8")
            if path == "prompt":
                return self.send(200, {"prompt": rv.make_prompt(mid)})
            if path == "queue":
                return self.send(200, app.job.info() if app.job else {"status": "idle"})
            return self.send(404, {"error": "Not found"})
        needs_id = ("status", "comment", "highlight", "suggestion", "validate", "owner-edit")
        if path in needs_id and mid is None:
            raise StateError("Field 'id' is required")
        if path == "status":
            rv.state.set_status(mid, need(body, "status", str))
        elif path == "comment":
            rv.state.set_comment(mid, need(body, "comment", str))
        elif path == "highlight":
            tg = rv.index[mid]["tgt"]
            if tg is None:
                raise StateError("The message has no text in this language")
            start, end = need(body, "start", int), need(body, "end", int)
            if body.get("remove"):
                rv.state.remove_highlight(mid, start, end)
            else:
                rv.state.add_highlight(mid, tg.raw, start, end)
        elif path == "suggestion":
            op = need(body, "op", str)
            if op not in POST_OPS:
                raise StateError("Unknown op: %s" % op)
            if op == "add":
                rv.add_suggestion(mid, need(body, "text", str), "owner")
            elif op == "edit":
                rv.edit_suggestion(mid, need(body, "sid", str), need(body, "text", str))
            else:
                rv.state.decide_suggestion(mid, need(body, "sid", str), op)
        elif path in ("bulk-ok", "bulk-undo"):
            ids = [i for i in need(body, "ids", list) if isinstance(i, str) and i in rv.index]
            if path == "bulk-ok":
                changed = rv.state.bulk_set(ids, "unreviewed", "ok")
            else:
                changed = rv.state.bulk_set(ids, "ok", "unreviewed")
            return self.send(200, {"changed": changed, "count": len(changed)})
        elif path == "validate":
            text = need(body, "text", str)
            return self.send(200, {"error": rv.validate(mid, text.strip("\n")),
                                   "unchanged": rv.index[mid]["tgt"] is not None and text.strip("\n") == rv.index[mid]["tgt"].raw})
        elif path == "owner-edit":
            base = body.get("base_hash")
            if base is not None and not isinstance(base, str):
                raise StateError("Field 'base_hash' must be a string")
            item, unchanged = rv.owner_edit(mid, need(body, "text", str), base, bool(body.get("accept")))
            item["unchanged"] = unchanged
            return self.send(200, item)
        elif path == "glossary":
            items = need(body, "items", list)
            if not all(isinstance(i, dict) for i in items):
                raise StateError("Each glossary item must be an object")
            return self.send(200, {"glossary": rv.state.set_glossary(items)})
        elif path == "queue":
            ids = [i for i in need(body, "ids", list) if isinstance(i, str) and i in rv.index]
            if not ids:
                raise StateError("No messages")
            model = body.get("model", "sonnet")
            if model not in promptmod.MODELS:
                raise StateError("Unknown model")
            job = app.start_job(rv, ids, model, bool(body.get("more")))
            if job is None:
                return self.send(409, {"error": "A request is already running."})
            return self.send(200, job.info())
        elif path == "queue/cancel":
            if app.job:
                app.job.cancel()
            return self.send(200, app.job.info() if app.job else {"status": "idle"})
        elif path == "apply/preview":
            return self.send(200, rv.apply_preview())
        elif path == "apply":
            expect = need(body, "expect", list)
            return self.send(200, rv.apply(expect, bool(body.get("confirm_dirty"))))
        else:
            return self.send(404, {"error": "Not found"})
        return self.send(200, rv.item(mid))


def make_server(app, port=8765):
    Handler.app = app
    return ThreadingHTTPServer(("127.0.0.1", port), Handler)


def find_repo(start):
    try:
        r = subprocess.run(["git", "-C", start, "rev-parse", "--show-toplevel"], capture_output=True,
                           text=True, timeout=10)
        if r.returncode == 0:
            return r.stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        pass
    return os.path.abspath(os.path.join(HERE, "..", ".."))


def main(argv=None):
    ap = argparse.ArgumentParser(description="Review translations side by side (local only).")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--target", default="et")
    ap.add_argument("--repo", default=None)
    ap.add_argument("--state", default=None, help="state file (default ~/.kadakas-l10n-review/<target>.json)")
    ap.add_argument("--claude-bin", default="claude")
    ap.add_argument("--timeout", type=int, default=120, help="seconds for one claude call")
    a = ap.parse_args(argv)
    repo = a.repo or find_repo(HERE)
    try:
        app = App(repo, a.target, a.state, a.claude_bin, a.timeout)
    except StateError as e:
        sys.exit("Error: %s" % e)

    def on_term(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, on_term)
    srv = make_server(app, a.port)
    rv = app.review(a.target)
    print("Reviewing %s in %s" % (a.target, repo))
    print("State file: %s" % rv.state.path)
    print("Open http://127.0.0.1:%d/  (Ctrl+C to stop)" % srv.server_address[1], flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        app.stop_job()
        srv.server_close()


if __name__ == "__main__":
    main()
