import glob
import os
import re
import unittest

import ftl

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
LOCALES = os.path.join(REPO, "crates", "kadakas-l10n", "locales")

SAMPLE = """## demo: a demo file (demo.rs)

# Shown in the title bar.
demo-title = Hello { $name }, you have { $count } items
demo-multi =
    First line
    second line { $x }
demo-count =
    { $n ->
        [one] { $n } item
       *[other] { $n } items
    }
demo-col0 = { $count ->
    [one] One
   *[other] { $count } things
}
# not attached

demo-attr = Greeting
    .title = The title { $who }
-term = Kadakas
demo-literal = a{ " " }b
"""


class ParserTest(unittest.TestCase):
    def setUp(self):
        self.f = ftl.parse_text(SAMPLE, "demo.ftl")
        self.m = {m.key: m for m in self.f.messages}

    def test_group_and_comment(self):
        self.assertEqual(self.f.group, "demo: a demo file (demo.rs)")
        self.assertEqual(self.m["demo-title"].comment, "Shown in the title bar.")
        self.assertEqual(self.m["demo-multi"].comment, "")
        self.assertEqual(self.m["demo-attr"].comment, "")

    def test_simple_message(self):
        m = self.m["demo-title"]
        self.assertEqual(m.raw, "Hello { $name }, you have { $count } items")
        self.assertEqual(m.variables, ["name", "count"])
        self.assertEqual((m.start, m.end), (4, 4))
        self.assertTrue(m.inline_first)

    def test_multiline(self):
        m = self.m["demo-multi"]
        self.assertEqual(m.raw, "First line\nsecond line { $x }")
        self.assertFalse(m.inline_first)
        self.assertEqual((m.start, m.end), (5, 7))

    def test_selector(self):
        m = self.m["demo-count"]
        self.assertEqual(m.selector, "$n")
        self.assertEqual([(v["key"], v["default"]) for v in m.variants], [("one", False), ("other", True)])
        self.assertEqual(m.variants[1]["text"], "{ $n } items")
        self.assertEqual(m.variables, ["n"])

    def test_selector_closing_brace_at_column_zero(self):
        m = self.m["demo-col0"]
        self.assertEqual(len(m.variants), 2)
        self.assertEqual(m.end - m.start, 3)

    def test_attribute_and_term(self):
        m = self.m["demo-attr"]
        self.assertEqual(m.raw, "Greeting")
        self.assertEqual(m.attributes, [{"name": "title", "raw": "The title { $who }"}])
        self.assertIn("who", m.variables)
        self.assertEqual([t.key for t in self.f.terms], ["-term"])
        self.assertNotIn("-term", self.m)

    def test_literal_is_not_variable(self):
        self.assertEqual(self.m["demo-literal"].variables, [])

    def test_extract_variables(self):
        self.assertEqual(ftl.extract_variables("{ $a } { $b-c } { $a }"), ["a", "b-c"])

    def test_parity(self):
        a = ftl.parse_text("x = 1\ny = 2\n", "f.ftl")
        b = ftl.parse_text("x = 1\nz = 3\n", "f.ftl")
        p = ftl.key_parity({"f.ftl": a}, {"f.ftl": b, "g.ftl": b})
        self.assertEqual(p["missing"], [("f.ftl", "y")])
        self.assertEqual(p["extra"], [("f.ftl", "z"), ("g.ftl", "x"), ("g.ftl", "z")])
        self.assertEqual(p["extra_files"], ["g.ftl"])


class RealFilesTest(unittest.TestCase):
    def test_every_real_file_parses_with_the_regex_count(self):
        paths = glob.glob(os.path.join(LOCALES, "*", "*.ftl"))
        self.assertTrue(paths)
        for p in paths:
            with open(p, encoding="utf-8") as f:
                text = f.read()
            expected = len(re.findall(r"^[a-z][a-z0-9-]* =", text, re.M))
            got = ftl.parse_file(p)
            self.assertEqual(len(got.messages), expected, p)
            for m in got.messages:
                self.assertTrue(m.raw.strip(), "%s %s has no value" % (p, m.key))

    def test_real_variable_sets_match_between_locales(self):
        en = ftl.parse_locale(os.path.join(LOCALES, "en-US"))
        for loc in os.listdir(LOCALES):
            if loc == "en-US":
                continue
            tg = ftl.parse_locale(os.path.join(LOCALES, loc))
            self.assertEqual(ftl.key_parity(en, tg)["missing"], [])
            for fn, f in tg.items():
                emap = {m.key: m for m in en[fn].messages}
                for m in f.messages:
                    self.assertEqual(set(m.variables), set(emap[m.key].variables), (fn, m.key))


if __name__ == "__main__":
    unittest.main()
