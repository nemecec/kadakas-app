import json
import os
import tempfile
import unittest

import apply as applymod
import fluent
import prompt as promptmod
import terms
import usage
from helpers import CANNED, et_path, make_fake_claude, make_repo
from review import Review
from state import ReviewState, StateError, utf16_slice


class UsageTest(unittest.TestCase):
    def test_index(self):
        root = make_repo()
        rv = Review(root, "et", state_path=os.path.join(root, "s.json"))
        rv.build_usage()
        u = rv.usage.lookup("demo-title")
        self.assertEqual(u["uses"][0]["path"], os.path.join("crates", "app", "src", "a.rs"))
        self.assertEqual(u["uses"][0]["line"], 3)
        self.assertIn('tr!("demo-title")', u["uses"][0]["snippet"])
        self.assertEqual(len(u["uses"][0]["snippet"].split("\n")), 3)
        # format!("layer-panel-{}") matches as a dynamic use
        idx = usage.UsageIndex(root, {"layer-panel-title", "other-key"})
        dyn = idx.lookup("layer-panel-title")
        self.assertTrue(dyn["uses"][0]["dynamic"])
        self.assertEqual(idx.lookup("other-key")["uses"], [])

    def test_real_repo_index(self):
        here = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
        idx = usage.UsageIndex(here, {"crash-ui-title"})
        self.assertTrue(idx.lookup("crash-ui-title")["uses"])


class StateTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.path = os.path.join(self.dir, "sub", "et.json")

    def test_roundtrip_and_atomic(self):
        s = ReviewState(self.path, "et")
        s.set_comment("f::k", "odd")
        s.add_highlight("f::k", "Tere maailm", 5, 11)
        s2 = ReviewState(self.path, "et")
        v = s2.view("f::k")
        self.assertEqual(v["comment"], "odd")
        self.assertEqual(v["highlights"], [{"start": 5, "end": 11, "text": "maailm"}])
        self.assertEqual(v["status"], "flagged")
        self.assertEqual([n for n in os.listdir(os.path.dirname(self.path))], ["et.json"])

    def test_utf16_offsets(self):
        text = "a\U0001F600b tere"      # an emoji takes two UTF-16 units
        s = ReviewState(self.path, "et")
        self.assertEqual(s.add_highlight("m", text, 5, 9), "tere")
        self.assertEqual(utf16_slice(text, 3, 4), "b")

    def test_overlap_replaced_and_removed(self):
        s = ReviewState(self.path, "et")
        s.add_highlight("m", "abcdef ghij", 0, 3)
        s.add_highlight("m", "abcdef ghij", 2, 6)
        self.assertEqual(len(s.view("m")["highlights"]), 1)
        s.remove_highlight("m", 2, 6)
        self.assertEqual(s.view("m")["highlights"], [])
        with self.assertRaises(StateError):
            s.add_highlight("m", "abc", 2, 9)

    def test_suggestion_flow(self):
        s = ReviewState(self.path, "et")
        a = s.add_suggestion("m", "uus", "owner")
        self.assertEqual(s.view("m")["status"], "suggested")
        s.decide_suggestion("m", a["id"], "accept")
        self.assertEqual(s.view("m")["status"], "accepted")
        self.assertEqual(s.accepted()[0][1]["text"], "uus")
        s.decide_suggestion("m", a["id"], "reject")
        self.assertEqual(s.view("m")["status"], "flagged")
        bad = s.add_suggestion("m", "x", "owner", error="nope")
        with self.assertRaises(StateError):
            s.decide_suggestion("m", bad["id"], "accept")

    def test_broken_file_is_kept(self):
        os.makedirs(os.path.dirname(self.path))
        with open(self.path, "w") as f:
            f.write("{oops")
        with self.assertRaises(StateError):
            ReviewState(self.path, "et")
        self.assertTrue(any(".broken-" in n for n in os.listdir(os.path.dirname(self.path))))

    def test_glossary(self):
        s = ReviewState(self.path, "et")
        s.set_glossary([{"en": "layer", "tr": "kiht", "note": ""}, {"en": "", "tr": "x"}])
        self.assertEqual(ReviewState(self.path, "et").glossary(), [{"en": "layer", "tr": "kiht", "note": ""}])


class ApplyTest(unittest.TestCase):
    def setUp(self):
        self.root = make_repo(git=True)
        self.rv = Review(self.root, "et", state_path=os.path.join(tempfile.mkdtemp(), "s.json"))

    def accept(self, key, text):
        mid = "demo.ftl::" + key
        s = self.rv.add_suggestion(mid, text)
        self.assertIsNone(s["error"], s["error"])
        self.rv.state.decide_suggestion(mid, s["id"], "accept")
        return mid

    def test_apply_changes_only_the_value(self):
        before = open(et_path(self.root), encoding="utf-8").read()
        mid = self.accept("demo-title", "Kokkujooksmise aruanne { $n } / { $total }")
        self.accept("demo-count", "{ $n ->\n    [one] { $n } tase\n   *[other] { $n } taset\n}")
        r = self.rv.apply()
        self.assertNotIn("refused", r)
        self.assertEqual(len(r["applied"]), 2)
        after = open(et_path(self.root), encoding="utf-8").read()
        self.assertIn("demo-title = Kokkujooksmise aruanne { $n } / { $total }", after)
        self.assertEqual(self.rv.state.view(mid)["status"], "fixed")
        diff = [l for l in after.split("\n") if l not in before.split("\n")]
        self.assertEqual(diff, ["demo-title = Kokkujooksmise aruanne { $n } / { $total }",
                                "        [one] { $n } tase", "       *[other] { $n } taset"])
        self.assertIn("# Title of the crash dialog.", after)

    def test_selector_and_multiline_rebuild(self):
        self.accept("demo-count", "{ $n ->\n    [one] { $n } tase\n   *[other] { $n } taset\n}")
        self.accept("demo-multi", "Esimene osa { $x }\nteine osa")
        pv = self.rv.apply_preview()
        self.assertTrue(all(not c["error"] for c in pv["changes"]), pv["changes"])
        self.rv.apply()
        parsed = fluent.parse_file(et_path(self.root))
        m = {x.key: x for x in parsed.messages}
        self.assertEqual(m["demo-multi"].raw, "Esimene osa { $x }\nteine osa")
        self.assertEqual(m["demo-count"].variants[0]["text"], "{ $n } tase")

    def test_refuses_dropped_variable_and_broken_selector(self):
        en = self.rv.src["demo.ftl"]
        old = next(m for m in self.rv.tgt["demo.ftl"].messages if m.key == "demo-title")
        enm = next(m for m in en.messages if m.key == "demo-title")
        self.assertIn("variables", applymod.check_replacement(old, enm, "Aruanne { $n }"))
        self.assertIn("variables", applymod.check_replacement(old, enm, "Aruanne { $n } { $total } { $zzz }"))
        oldc = next(m for m in self.rv.tgt["demo.ftl"].messages if m.key == "demo-count")
        enc = next(m for m in en.messages if m.key == "demo-count")
        self.assertIn("selector", applymod.check_replacement(oldc, enc, "{ $n } kihti"))
        self.assertIn("selector", applymod.check_replacement(oldc, enc, "{ $n ->\n    [one] a\n    [other] b\n}"))

    def test_invalid_suggestion_cannot_be_accepted(self):
        s = self.rv.add_suggestion("demo.ftl::demo-title", "Aruanne")
        self.assertTrue(s["error"])
        with self.assertRaises(StateError):
            self.rv.state.decide_suggestion("demo.ftl::demo-title", s["id"], "accept")
        s = self.rv.add_suggestion("demo.ftl::demo-recover", " tühik ees")
        self.assertIn("space", s["error"])

    def test_dirty_files_need_confirmation(self):
        self.accept("demo-recover", "Alles on need varukoopiad.")
        with open(et_path(self.root), "a", encoding="utf-8") as f:
            f.write("\n")
        r = self.rv.apply()
        self.assertEqual(r["refused"], "dirty")
        self.assertIn("Alles on need taastekoopiad.", open(et_path(self.root), encoding="utf-8").read())
        r = self.rv.apply(confirm_dirty=True)
        self.assertEqual(len(r["applied"]), 1)
        self.assertIn("varukoopiad", open(et_path(self.root), encoding="utf-8").read())

    def test_no_backup_files(self):
        self.accept("demo-recover", "Alles on need varukoopiad.")
        self.rv.apply()
        d = os.path.dirname(et_path(self.root))
        self.assertEqual(os.listdir(d), ["demo.ftl"])

    def test_exact_diff_lines(self):
        self.accept("demo-recover", "Alles on need varukoopiad.")
        c = self.rv.apply_preview()["changes"][0]
        self.assertEqual(c["old_lines"], ["demo-recover = Alles on need taastekoopiad."])
        self.assertEqual(c["new_lines"], ["demo-recover = Alles on need varukoopiad."])
        self.assertEqual(c["line"], 5)


class ReviewTest(unittest.TestCase):
    def test_list_counts_and_parity(self):
        root = make_repo()
        with open(et_path(root), "a", encoding="utf-8") as f:
            f.write("demo-extra = Lisa\n")
        rv = Review(root, "et", state_path=os.path.join(root, "s.json"))
        d = rv.list()
        self.assertEqual(d["total"], 5)
        self.assertEqual(d["counts"]["flagged"], 0)
        self.assertEqual([x["key"] for x in d["parity"]["extra"]], ["demo-extra"])
        self.assertEqual(d["locales"], ["et"])
        self.assertEqual(rv.item("demo.ftl::demo-title")["en"]["examples"], {"n": "3", "total": "12"})

    def test_export(self):
        root = make_repo()
        rv = Review(root, "et", state_path=os.path.join(root, "s.json"))
        mid = "demo.ftl::demo-recover"
        rv.state.add_highlight(mid, "Alles on need taastekoopiad.", 0, 5)
        rv.state.set_comment(mid, "sounds odd")
        md = rv.export_markdown()
        self.assertIn("demo.ftl `demo-recover`", md)
        self.assertIn('"Alles"', md)
        self.assertIn("sounds odd", md)
        self.assertNotIn("demo-title", md.split("## Glossary")[0])

    def test_stale_highlights_hidden_after_text_change(self):
        root = make_repo()
        rv = Review(root, "et", state_path=os.path.join(root, "s.json"))
        mid = "demo.ftl::demo-recover"
        rv.state.add_highlight(mid, "Alles on need taastekoopiad.", 0, 5)
        with open(et_path(root), "w", encoding="utf-8") as f:
            f.write(open(et_path(root), encoding="utf-8").read().replace("Alles", "Veel"))
        rv.reload()
        it = rv.item(mid)
        self.assertEqual(it["highlights"], [])
        self.assertTrue(it["highlights_stale"])

    def test_terms(self):
        pairs = [("a%d" % i, "Open the sheet %d" % i, "Ava leht %d" % i) for i in range(3)]
        pairs += [("b%d" % i, "Close the sheet %d" % i, "Sulge tabel %d" % i) for i in range(3)]
        res = terms.find_inconsistent(pairs)
        words = [r["word"] for r in res]
        self.assertIn("sheet", words)
        warn = terms.glossary_warnings(pairs, [{"en": "sheet", "tr": "leht", "note": ""}])
        self.assertEqual(warn[0]["messages"], 6)
        self.assertEqual(warn[0]["missing_count"], 3)


class PromptTest(unittest.TestCase):
    def setUp(self):
        self.root = make_repo()
        self.dir = tempfile.mkdtemp()
        self.fake = make_fake_claude(self.dir)
        with open(os.path.join(self.dir, "canned.json"), "w") as f:
            f.write(CANNED)
        os.environ["FAKE_DIR"] = self.dir
        self.rv = Review(self.root, "et", state_path=os.path.join(self.dir, "s.json"), claude_bin=self.fake)
        self.mid = "demo.ftl::demo-title"

    def tearDown(self):
        os.environ.pop("FAKE_MODE", None)

    def test_prompt_content(self):
        self.rv.state.add_highlight(self.mid, "Jooksutuse aruanne { $n } / { $total }", 0, 10)
        self.rv.state.set_comment(self.mid, "wrong word")
        p = self.rv.make_prompt(self.mid)
        for part in ["Crash report { $n } of { $total }", "Jooksutuse aruanne", '"Jooksutuse"', "wrong word",
                     "crash report -> kokkujooksmise aruanne", "demo-recover", "Title of the crash dialog",
                     "{ $n }", "strict JSON", "exactly 3"]:
            self.assertIn(part, p)

    def test_parse_alternatives(self):
        alts = promptmod.parse_alternatives('text ```json\n' + CANNED + '\n``` more')
        self.assertEqual(len(alts), 3)
        self.assertEqual(alts[0]["why"], "Standard term")
        with self.assertRaises(ValueError):
            promptmod.parse_alternatives("nothing")
        with self.assertRaises(ValueError):
            promptmod.parse_alternatives('{"alternatives": []}')

    def test_suggest_with_fake_claude(self):
        os.environ["FAKE_MODE"] = "json"
        added = self.rv.suggest(self.mid, "haiku", 20)
        self.assertEqual(len(added), 3)
        self.assertIsNone(added[0]["error"])
        self.assertIn("variables", added[1]["error"])
        self.assertIn("space", added[2]["error"])
        self.assertEqual(open(os.path.join(self.dir, "args.txt")).read().strip(), "-p --model haiku")
        self.assertIn("Crash report", open(os.path.join(self.dir, "stdin.txt")).read())
        cwd = open(os.path.join(self.dir, "cwd.txt")).read().strip()
        self.assertNotIn(os.path.realpath(self.root), os.path.realpath(cwd))

    def test_fenced_json(self):
        os.environ["FAKE_MODE"] = "fence"
        self.assertEqual(len(self.rv.suggest(self.mid, "sonnet", 20)), 3)

    def test_more_options_avoids_rejected(self):
        os.environ["FAKE_MODE"] = "json"
        added = self.rv.suggest(self.mid)
        self.rv.state.decide_suggestion(self.mid, added[1]["id"], "reject")
        self.rv.suggest(self.mid, more=True)
        self.assertIn("Do not repeat", open(os.path.join(self.dir, "stdin.txt")).read())

    def test_errors(self):
        os.environ["FAKE_MODE"] = "bad"
        with self.assertRaises(promptmod.AskError) as c:
            self.rv.suggest(self.mid)
        self.assertIn("JSON", str(c.exception))
        os.environ["FAKE_MODE"] = "fail"
        with self.assertRaises(promptmod.AskError) as c:
            self.rv.suggest(self.mid)
        self.assertEqual(c.exception.code, 1)
        self.assertIn("Not logged in", c.exception.stderr)
        os.environ["FAKE_MODE"] = "slow"
        with self.assertRaises(promptmod.AskError) as c:
            self.rv.suggest(self.mid, timeout=1)
        self.assertEqual(c.exception.code, "timeout")
        with self.assertRaises(promptmod.AskError):
            promptmod.ask_claude("x", binary="/nonexistent/claude")
        with self.assertRaises(promptmod.AskError):
            promptmod.ask_claude("x", binary=self.fake, model="gpt")


if __name__ == "__main__":
    unittest.main()
