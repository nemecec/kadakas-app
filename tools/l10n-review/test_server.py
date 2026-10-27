import json
import os
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

import server
from helpers import CANNED, make_fake_claude, make_repo


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
        cls.srv = server.make_server(app, 0)
        cls.port = cls.srv.server_address[1]
        cls.thread = threading.Thread(target=cls.srv.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()
        cls.srv.server_close()
        cls.thread.join(5)

    def req(self, path, body=None, headers=None):
        r = urllib.request.Request("http://127.0.0.1:%d%s" % (self.port, path),
                                   data=None if body is None else json.dumps(body).encode(),
                                   headers=headers or {})
        with urllib.request.urlopen(r, timeout=30) as resp:
            data = resp.read().decode()
            return json.loads(data) if resp.headers["Content-Type"].startswith("application/json") else data

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
        with self.assertRaises(urllib.error.HTTPError) as c:
            self.req("/api/list", headers={"Host": "evil.example"})
        self.assertEqual(c.exception.code, 403)

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
        with self.assertRaises(urllib.error.HTTPError):
            self.req("/api/queue", {"ids": [mid], "model": "gpt"})


if __name__ == "__main__":
    unittest.main()
