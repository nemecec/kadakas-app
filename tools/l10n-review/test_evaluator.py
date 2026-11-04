import os
import unittest

import evaluator as ev
import ftl

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
LOCALES = os.path.join(REPO, "crates", "kadakas-l10n", "locales")

SRC = """## t: test
plain = Just text
one = { $n ->
    [one] { $n } layer
   *[other] { $n } layers
}
numkey = { $n ->
    [0] nothing
    [1] exactly one
   *[other] { $n } things
}
nested = { $n ->
    [one] one { $kind ->
        [line] a line
       *[other] a thing
    }
   *[other] many
}
ref = See { plain } and { -brand }
-brand = Kadakas
-case = { $case ->
    [gen] Kadaka
   *[nom] Kadakas
}
withterm = { -case(case: "gen") } and { -case }
cycle-a = A { cycle-b }
cycle-b = B { cycle-a }
selfref = { selfref }
missing = { $x } and { nothing-here } and { -no-term }
literal = a{ " " }b { "{" }
number = { NUMBER($n, minimumFractionDigits: 1) } of { NUMBER($total) }
numsel = { NUMBER($n) ->
    [one] one
   *[other] other
}
dec = { $width }
varref = { varinner } end
varinner = value { $name }
bad = { $n ->
"""


class EvaluatorTest(unittest.TestCase):
    def setUp(self):
        self.f = ftl.parse_text(SRC, "t.ftl")
        self.msg = {m.key: m for m in self.f.messages}
        self.en = ev.Bundle("en-US", {"t": self.f})
        self.et = ev.Bundle("et", {"t": self.f})

    def lines(self, key, bundle=None, owner=None):
        r = ev.previews(self.msg[key].raw, bundle or self.en, owner)
        return [(l["label"], l["text"]) for l in r["lines"]]

    def test_plain_and_plural(self):
        self.assertEqual(self.lines("plain"), [("", "Just text")])
        self.assertEqual(self.lines("one"), [("n = 1", "1 layer"), ("n = 2", "2 layers")])
        self.assertEqual(self.lines("one", self.et), [("n = 1", "1 layer"), ("n = 2", "2 layers")])

    def test_number_keys_and_default(self):
        got = self.lines("numkey")
        self.assertEqual([t for _, t in got], ["nothing", "exactly one", "2 things"])

    def test_nested_selectors_combine(self):
        got = self.lines("nested")
        labels = [l for l, _ in got]
        self.assertIn("kind = line, n = 1", labels)
        self.assertEqual(dict(got)["kind = line, n = 1"], "one a line")
        self.assertEqual(dict(got)["kind = other, n = 1"], "one a thing")
        self.assertEqual(dict(got)["kind = line, n = 2"], "many")

    def test_references_terms_and_arguments(self):
        self.assertEqual(self.lines("ref")[0][1], "See Just text and Kadakas")
        self.assertEqual(self.lines("withterm")[0][1], "Kadaka and Kadakas")

    def test_cycles_do_not_hang(self):
        r = ev.previews(self.msg["cycle-a"].raw, self.en)["lines"][0]
        self.assertIn("{? cycle", r["text"])
        self.assertTrue(any("cycle" in e for e in r["errors"]))
        r = ev.previews(self.msg["selfref"].raw, self.en)["lines"][0]
        self.assertTrue(r["errors"])

    def test_missing_things_are_shown(self):
        r = ev.previews(self.msg["missing"].raw, self.en)["lines"][0]
        self.assertIn("12.5 and", r["text"])   # $x has a sample, so it is not missing
        self.assertIn("{? nothing-here}", r["text"])
        self.assertIn("{? -no-term}", r["text"])
        self.assertEqual(len(r["errors"]), 2)
        text, errors = ev.render("{ $x }", self.en, {})
        self.assertEqual((text, errors), ("{? $x}", ["missing variable $x"]))

    def test_literals(self):
        self.assertEqual(self.lines("literal")[0][1], "a b {")

    def test_number_function_and_locale_separator(self):
        self.assertEqual(self.lines("number", self.en)[0][1], "2.0 of 5")
        self.assertEqual(self.lines("number", self.et)[0][1], "2,0 of 5")
        self.assertEqual(self.lines("dec", self.en)[0][1], "12.5")
        self.assertEqual(self.lines("dec", self.et)[0][1], "12,5")
        self.assertEqual(ev.format_num(ev.Num(12345), "et"), "12 345")
        self.assertEqual(ev.format_num(ev.Num(1234.5, 1), "en-US"), "1,234.5")

    def test_number_selector(self):
        self.assertEqual([t for _, t in self.lines("numsel")], ["one", "other"])

    def test_plural_rules(self):
        for loc in ("en-US", "et"):
            self.assertEqual(ev.plural_category(ev.Num(1), loc), "one")
            self.assertEqual(ev.plural_category(ev.Num(1, 1), loc), "other")   # 1.0
            self.assertEqual(ev.plural_category(ev.Num(0), loc), "other")
            self.assertEqual(ev.plural_category(ev.Num(2), loc), "other")

    def test_owner_samples_win_and_fix_the_value(self):
        got = self.lines("one", owner={"n": "1"})
        self.assertEqual(got, [("", "1 layer")])
        got = self.lines("varref", owner={"name": "Hekk"})
        self.assertEqual(got, [("", "value Hekk end")])
        self.assertEqual(self.lines("dec", owner={"width": "3,5"}, bundle=self.et)[0][1], "3,5")

    def test_variable_of_a_referenced_message_gets_a_sample(self):
        r = ev.previews(self.msg["varref"].raw, self.en)["lines"][0]
        self.assertEqual(r["errors"], [])
        self.assertEqual(r["text"], "value Näidis end")

    def test_syntax_error_is_shown(self):
        r = ev.previews(self.msg["bad"].raw, self.en)["lines"][0]
        self.assertIn("syntax error", r["text"])

    def test_combination_cap(self):
        keys = "\n".join("    [%d] k%d" % (i, i) for i in range(20))
        raw = "{ $a ->\n%s\n   *[other] x\n}" % keys
        r = ev.previews(raw, self.en)
        self.assertEqual(len(r["lines"]), 12)
        self.assertTrue(r["truncated"])

    def test_string_selector(self):
        raw = '{ $kind ->\n    [line] L\n    [area] A\n   *[other] O\n}'
        got = ev.previews(raw, self.en)["lines"]
        self.assertEqual([l["label"] for l in got], ["kind = line", "kind = area", "kind = other"])
        self.assertEqual([l["text"] for l in got], ["L", "A", "O"])


class RealMessagesTest(unittest.TestCase):
    def test_every_real_message_renders_in_both_locales(self):
        failures = []
        count = 0
        for loc in sorted(os.listdir(LOCALES)):
            files = ftl.parse_locale(os.path.join(LOCALES, loc))
            bundle = ev.Bundle(loc, files)
            for fn, f in files.items():
                for m in f.messages:
                    count += 1
                    try:
                        r = ev.previews(m.raw, bundle)
                    except Exception as e:   # the evaluator must never raise
                        failures.append((loc, fn, m.key, "exception %r" % e))
                        continue
                    self.assertLessEqual(len(r["lines"]), ev.MAX_COMBOS)
                    for line in r["lines"]:
                        if line["errors"]:
                            failures.append((loc, fn, m.key, line["errors"]))
                    for a in m.attributes:
                        pass
        self.assertGreater(count, 6000)
        self.assertEqual(failures, [], "\n".join(map(str, failures[:30])))


if __name__ == "__main__":
    unittest.main()
