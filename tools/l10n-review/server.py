#!/usr/bin/env python3
"""Local review tool for translations. Binds to 127.0.0.1 only. Standard library only."""
import argparse
import json
import os
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
          "/offsets.js": ("offsets.js", "text/javascript; charset=utf-8"),
          "/style.css": ("style.css", "text/css; charset=utf-8")}


class Job:
    """Runs `claude -p` for a list of messages, one after another."""

    def __init__(self, review, ids, model, timeout, more):
        self.review, self.ids, self.model, self.timeout, self.more = review, ids, model, timeout, more
        self.done = 0
        self.current = None
        self.started = time.time()
        self.status = "running"
        self.errors = {}
        self.finished_ids = []
        self.cancelled = False
        self.procs = []
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        for mid in self.ids:
            if self.cancelled:
                break
            self.current = mid
            self.item_started = time.time()
            try:
                self.review.suggest(mid, self.model, self.timeout, self.more, self.procs)
            except promptmod.AskError as e:
                if not self.cancelled:
                    self.errors[mid] = {"message": str(e), "code": e.code, "stderr": e.stderr}
            except Exception as e:  # keep the queue going
                self.errors[mid] = {"message": "Unexpected error: %s" % e, "code": None, "stderr": ""}
            self.done += 1
            self.finished_ids.append(mid)
        self.current = None
        self.status = "cancelled" if self.cancelled else "done"

    def cancel(self):
        self.cancelled = True
        for p in list(self.procs):
            try:
                p.kill()
            except OSError:
                pass

    def info(self):
        now = time.time()
        return {"status": self.status, "done": self.done, "total": len(self.ids), "current": self.current,
                "elapsed": int(now - self.started),
                "item_elapsed": int(now - getattr(self, "item_started", now)) if self.current else 0,
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
        self.job = None
        self.review(target)

    def review(self, target):
        with self.lock:
            if target not in self.reviews:
                path = self.state_path if target == self.default_target and self.state_path else None
                r = Review(self.repo, target, state_path=path, claude_bin=self.claude_bin)
                r.build_usage()
                self.reviews[target] = r
            return self.reviews[target]


class Handler(BaseHTTPRequestHandler):
    server_version = "l10n-review"
    app = None

    def log_message(self, fmt, *args):
        if os.environ.get("L10N_REVIEW_LOG"):
            super().log_message(fmt, *args)

    # helpers
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

    def host_ok(self):
        host = (self.headers.get("Host") or "").rsplit(":", 1)[0].strip("[]")
        if host not in ("127.0.0.1", "localhost"):
            return False
        origin = self.headers.get("Origin")
        if origin:
            o = urllib.parse.urlparse(origin).hostname
            if o not in ("127.0.0.1", "localhost"):
                return False
        return True

    def do_GET(self):
        self.handle_request("GET")

    def do_POST(self):
        self.handle_request("POST")

    def handle_request(self, method):
        if not self.host_ok():
            return self.send(403, {"error": "Forbidden host"})
        u = urllib.parse.urlparse(self.path)
        q = {k: v[0] for k, v in urllib.parse.parse_qs(u.query).items()}
        body = {}
        if method == "POST":
            n = int(self.headers.get("Content-Length") or 0)
            if n > 4_000_000:
                return self.send(413, {"error": "Too large"})
            raw = self.rfile.read(n) if n else b""
            try:
                body = json.loads(raw.decode("utf-8")) if raw else {}
            except ValueError:
                return self.send(400, {"error": "Bad JSON"})
        try:
            if u.path in STATIC and method == "GET":
                name, ctype = STATIC[u.path]
                with open(os.path.join(HERE, "static", name), "rb") as f:
                    return self.send(200, f.read(), ctype)
            if not u.path.startswith("/api/"):
                return self.send(404, {"error": "Not found"})
            target = q.get("target") or body.get("target") or self.app.default_target
            if target not in Review.locales_available(self.app.repo) and target != self.app.default_target:
                return self.send(404, {"error": "Unknown locale"})
            rv = self.app.review(target)
            return self.route(method, u.path[5:], q, body, rv)
        except StateError as e:
            return self.send(400, {"error": str(e)})
        except KeyError as e:
            return self.send(404, {"error": "Unknown message: %s" % e})
        except Exception as e:
            return self.send(500, {"error": "%s: %s" % (type(e).__name__, e)})

    def route(self, method, path, q, body, rv):
        app = self.app
        mid = body.get("id") or q.get("id")
        if mid is not None and mid not in rv.index:
            return self.send(404, {"error": "Unknown message"})
        if method == "GET":
            if path == "list":
                rv.reload()
                return self.send(200, rv.list())
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
        # POST
        if path == "status":
            rv.state.set_status(mid, body.get("status"))
        elif path == "comment":
            rv.state.set_comment(mid, body.get("comment", ""))
        elif path == "highlight":
            tg = rv.index[mid]["tgt"]
            if tg is None:
                raise StateError("The message has no text in this language")
            if body.get("remove"):
                rv.state.remove_highlight(mid, body["start"], body["end"])
            else:
                rv.state.add_highlight(mid, tg.raw, body.get("start"), body.get("end"))
        elif path == "suggestion":
            op = body.get("op")
            if op == "add":
                rv.add_suggestion(mid, body.get("text", ""), "owner")
            elif op == "edit":
                rv.edit_suggestion(mid, body.get("sid"), body.get("text", ""))
            else:
                rv.state.decide_suggestion(mid, body.get("sid"), op)
        elif path == "glossary":
            return self.send(200, {"glossary": rv.state.set_glossary(body.get("items", []))})
        elif path == "queue":
            if app.job and app.job.status == "running":
                return self.send(409, {"error": "A request is already running."})
            ids = [i for i in body.get("ids", []) if i in rv.index]
            if not ids:
                return self.send(400, {"error": "No messages"})
            model = body.get("model", "sonnet")
            if model not in promptmod.MODELS:
                return self.send(400, {"error": "Unknown model"})
            app.job = Job(rv, ids, model, app.timeout, bool(body.get("more")))
            return self.send(200, app.job.info())
        elif path == "queue/cancel":
            if app.job:
                app.job.cancel()
            return self.send(200, app.job.info() if app.job else {"status": "idle"})
        elif path == "apply/preview":
            return self.send(200, rv.apply_preview())
        elif path == "apply":
            return self.send(200, rv.apply(bool(body.get("confirm_dirty"))))
        else:
            return self.send(404, {"error": "Not found"})
        return self.send(200, rv.item(mid) if mid else {"ok": True})


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
    app = App(repo, a.target, a.state, a.claude_bin, a.timeout)
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
        srv.server_close()


if __name__ == "__main__":
    main()
