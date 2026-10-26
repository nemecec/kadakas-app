"""Tests of scripts/smoke/smoke_run.py with a fake launcher. Run: python3 -I this_file.

The fake launcher is a small Python program that does what the real one does in the smoke test:
it writes the state file and the settings at the first start, makes a run record at the next
starts, and serves auth and describe on a port when KADAKAS_AGENT_PORT is set. FAKE_MODE makes it
fail in one way. Unix only (the fake is a script with a shebang).
"""
import contextlib
import importlib.util
import io
import os
import stat
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "smoke", "smoke_run.py")
spec = importlib.util.spec_from_file_location("smoke_run", SCRIPT)
smoke_run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke_run)

FAKE = r'''#!{python}
import json, os, socket, sys, threading, time
mode = os.environ.get("FAKE_MODE", "ok")
version = "1.2.3"
root = os.environ["KADAKAS_LAUNCHER_ROOT"]
prefs = os.environ["KADAKAS_PREFS_DIR"]
feedback = os.environ["KADAKAS_FEEDBACK_DIR"]
state = os.environ.get("KADAKAS_SMOKE_STATE")
for d in (root, prefs, feedback):
    os.makedirs(d, exist_ok=True)

def put(path, obj):
    with open(path, "w") as f:
        json.dump(obj, f)

def dialog():
    shown = {{k: "x" for k in ["mode", "language", "units", "number", "theme", "navigation", "crash", "updates"]}}
    shown["mode"] = "kadakas-modeler"
    put(state, {{"version": "9.9.9" if mode == "wrong_version" else version,
                "kind": "update" if mode == "wrong_kind" else "first_run",
                "modeler_installed": True, "shown": shown}})
    time.sleep(0.2)
    put(os.path.join(root, "welcome.json"), {{"done": 1}})
    put(os.path.join(root, "launcher.json"), {{"flavor": "kadakas-modeler"}})
    assert os.environ["KADAKAS_UPDATE_URL"].startswith("file:///")
    if mode != "no_prefs":
        put(os.path.join(prefs, "prefs.json"), {{}})
    put(os.path.join(feedback, "settings.json"), {{}})

def serve():
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    d = os.path.join(os.environ["XDG_RUNTIME_DIR"], "kadakas")
    os.makedirs(d, exist_ok=True)
    put(os.path.join(d, "agent.json"), {{"port": srv.getsockname()[1], "token": "t0k"}})
    conn, _ = srv.accept()
    f = conn.makefile("rw")
    for line in f:
        req = json.loads(line)
        if req["method"] == "auth":
            ok = req["params"]["token"] == "t0k"
            out = {{"result": {{"ok": True}}}} if ok else {{"error": {{"message": "no"}}}}
        else:
            out = {{"result": {{"version": "0.0.1" if mode == "wrong_ping" else version, "methods": [{{"name": "a"}}]}}}}
        out.update({{"jsonrpc": "2.0", "id": req["id"]}})
        f.write(json.dumps(out) + "\n")
        f.flush()

first = not os.path.exists(os.path.join(root, "welcome.json"))
if first and mode != "no_state":
    dialog()
elif not first:
    if mode == "dialog_again" and state:
        dialog()
    if mode != "no_run":
        os.makedirs(os.path.join(root, "runs"), exist_ok=True)
        put(os.path.join(root, "runs", "1.json"), {{}})
    if mode == "exits":
        sys.exit(3)
    if os.environ.get("KADAKAS_AGENT_PORT") is not None and mode != "no_agent":
        threading.Thread(target=serve, daemon=True).start()
time.sleep(120)
'''


@unittest.skipIf(os.name == "nt", "the fake launcher is a script with a shebang")
class SmokeRun(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="kadakas-smoke-test-")
        self.fake = os.path.join(self.tmp, "fake-launcher")
        with open(self.fake, "w") as f:
            f.write(FAKE.format(python=sys.executable))
        os.chmod(self.fake, os.stat(self.fake).st_mode | stat.S_IXUSR)
        self.runs = 0
        self.artifacts = os.path.join(self.tmp, "artifacts")

    def run_it(self, mode="ok", version="1.2.3", shot=""):
        os.environ["FAKE_MODE"] = mode
        out, err = io.StringIO(), io.StringIO()
        self.runs += 1
        self.work = os.path.join(self.tmp, f"work{self.runs}")
        args = ["run", "--launcher", self.fake, "--version", version, "--work", self.work,
                "--artifacts", self.artifacts, "--hold", "0", "--timeout", "15", "--settle", "1"]
        if shot:
            args += ["--screenshot-cmd", shot]
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = smoke_run.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_a_good_install_passes_and_leaves_no_process(self):
        code, out, err = self.run_it()
        self.assertEqual(code, 0, out + err)
        self.assertIn("the app answered the ping with version 1.2.3", out)
        self.assertTrue(os.path.isfile(os.path.join(self.artifacts, "third.log")))

    def test_the_screenshot_is_kept_and_a_failed_one_is_only_a_warning(self):
        code, out, err = self.run_it(shot="cp /etc/hosts {out}")
        self.assertEqual(code, 0, out + err)
        self.assertTrue(os.path.isfile(os.path.join(self.artifacts, "welcome.png")))
        code, out, err = self.run_it(shot="false {out}")
        self.assertEqual(code, 0, out + err)
        self.assertIn("WARNING: screenshot", out)
        code, out, err = self.run_it(shot="/no/such/tool {out}")
        self.assertEqual(code, 0, out + err)
        self.assertIn("WARNING: screenshot", out)

    def failure(self, mode, text, **kw):
        code, out, err = self.run_it(mode, **kw)
        self.assertEqual(code, 1, out + err)
        self.assertIn(text, err)

    def test_each_failure_has_its_own_message(self):
        self.failure("ok", "expected '9.9.9'", version="9.9.9")
        self.failure("wrong_version", "the dialog reports version '9.9.9'")
        self.failure("wrong_kind", "the first start shows a 'update' dialog")
        self.failure("no_prefs", "were not written by 'Use defaults'")
        self.failure("dialog_again", "the second start shows the welcome dialog again")
        self.failure("no_run", "made no run record")
        self.failure("exits", "the launcher ended at the second start (3)")
        self.failure("wrong_ping", "the app answers version '0.0.1'")

    def test_a_launcher_that_never_opens_the_dialog_times_out_with_its_log(self):
        os.environ["FAKE_MODE"] = "no_state"
        self.work = os.path.join(self.tmp, "work-no-state")
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = smoke_run.main(["run", "--launcher", self.fake, "--version", "1.2.3", "--work", self.work,
                                   "--timeout", "2", "--settle", "1"])
        self.assertEqual(code, 1)
        self.assertIn("timeout (2.0 s) while we waited for the state file", err.getvalue())

    def test_a_work_folder_with_an_old_run_is_refused(self):
        code, out, err = self.run_it()
        self.assertEqual(code, 0, out + err)
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = smoke_run.main(["run", "--launcher", self.fake, "--version", "1.2.3", "--work", self.work])
        self.assertEqual(code, 1)
        self.assertIn("already has a run", err.getvalue())

    def test_a_missing_launcher_is_an_error(self):
        err = io.StringIO()
        self.work = os.path.join(self.tmp, "work-missing")
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = smoke_run.main(["run", "--launcher", "/no/such/launcher", "--version", "1", "--work", self.work])
        self.assertEqual(code, 1)
        self.assertIn("does not exist", err.getvalue())

    def test_the_state_checks(self):
        shown = {k: "x" for k in smoke_run.STATE_KEYS}
        shown["mode"] = "kadakas-app"
        good = {"version": "1", "kind": "first_run", "modeler_installed": False, "shown": shown}
        smoke_run.check_state(good, "1")
        for bad in (
            dict(good, version="2"),
            dict(good, kind="manual"),
            dict(good, shown={}),
            dict(good, shown=dict(shown, mode="other")),
            dict(good, shown=dict(shown, theme="")),
            {k: v for k, v in good.items() if k != "modeler_installed"},
        ):
            with self.assertRaises(smoke_run.SmokeError):
                smoke_run.check_state(bad, "1")


if __name__ == "__main__":
    unittest.main()
