"""Unit and mini repository tests of scripts/check_attribution.py. Run: python3 -I this_file."""
import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "check_attribution.py")
spec = importlib.util.spec_from_file_location("check_attribution", SCRIPT)
ca = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ca)

LICENCES = "MIT\nApache-2.0\nCC0-1.0\nCC-BY-ND-4.0 test-only\nLLVM-exception exception\n"
OWNROW = "Kadakas project"


class Expressions(unittest.TestCase):
    allowed = {"MIT": "", "Apache-2.0": "", "CC-BY-ND-4.0": "test-only", "LLVM-exception": "exception"}

    def ok(self, expr, ships=True):
        self.assertEqual(ca.licence_problems(expr, self.allowed, ships), [], expr)

    def bad(self, expr, ships=True):
        self.assertTrue(ca.licence_problems(expr, self.allowed, ships), expr)

    def test_good(self):
        self.ok("MIT")
        self.ok("MIT OR Apache-2.0")
        self.ok("(Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT)")
        self.ok("MIT AND (Apache-2.0)")

    def test_bad(self):
        for e in ("", "OR", "( )", "()", "MIT OR", "OR MIT", "MIT OR OR Apache-2.0", "MIT AND", "(MIT", "MIT)",
                  "MIT WITH", "MIT Apache-2.0", "Nope-1.0"):
            self.bad(e)

    def test_exceptions(self):
        self.bad("LLVM-exception")
        self.bad("MIT WITH Apache-2.0")
        self.bad("MIT WITH Missing-exception")
        self.ok("MIT WITH LLVM-exception")

    def test_test_only(self):
        self.bad("CC-BY-ND-4.0", ships=True)
        self.ok("CC-BY-ND-4.0", ships=False)


class Globs(unittest.TestCase):
    def m(self, pat, f):
        return ca.glob_re(pat).match(f) is not None

    def test_star_stays_in_folder(self):
        self.assertTrue(self.m("a/*.svg", "a/x.svg"))
        self.assertFalse(self.m("a/*.svg", "a/b/x.svg"))

    def test_double_star_crosses_folders(self):
        self.assertTrue(self.m("a/**", "a/b/c/x.svg"))
        self.assertTrue(self.m("a/**/x.svg", "a/x.svg"))
        self.assertTrue(self.m("a/**/x.svg", "a/b/c/x.svg"))
        self.assertFalse(self.m("a/**", "b/x.svg"))

    def test_question_mark(self):
        self.assertTrue(self.m("s?.dxf", "s1.dxf"))
        self.assertFalse(self.m("s?.dxf", "s/.dxf"))


class Rules(unittest.TestCase):
    def test_ext_and_dirs(self):
        self.assertTrue(ca.rule_ext_dir("x/y.webp"))
        self.assertTrue(ca.rule_ext_dir("x/y.JS"))
        self.assertTrue(ca.rule_ext_dir("fonts/readme.txt"))
        self.assertFalse(ca.rule_ext_dir("fonts/code.rs"))
        self.assertTrue(ca.rule_ext_dir("vendor/lib/code.rs"))
        self.assertTrue(ca.rule_ext_dir("web/lib.md"))
        self.assertFalse(ca.rule_ext_dir("crates/a/src/lib.rs"))
        self.assertFalse(ca.rule_ext_dir("data/.gitattributes"))
        self.assertFalse(ca.rule_ext_dir("a/proptest-regressions/x.txt"))

    def test_embedded(self):
        with tempfile.TemporaryDirectory() as t:
            old = ca.ROOT
            ca.ROOT = os.path.realpath(t)
            try:
                os.makedirs(os.path.join(t, "crates/c/src"))
                os.makedirs(os.path.join(t, "assets"))
                open(os.path.join(t, "crates/c/Cargo.toml"), "w").write("")
                for n in ("crates/c/src/a.bin", "assets/b.svg", "crates/c/src/n.rs", "crates/c/src/doc.md"):
                    open(os.path.join(t, n), "w").write("x")
                open(os.path.join(t, "crates/c/src/lib.rs"), "w").write(
                    'const A: &[u8] = include_bytes!("a.bin");\n'
                    'const B: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/b.svg"));\n'
                    'const C: &str = include_str!("n.rs");\n'
                    'const D: &str = include_str!("doc.md");\n'
                    'const E: &[u8] = include_bytes!(env!("OUT_DIR"));\n'
                    'const F: &[u8] = include_bytes!("missing.bin");\n')
                got = ca.embedded_targets(["crates/c/src/lib.rs"])
            finally:
                ca.ROOT = old
        self.assertEqual(got, {"crates/c/src/a.bin", "assets/b.svg", "crates/c/src/n.rs", "crates/c/src/doc.md"})


class MiniRepo(unittest.TestCase):
    """Runs the script on a small git repository."""

    def make(self, files, tsv, open_list="", licences=LICENCES, notices="", att=None, gha=False):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        r = os.path.join(self.tmp.name, "repo")
        os.makedirs(r)
        subprocess.run(["git", "init", "-q", r], check=True)
        for n, c in files.items():
            p = os.path.join(r, n)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            open(p, "w").write(c)
        d = os.path.join(self.tmp.name, "data")
        os.makedirs(d)
        w = lambda n, c: (open(os.path.join(d, n), "w").write(c), os.path.join(d, n))[1]
        env = dict(os.environ)
        env.update(ATTR_ROOT=r, ATTR_TSV=w("t.tsv", tsv), ATTR_LICENCES=w("l.txt", licences),
                   ATTR_OPEN=w("o.txt", open_list), ATTR_NOTICES=w("n.txt", notices),
                   ATTR_METADATA=w("m.json", '{"packages": []}'), ATTR_TREE=w("tree.txt", ""))
        env["GITHUB_ACTIONS"] = "true" if gha else ""
        env["ATTR_ATTRIBUTION"] = w("a.md", "")
        table = subprocess.run([sys.executable, "-I", SCRIPT, "--table"], env=env, capture_output=True, text=True).stdout
        env["ATTR_ATTRIBUTION"] = w("a.md", att if att is not None else table)
        self.env = env

    def run_gate(self):
        p = subprocess.run([sys.executable, "-I", SCRIPT], env=self.env, capture_output=True, text=True)
        return p.returncode, p.stdout + p.stderr

    @staticmethod
    def row(*c):
        return "\t".join(c) + "\n"

    def test_pass_and_ships_notice(self):
        tsv = self.row("assets/a.svg", "Pretty", "Jane", "MIT", "https://x.org", "ships")
        self.make({"assets/a.svg": "<svg/>"}, tsv, notices="Pretty icons")
        self.assertEqual(self.run_gate()[0], 0)
        self.make({"assets/a.svg": "<svg/>"}, tsv, notices="nothing")
        code, out = self.run_gate()
        self.assertEqual(code, 1)
        self.assertIn("does not name 'Pretty'", out)

    def test_test_rows_need_no_notice(self):
        tsv = self.row("tests/a.svg", "Pretty", "Jane", "MIT", "https://x.org", "test")
        self.make({"tests/a.svg": "<svg/>"}, tsv)
        self.assertEqual(self.run_gate()[0], 0)

    def test_ext_rows(self):
        ok = self.row("ext:lib-1", "Libone", "Jane", "MIT", "https://x.org", "ships")
        self.make({}, ok, notices="Libone")
        self.assertEqual(self.run_gate()[0], 0)
        self.make({}, ok, notices="")
        self.assertEqual(self.run_gate()[0], 1)
        self.make({}, self.row("ext:bad id", "Libone", "Jane", "MIT", "https://x.org", "test"))
        self.assertIn("ext: id", self.run_gate()[1])
        self.make({}, self.row("ext:lib-1", "Libone", "Jane", "MIT", "https://x.org", "maybe"))
        self.assertIn("column use", self.run_gate()[1])

    def test_unknown_source_with_known_licence(self):
        tsv = self.row("tests/a.png", "Pic", "Jane", "MIT", "UNKNOWN", "test")
        self.make({"tests/a.png": "x"}, tsv)
        code, out = self.run_gate()
        self.assertEqual(code, 1)
        self.assertIn("does not name it", out)
        self.make({"tests/a.png": "x"}, tsv, open_list="tests/a.png\n")
        code, out = self.run_gate()
        self.assertEqual(code, 0, out)
        self.assertIn("WARN  tests/a.png", out)
        self.assertIn("1 UNKNOWN row(s)", out)

    def test_unknown_outside_test_dirs_fails(self):
        tsv = self.row("assets/a.png", "Pic", "UNKNOWN", "UNKNOWN", "UNKNOWN", "ships")
        self.make({"assets/a.png": "x"}, tsv, open_list="assets/a.png\n", notices="Pic")
        code, out = self.run_gate()
        self.assertEqual(code, 1)
        self.assertIn("only allowed for a path under tests/", out)

    def test_open_list_rules(self):
        tsv = self.row("tests/*.png", "Pics", "UNKNOWN", "UNKNOWN", "UNKNOWN", "test")
        files = {"tests/a.png": "x", "tests/b.png": "x"}
        self.make(files, tsv, open_list="tests/a.png\n")
        self.assertIn("tests/b.png", self.run_gate()[1])
        self.make(files, tsv, open_list="tests/a.png\ntests/b.png\ntests/a.png\n")
        self.assertIn("listed twice", self.run_gate()[1])
        self.make(files, tsv, open_list="tests/*.png\n")
        self.assertIn("is a glob", self.run_gate()[1])
        self.make(files, tsv, open_list="tests/a.png\ntests/b.png\ntests/gone.png\n")
        self.assertIn("not a file", self.run_gate()[1])
        self.make(files, tsv, open_list="tests/a.png\ntests/b.png\n", gha=True)
        code, out = self.run_gate()
        self.assertEqual(code, 0, out)
        self.assertIn("::warning file=tests/a.png::", out)
        self.assertIn("1 UNKNOWN row(s) covering 2 file(s)", out)

    def test_duplicate_rows(self):
        r = self.row("tests/a.svg", "Pretty", "Jane", "MIT", "https://x.org", "test")
        self.make({"tests/a.svg": "x"}, r + r)
        self.assertIn("twice", self.run_gate()[1])

    def test_colon_names(self):
        self.make({"tests/a.svg": "x"}, self.row("tests/a.svg", "::error x", "Jane", "MIT", "https://x.org", "test"))
        self.assertIn("must not start with '::'", self.run_gate()[1])

    def test_third_party_header_needs_third_party_row(self):
        f = {"tests/a.txt": "Copyright (c) 2001 Somebody\n"}
        own = self.row("tests/a.txt", "Own", OWNROW, "MIT", "https://x.org", "test")
        self.make(f, own)
        self.assertIn("only a row of own work", self.run_gate()[1])
        other = self.row("tests/a.txt", "Theirs", "Somebody", "MIT", "https://x.org", "test")
        self.make(f, other)
        self.assertEqual(self.run_gate()[0], 0)
        # Lowercase, no (c), and Licensed under are found.
        for text in ("copyright 2001 Somebody", "Licensed under the Foo", "SPDX-License-Identifier: MIT"):
            self.make({"tests/a.txt": text + "\n"}, own)
            self.assertEqual(self.run_gate()[0], 1, text)
        # Lines that name the project or its licence are ours.
        for text in ("Copyright (c) Kadakas", "SPDX-License-Identifier: Apache-2.0 OR MIT"):
            self.make({"tests/a.txt": text + "\n"}, own)
            self.assertEqual(self.run_gate()[0], 0, text)

    def test_stale_and_missing(self):
        tsv = self.row("assets/none/**", "Ghost", "Jane", "MIT", "https://x.org", "test")
        self.make({"assets/a.svg": "x"}, tsv)
        code, out = self.run_gate()
        self.assertIn("matches no file", out)
        self.assertIn("assets/a.svg", out)

    def test_symlink_outside(self):
        self.make({"tests/a.txt": "x"}, self.row("tests/a.txt", "A", "Jane", "MIT", "https://x.org", "test"))
        os.symlink("/etc/hosts", os.path.join(self.env["ATTR_ROOT"], "tests", "link.txt"))
        self.assertIn("outside the repository", self.run_gate()[1])

    def test_non_utf8_path(self):
        self.make({}, self.row("tests/a.txt", "A", "Jane", "MIT", "https://x.org", "test"))
        bad = os.path.join(os.fsencode(self.env["ATTR_ROOT"]), b"tests", b"caf\xe9.svg")
        os.makedirs(os.path.dirname(bad), exist_ok=True)
        try:
            with open(bad, "w") as fh:
                fh.write("x")
        except OSError:
            self.skipTest("this file system refuses names that are not UTF-8")
        code, out = self.run_gate()
        self.assertNotIn("Traceback", out)
        self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
