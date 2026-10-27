import http.client
import json
import os
import re
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

import server
from helpers import CANNED, et_path, make_fake_claude, make_repo


class ServerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.mkdtemp()
        cls.root = make_repo(git=True)
        cls.fake = make_fake_claude(cls.dir)
        with open(os.path.join(cls.dir, "canned.json"), "w") as f:
            f.write(CANNED)
        os.environ["FAKE_DIR"] = cls.dir
        os.environ["FAKE_MODE"] = "json"
        app = server.App(cls.root, "et", os.path.join(cls.dir, "state.json"), cls.fake, timeout=20)
        cls.app = app
        cls.srv = server.make_server(app, 0)
        cls.port = cls.srv.server_address[1]
        cls.thread = threading.Thread(target=cls.srv.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()
        cls.srv.server_close()
        cls.thread.join(5)

    def raw(self, method, path, body=None, headers=None, token=True, ctype="application/json"):
        """Low-level request. Returns (status, text)."""
        h = {}
        if token:
            h["X-L10n-Token"] = self.app.token
        if body is not None:
            h["Content-Type"] = ctype
        h.update(headers or {})
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=30)
        data = None if body is None else (body if isinstance(body, str) else json.dumps(body))
        conn.request(method, path, body=data, headers=h)
        r = conn.getresponse()
        text = r.read().decode()
        conn.close()
        return r.status, text

    def req(self, path, body=None, headers=None):
        status, text = self.raw("GET" if body is None else "POST", path, body, headers)
        if status >= 400:
            raise urllib.error.HTTPError(path, status, text, {}, None)
        try:
            return json.loads(text)
        except ValueError:
            return text

    def test_smoke(self):
        self.assertEqual(self.srv.server_address[0], "127.0.0.1")
        page = self.req("/")
        self.assertIn("Translation review", page)
        self.assertIn("Offsets", self.req("/offsets.js"))
        d = self.req("/api/list")
        self.assertEqual(d["total"], 4)
        self.assertEqual(d["counts"]["flagged"], 0)
        self.assertEqual(len(d["files"]), 1)

    def test_bad_host_rejected(self):
        self.assertEqual(self.raw("GET", "/api/list", headers={"Host": "evil.example"})[0], 403)

    def test_highlight_comment_and_queue(self):
        mid = "demo.ftl::demo-title"
        it = self.req("/api/highlight", {"id": mid, "start": 0, "end": 10})
        self.assertEqual(it["highlights"][0]["text"], "Jooksutuse")
        self.assertEqual(it["status"], "flagged")
        self.req("/api/comment", {"id": mid, "comment": "bad"})
        self.assertEqual(self.req("/api/list")["counts"]["flagged"], 1)
        self.req("/api/queue", {"ids": [mid], "model": "haiku"})
        for _ in range(100):
            j = self.req("/api/queue")
            if j["status"] != "running":
                break
            time.sleep(0.1)
        self.assertEqual(j["status"], "done")
        it = self.req("/api/item?id=" + mid)
        self.assertEqual(len(it["suggestions"]), 3)
        self.assertEqual(self.req("/api/usage?id=" + mid)["uses"][0]["line"], 3)
        self.assertIn("Weird words", self.req("/api/export"))
        self.assertEqual(self.raw("POST", "/api/queue", {"ids": [mid], "model": "gpt"})[0], 400)

    def test_csrf_rules(self):
        port = self.port
        good = {"id": "demo.ftl::demo-title", "comment": "x"}
        self.assertEqual(self.raw("POST", "/api/comment", good)[0], 200)
        # foreign port origin, foreign host origin, null origin
        for origin in ("http://127.0.0.1:%d" % (port + 1), "http://localhost:1", "http://evil.example", "null"):
            self.assertEqual(self.raw("POST", "/api/comment", good, {"Origin": origin})[0], 403, origin)
            self.assertEqual(self.raw("GET", "/api/list", headers={"Origin": origin})[0], 403, origin)
        self.assertEqual(self.raw("POST", "/api/comment", good, {"Origin": "http://localhost:%d" % port})[0], 200)
        # text/plain and form posts
        self.assertEqual(self.raw("POST", "/api/comment", json.dumps(good), ctype="text/plain")[0], 403)
        self.assertEqual(self.raw("POST", "/api/comment", "id=1", ctype="application/x-www-form-urlencoded")[0], 403)
        # no token and a wrong token, on GET and POST
        self.assertEqual(self.raw("GET", "/api/list", token=False)[0], 403)
        self.assertEqual(self.raw("POST", "/api/comment", good, token=False)[0], 403)
        self.assertEqual(self.raw("GET", "/api/list", headers={"X-L10n-Token": "nope"})[0], 403)
        # preflight is never answered with permission
        status, _ = self.raw("OPTIONS", "/api/comment", headers={"Origin": "http://evil.example",
                                                                 "Access-Control-Request-Method": "POST"})
        self.assertEqual(status, 405)

    def test_page_embeds_the_token(self):
        page = self.req("/")
        self.assertIn('name="l10n-token" content="%s"' % self.app.token, page)
        self.assertNotIn("__TOKEN__", page)

    def test_field_validation(self):
        mid = "demo.ftl::demo-title"
        cases = [("/api/comment", {"id": 5, "comment": "x"}), ("/api/comment", {"id": mid}),
                 ("/api/comment", {"id": mid, "comment": 3}), ("/api/highlight", {"id": mid, "remove": True}),
                 ("/api/highlight", {"id": mid, "start": "a", "end": 2}),
                 ("/api/highlight", {"id": mid, "start": True, "end": 2}),
                 ("/api/suggestion", {"id": mid, "op": "bogus"}), ("/api/suggestion", {"id": mid, "op": "accept"}),
                 ("/api/glossary", {"items": "x"}), ("/api/glossary", {"items": [1]}),
                 ("/api/queue", {"ids": "x"}), ("/api/apply", {}), ("/api/status", {"id": mid, "status": "weird"})]
        for path, body in cases:
            status, text = self.raw("POST", path, body)
            self.assertEqual(status, 400, (path, body, text))
            self.assertIn("error", json.loads(text))
        self.assertEqual(self.raw("POST", "/api/comment", "[1]")[0], 400)
        self.assertEqual(self.raw("POST", "/api/comment", "{oops")[0], 400)
        self.assertEqual(self.raw("GET", "/api/item")[0], 400)
        self.assertEqual(self.raw("GET", "/api/item?id=nope")[0], 404)
        self.assertEqual(self.raw("GET", "/api/list?target=zz")[0], 400)
        self.assertEqual(self.raw("POST", "/api/comment", {"id": "demo.ftl::demo-title", "comment": "x"},
                                  {"Content-Length": "-5"})[0], 400)

    def test_apply_over_http_needs_the_preview_digest(self):
        mid = "demo.ftl::demo-recover"
        it = self.req("/api/suggestion", {"id": mid, "op": "add", "text": "Alles on need varukoopiad."})
        sid = it["suggestions"][-1]["id"]
        self.req("/api/suggestion", {"id": mid, "sid": sid, "op": "accept"})
        pv = self.req("/api/apply/preview", {})
        self.assertEqual(len(pv["digest"]), 1)
        r = self.req("/api/apply", {"expect": [], "confirm_dirty": True})
        self.assertEqual(r["refused"], "changed")
        self.assertIn("taastekoopiad", open(et_path(self.root), encoding="utf-8").read())
        r = self.req("/api/apply", {"expect": pv["digest"], "confirm_dirty": True})
        self.assertEqual(r["applied"], [mid])
        self.assertIn("varukoopiad", open(et_path(self.root), encoding="utf-8").read())


class QueueKillTest(unittest.TestCase):
    def test_cancel_leaves_running_and_kills_the_grandchild(self):
        d = tempfile.mkdtemp()
        root = make_repo(git=True)
        fake = make_fake_claude(d)
        os.environ["FAKE_DIR"] = d
        os.environ["FAKE_MODE"] = "grand"
        app = server.App(root, "et", os.path.join(d, "s.json"), fake, timeout=60)
        rv = app.review("et")
        job = app.start_job(rv, ["demo.ftl::demo-title", "demo.ftl::demo-recover"], "haiku", False)
        self.assertIsNotNone(job)
        self.assertIsNone(app.start_job(rv, ["demo.ftl::demo-title"], "haiku", False))   # one at a time
        pidfile = os.path.join(d, "grandchild.pid")
        for _ in range(50):
            if os.path.exists(pidfile) and open(pidfile).read().strip():
                break
            time.sleep(0.1)
        pid = int(open(pidfile).read())
        t0 = time.time()
        job.cancel()
        while job.status == "running" and time.time() - t0 < 5:
            time.sleep(0.05)
        self.assertEqual(job.status, "cancelled")
        self.assertLess(time.time() - t0, 5)
        end = time.time() + 5
        alive = True
        while alive and time.time() < end:
            try:
                os.kill(pid, 0)
                time.sleep(0.1)
            except OSError:
                alive = False
        self.assertFalse(alive, "the grandchild is still running")
        # the second message was never started
        self.assertEqual(job.done, 1)
        os.environ.pop("FAKE_MODE")


if __name__ == "__main__":
    unittest.main()
