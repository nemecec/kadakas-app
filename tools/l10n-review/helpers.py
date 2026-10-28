"""Shared helpers for the tests: a small fake repo and a fake claude executable."""
import os
import stat
import subprocess
import tempfile

EN = """## demo: demo texts

# Title of the crash dialog.
demo-title = Crash report { $n } of { $total }
demo-recover = Recovery copies are still here.
demo-count =
    { $n ->
        [one] { $n } layer
       *[other] { $n } layers
    }
demo-multi =
    First { $x }
    second
"""
ET = """## demo: demo tekstid

# Title of the crash dialog.
demo-title = Jooksutuse aruanne { $n } / { $total }
demo-recover = Alles on need taastekoopiad.
demo-count =
    { $n ->
        [one] { $n } kiht
       *[other] { $n } kihti
    }
demo-multi =
    Esimene { $x }
    teine
"""
RS = """fn f() {
    let a = 1;
    let t = tr!("demo-title");
    let r = tr!("demo-recover");
    let d = format!("layer-panel-{}", x);
}
"""


def make_repo(root=None, git=False):
    root = root or tempfile.mkdtemp(prefix="l10n-test-")
    loc = os.path.join(root, "crates", "kadakas-l10n", "locales")
    for name, text in (("en-US", EN), ("et", ET)):
        os.makedirs(os.path.join(loc, name), exist_ok=True)
        with open(os.path.join(loc, name, "demo.ftl"), "w", encoding="utf-8") as f:
            f.write(text)
    src = os.path.join(root, "crates", "app", "src")
    os.makedirs(src, exist_ok=True)
    with open(os.path.join(src, "a.rs"), "w") as f:
        f.write(RS)
    if git:
        run = lambda *a: subprocess.run(["git", "-C", root] + list(a), check=True, capture_output=True)
        run("init", "-q")
        run("add", "-A")
        run("-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", "init")
    return root


def et_path(root):
    return os.path.join(root, "crates", "kadakas-l10n", "locales", "et", "demo.ftl")


def make_fake_claude(folder):
    """A fake `claude`. FAKE_MODE: json, fence, bad, slow, fail. It records stdin, args and cwd."""
    path = os.path.join(folder, "claude")
    with open(path, "w") as f:
        f.write("""#!/bin/sh
cat > "$FAKE_DIR/stdin.txt"
printf '%s\n' "$@" > "$FAKE_DIR/args.txt"
pwd > "$FAKE_DIR/cwd.txt"
case "$FAKE_MODE" in
  json) cat "$FAKE_DIR/canned.json" ;;
  fence) printf 'Here you go:\\n```json\\n'; cat "$FAKE_DIR/canned.json"; printf '\\n```\\n' ;;
  bad) echo "I cannot do JSON today" ;;
  slow) exec sleep 20 ;;
  grand) sleep 300 &
         echo $! > "$FAKE_DIR/grandchild.pid"
         wait ;;
  fail) echo "Not logged in. Run claude login." >&2; exit 1 ;;
esac
""")
    os.chmod(path, os.stat(path).st_mode | stat.S_IEXEC)
    return path


CANNED = """{"alternatives":[
{"text":"Kokkujooksmise aruanne { $n } / { $total }","why":"Standard term","keeps_variables":true},
{"text":"Kokkujooksmise aruanne { $n }","why":"Drops total","keeps_variables":true},
{"text":"  Krahhi aruanne { $n } / { $total }","why":"Leading spaces","keeps_variables":true}
]}"""
