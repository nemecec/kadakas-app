#!/usr/bin/env python3
"""Smoke test of an installed Kadakas (PLAT-28): the starts and the checks after the install.

The platform scripts (linux.sh, macos.sh, windows.ps1) install a package, then call this file:

    python3 -I scripts/smoke/smoke_run.py run --launcher PATH --version X.Y.Z [options]

The run does these steps. Each one fails with a clear message.

1. First start: the launcher opens the welcome dialog. The test mode of the dialog
   (KADAKAS_SMOKE_STATE) writes a JSON file, waits, answers "Use defaults" and ends the dialog.
   The run checks the file (version, kind, shown values) and then `welcome.json` and the settings.
2. Second start: no dialog. The launcher starts the default program and keeps it running.
3. Third start with the app (`kadakas-app`, flavor set in launcher.json) and an agent port:
   the app answers a JSON-RPC `auth` and `describe` over TCP with the right version.

The screenshots are best effort. `--screenshot-cmd` is a command with `{out}` in it. A failed
screenshot is a warning, never an error.

The test uses its own folders in `--work` (launcher root, preferences, feedback, runtime), so a
run on a developer machine does not touch the real settings. Python 3.8 and the standard library.
"""
import argparse
import json
import os
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import time

IS_WINDOWS = os.name == "nt"
STATE_KEYS = ["mode", "language", "units", "number", "theme", "navigation", "crash", "updates"]
# Files that "Use defaults" writes, relative to the folder named in the first word.
EXPECTED_FILES = [
    ("launcher", "welcome.json"),
    ("launcher", "launcher.json"),
    ("prefs", "prefs.json"),
    ("feedback", "settings.json"),
]


class SmokeError(Exception):
    """A check failed. The text says which."""


def log(msg):
    print(f"smoke: {msg}", flush=True)


def warn(msg):
    print(f"smoke: WARNING: {msg}", flush=True)


# ---------------------------------------------------------------------------------------------
# Folders and environment


class Work:
    """The folders of one run and the environment that every start gets."""

    def __init__(self, root):
        self.root = os.path.abspath(root)
        self.launcher = os.path.join(self.root, "launcher")
        self.prefs = os.path.join(self.root, "prefs")
        self.feedback = os.path.join(self.root, "feedback")
        self.home = os.path.join(self.root, "home")
        self.runtime = os.path.join(self.root, "runtime")
        self.localappdata = os.path.join(self.root, "localappdata")
        self.state = os.path.join(self.root, "welcome-state.json")
        # A local folder that does not exist (the launcher takes only file:// sites, no spaces).
        self.no_updates = "file:///kadakas-smoke-no-updates"
        for d in (self.root, self.home, self.runtime, self.localappdata):
            os.makedirs(d, exist_ok=True)
        if not IS_WINDOWS:
            os.chmod(self.runtime, 0o700)

    def env(self, base, hold, agent_port=None, state=True):
        env = dict(base)
        for name in ("KADAKAS_SKIP_WELCOME", "KADAKAS_AGENT_PORT", "KADAKAS_SMOKE_STATE"):
            env.pop(name, None)
        env["KADAKAS_LAUNCHER_ROOT"] = self.launcher
        env["KADAKAS_PREFS_DIR"] = self.prefs
        env["KADAKAS_FEEDBACK_DIR"] = self.feedback
        env["KADAKAS_SMOKE_HOLD_SECS"] = str(hold)
        # A dialog that stays open for ever must not hang the job.
        env["KADAKAS_WELCOME_TIMEOUT_SECS"] = "300"
        # No network: the update check looks into a local folder that does not exist, so no
        # newer version can replace the one under test.
        env["KADAKAS_UPDATE_URL"] = self.no_updates
        if state:
            env["KADAKAS_SMOKE_STATE"] = self.state
        if agent_port is not None:
            env["KADAKAS_AGENT_PORT"] = str(agent_port)
        if IS_WINDOWS:
            env["LOCALAPPDATA"] = self.localappdata
        else:
            env["HOME"] = self.home
            env["XDG_RUNTIME_DIR"] = self.runtime
            env.pop("XDG_STATE_HOME", None)
            env.pop("XDG_CONFIG_HOME", None)
        return env

    def agent_info(self):
        """Where the app writes agent.json (see kadakas_sys::dirs::runtime_dir)."""
        if IS_WINDOWS:
            return os.path.join(self.localappdata, "kadakas", "agent.json")
        # Elsewhere XDG_RUNTIME_DIR wins, also on macOS.
        return os.path.join(self.runtime, "kadakas", "agent.json")


# ---------------------------------------------------------------------------------------------
# Processes


class Started:
    """A started program and its children (one process group, or a job tree on Windows)."""

    def __init__(self, argv, env, log_path):
        self.argv = argv
        self.log_path = log_path
        self.out = open(log_path, "ab")
        kwargs = {}
        if IS_WINDOWS:
            kwargs["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
        else:
            kwargs["start_new_session"] = True
        self.proc = subprocess.Popen(
            argv, env=env, stdin=subprocess.DEVNULL, stdout=self.out, stderr=subprocess.STDOUT, **kwargs
        )

    def alive(self):
        return self.proc.poll() is None

    def stop(self):
        """Ends the program and everything it started. Safe to call twice."""
        if self.alive():
            try:
                if IS_WINDOWS:
                    subprocess.run(
                        ["taskkill", "/PID", str(self.proc.pid), "/T", "/F"],
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                        timeout=60,
                    )
                else:
                    os.killpg(self.proc.pid, signal.SIGTERM)
            except (OSError, subprocess.SubprocessError):
                pass
            try:
                self.proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                try:
                    if IS_WINDOWS:
                        self.proc.kill()
                    else:
                        os.killpg(self.proc.pid, signal.SIGKILL)
                except OSError:
                    pass
                try:
                    self.proc.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    pass
        elif not IS_WINDOWS:
            # The launcher is gone but its children may live on.
            try:
                os.killpg(self.proc.pid, signal.SIGKILL)
            except OSError:
                pass
        self.out.close()

    def tail(self, n=40):
        try:
            with open(self.log_path, "rb") as f:
                lines = f.read().decode("utf-8", "replace").splitlines()
        except OSError:
            return ""
        return "\n".join(lines[-n:])


def wait_for(what, cond, timeout, proc=None, poll=0.25):
    """Waits until `cond()` is true. Fails with the end of the log if the program ends first."""
    deadline = time.monotonic() + timeout
    while True:
        if cond():
            return
        if proc is not None and not proc.alive():
            raise SmokeError(
                f"the launcher ended with {proc.proc.returncode} while we waited for {what}\n{proc.tail()}"
            )
        if time.monotonic() >= deadline:
            alive = "" if proc is None else f"\n{proc.tail()}"
            raise SmokeError(f"timeout ({timeout} s) while we waited for {what}{alive}")
        time.sleep(poll)


# ---------------------------------------------------------------------------------------------
# Screenshots


def screenshot(cmd, out, artifacts):
    """Best effort. A failure is a warning."""
    if not cmd:
        return
    try:
        out = os.path.abspath(out)
        argv = [a.replace("{out}", out) for a in shlex.split(cmd, posix=not IS_WINDOWS)]
        r = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        if r.returncode != 0 or not os.path.isfile(out) or os.path.getsize(out) == 0:
            warn(f"screenshot {os.path.basename(out)} failed ({r.returncode}): {r.stdout.decode('utf-8', 'replace')[-300:]}")
            return
        log(f"screenshot {out} ({os.path.getsize(out)} bytes)")
        if artifacts:
            os.makedirs(artifacts, exist_ok=True)
            dest = os.path.join(artifacts, os.path.basename(out))
            if os.path.abspath(dest) != out:
                with open(out, "rb") as a, open(dest, "wb") as b:
                    b.write(a.read())
    except Exception as e:  # noqa: BLE001 - a screenshot never fails the run
        warn(f"screenshot failed: {e}")


# ---------------------------------------------------------------------------------------------
# Checks


def read_json(path):
    try:
        with open(path, "rb") as f:
            return json.loads(f.read().decode("utf-8"))
    except (OSError, ValueError) as e:
        raise SmokeError(f"{path} cannot be read as JSON: {e}")


def check_state(state, version):
    """The state file of the dialog (kadakas_welcome::smoke::state_json)."""
    if state.get("version") != version:
        raise SmokeError(f"the dialog reports version {state.get('version')!r}, expected {version!r}")
    if state.get("kind") != "first_run":
        raise SmokeError(f"the first start shows a {state.get('kind')!r} dialog, expected 'first_run'")
    shown = state.get("shown")
    if not isinstance(shown, dict):
        raise SmokeError("the state file has no 'shown' values")
    for k in STATE_KEYS:
        if not isinstance(shown.get(k), str) or not shown[k]:
            raise SmokeError(f"the state file has no shown value for {k!r}: {shown}")
    if shown["mode"] not in ("kadakas-modeler", "kadakas-app"):
        raise SmokeError(f"unknown mode {shown['mode']!r}")
    if not isinstance(state.get("modeler_installed"), bool):
        raise SmokeError("the state file does not say if the modeler is installed")


def check_files(work):
    """welcome.json and the settings files that 'Use defaults' writes."""
    dirs = {"launcher": work.launcher, "prefs": work.prefs, "feedback": work.feedback}
    missing = [os.path.join(dirs[d], f) for d, f in EXPECTED_FILES if not os.path.isfile(os.path.join(dirs[d], f))]
    if missing:
        raise SmokeError("these files were not written by 'Use defaults': " + ", ".join(missing))
    mark = read_json(os.path.join(work.launcher, "welcome.json"))
    if not isinstance(mark, dict) or mark.get("done") != 1:
        raise SmokeError(f"welcome.json does not say that the dialog is done: {mark}")
    launcher = read_json(os.path.join(work.launcher, "launcher.json"))
    if not isinstance(launcher, dict) or "flavor" not in launcher:
        raise SmokeError(f"launcher.json has no flavor: {launcher}")
    return launcher


def ping(info_path, version, timeout=30):
    """JSON-RPC over TCP as the app serves it: `auth` first, then `describe` (it names the version)."""
    info = read_json(info_path)
    port, token = info.get("port"), info.get("token")
    if not isinstance(port, int) or not isinstance(token, str):
        raise SmokeError(f"{info_path} has no port and token")
    with socket.create_connection(("127.0.0.1", port), timeout=timeout) as s:
        s.settimeout(timeout)
        f = s.makefile("rwb")

        def call(i, method, params=None):
            req = {"jsonrpc": "2.0", "id": i, "method": method}
            if params is not None:
                req["params"] = params
            f.write(json.dumps(req).encode() + b"\n")
            f.flush()
            line = f.readline()
            if not line:
                raise SmokeError(f"the app closed the connection after {method}")
            return json.loads(line.decode("utf-8"))

        r = call(1, "auth", {"token": token})
        if r.get("result", {}).get("ok") is not True:
            raise SmokeError(f"auth was refused: {r}")
        r = call(2, "describe", {})
        if "result" not in r:
            raise SmokeError(f"describe failed: {r}")
        got = r["result"].get("version")
        if got != version:
            raise SmokeError(f"the app answers version {got!r}, expected {version!r}")
        if not r["result"].get("methods"):
            raise SmokeError("describe lists no methods")
    return got


def set_flavor(work, flavor):
    """Sets the key `flavor` of launcher.json and keeps the other keys."""
    path = os.path.join(work.launcher, "launcher.json")
    data = read_json(path)
    data["flavor"] = flavor
    tmp = path + ".smoke"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2)
    os.replace(tmp, path)


# ---------------------------------------------------------------------------------------------
# The run


def run(args):
    work_root = args.work or tempfile.mkdtemp(prefix="kadakas-smoke-")
    if os.path.exists(os.path.join(os.path.abspath(work_root), "launcher")):
        raise SmokeError(f"{work_root} already has a run: use a new folder")
    work = Work(work_root)
    artifacts = args.artifacts
    launcher = os.path.abspath(args.launcher)
    if not os.path.isfile(launcher):
        raise SmokeError(f"the launcher {launcher} does not exist")
    log(f"launcher {launcher}, version {args.version}, work folder {work.root}")
    started = []
    try:
        # 1. First start: the dialog.
        env = work.env(os.environ, args.hold)
        first = Started([launcher] + shlex.split(args.launcher_args), env, os.path.join(work.root, "first.log"))
        started.append(first)
        wait_for("the state file of the welcome dialog", lambda: os.path.isfile(work.state), args.timeout, first)
        state = read_json(work.state)
        log(f"the dialog is open: {json.dumps(state)}")
        screenshot(args.screenshot_cmd, os.path.join(work.root, "welcome.png"), artifacts)
        check_state(state, args.version)
        wait_for(
            "welcome.json (Use defaults)",
            lambda: os.path.isfile(os.path.join(work.launcher, "welcome.json")),
            args.timeout,
            first,
        )
        launcher_json = None

        def settled():
            nonlocal launcher_json
            try:
                launcher_json = check_files(work)
                return True
            except SmokeError:
                return False

        try:
            wait_for("the settings files", settled, 30, first)
        except SmokeError:
            check_files(work)  # the message names the missing files
            raise
        log(f"welcome.json and the settings exist; flavor {launcher_json.get('flavor')!r}")
        # The launcher starts the chosen program after the dialog. It must stay up.
        time.sleep(args.settle)
        if not first.alive():
            raise SmokeError(f"the launcher ended after the dialog ({first.proc.returncode})\n{first.tail()}")
        first.stop()

        # 2. Second start: no dialog, the program starts.
        os.remove(work.state)
        second = Started([launcher] + shlex.split(args.launcher_args), work.env(os.environ, args.hold), os.path.join(work.root, "second.log"))
        started.append(second)
        deadline = time.monotonic() + args.settle
        while time.monotonic() < deadline:
            if os.path.exists(work.state):
                raise SmokeError("the second start shows the welcome dialog again")
            if not second.alive():
                raise SmokeError(f"the launcher ended at the second start ({second.proc.returncode})\n{second.tail()}")
            time.sleep(0.25)
        if not os.path.isdir(os.path.join(work.launcher, "runs")) or not os.listdir(os.path.join(work.launcher, "runs")):
            raise SmokeError(f"the second start made no run record in {work.launcher}/runs\n{second.tail()}")
        log("the second start opened the program without a dialog")
        screenshot(args.screenshot_cmd, os.path.join(work.root, "second-start.png"), artifacts)
        second.stop()

        # 3. The app answers a JSON-RPC ping.
        set_flavor(work, "kadakas-app")
        third = Started(
            [launcher] + shlex.split(args.launcher_args),
            work.env(os.environ, args.hold, agent_port=0),
            os.path.join(work.root, "third.log"),
        )
        started.append(third)
        info = work.agent_info()
        wait_for("agent.json of the app", lambda: os.path.isfile(info), args.timeout, third)
        if os.path.exists(work.state):
            raise SmokeError("the third start shows the welcome dialog")
        time.sleep(1)
        answered = None
        deadline = time.monotonic() + 60
        while True:
            try:
                answered = ping(info, args.version)
                break
            except (OSError, ValueError) as e:
                if time.monotonic() > deadline:
                    raise SmokeError(f"the app does not answer the ping: {e}\n{third.tail()}")
                time.sleep(1)
        log(f"the app answered the ping with version {answered}")
        screenshot(args.screenshot_cmd, os.path.join(work.root, "app.png"), artifacts)
        third.stop()
        log("OK")
        return 0
    finally:
        for s in reversed(started):
            s.stop()
        if artifacts:
            try:
                os.makedirs(artifacts, exist_ok=True)
                for name in ("first.log", "second.log", "third.log", "welcome-state.json"):
                    src = os.path.join(work.root, name)
                    if os.path.isfile(src):
                        with open(src, "rb") as a, open(os.path.join(artifacts, name), "wb") as b:
                            b.write(a.read())
            except OSError as e:
                warn(f"logs were not copied: {e}")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="the three starts and their checks")
    r.add_argument("--launcher", required=True, help="the installed kadakas-launcher program")
    r.add_argument("--launcher-args", default="", help="extra arguments of the launcher (one string)")
    r.add_argument("--version", required=True, help="the version of the app, for example 0.1.0-beta.1")
    r.add_argument("--work", help="folder for the state of the test (default: a new temporary folder)")
    r.add_argument("--artifacts", help="folder for screenshots and logs")
    r.add_argument("--screenshot-cmd", default="", help="command that writes a PNG to {out}")
    r.add_argument("--hold", type=float, default=8, help="seconds that the dialog stays after its state file")
    r.add_argument("--timeout", type=float, default=240, help="seconds to wait for each step")
    r.add_argument("--settle", type=float, default=12, help="seconds that a started program must stay up")
    args = p.parse_args(argv)
    try:
        return run(args)
    except SmokeError as e:
        print(f"smoke: FAILED: {e}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    sys.exit(main())
