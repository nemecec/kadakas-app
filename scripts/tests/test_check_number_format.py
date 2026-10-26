"""Tests of scripts/check_number_format.py. Run: python3 -I this_file."""
import importlib.util
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "check_number_format.py")
spec = importlib.util.spec_from_file_location("check_number_format", SCRIPT)
cnf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cnf)


def marked(chain):
    return any(k in chain for k in cnf.MARKERS)


class Widgets(unittest.TestCase):
    def test_a_chain_is_found_across_lines_and_nested_brackets(self):
        text = """
        ui.add(
            DragValue::new(&mut s.spread[0])
                .speed(f(1.0, (2, 3)))
                .range(1.0..=100000.0),
        );
        let x = 1;
        """
        found = cnf.widgets(text)
        self.assertEqual(len(found), 1)
        chain = found[0][2]
        self.assertTrue(chain.endswith(".range(1.0..=100000.0)"), chain)
        self.assertFalse(marked(chain))

    def test_markers_are_seen(self):
        for ext in [".length()", ".degrees()", ".ui_number()", ".paper_mm()", ".direction()"]:
            chain = cnf.widgets(f"ui.add(DragValue::new(v).speed(1.0){ext});")[0][2]
            self.assertTrue(marked(chain), ext)
        s = "ui.add(egui::Slider::new(&mut pos, 0..=last).show_value(false))"
        self.assertTrue(marked(cnf.widgets(s)[0][2]))
        s = "DragValue::new(&mut h).custom_formatter(|v, _| v.to_string())"
        self.assertTrue(marked(cnf.widgets(s)[0][2]))

    def test_a_string_with_brackets_does_not_confuse_the_scan(self):
        text = 'ui.add(DragValue::new(v).suffix(" )(")); let y = DragValue::new(w);'
        found = cnf.widgets(text)
        self.assertEqual(len(found), 2)
        self.assertEqual(found[0][2], 'DragValue::new(v).suffix(" )(")')

    def test_the_test_module_is_cut_off(self):
        text = "fn a() {}\n#[cfg(test)]\nmod tests { fn b() { DragValue::new(v); } }\n"
        self.assertNotIn("DragValue", cnf.code_part(text))

    def test_raw_float_formatting_is_found_in_every_form(self):
        bad = [
            'format!("{:.2}", x)',
            'format!("{v:.3} m")',
            'format!("{x:.1}")',
            'format!("{:>6.1}", x)',
            'format!("{x:>8.2} mm")',
            'format!("{:.10}", x)',
            'format!("{:.prec$}", x, prec = 3)',
            'format!("{:.*}", 2, x)',
            'write!(f, "{:.1} MB", mb)',
            'writeln!(out, "{x:<5.2}")',
            'println!("{:.3}", v)',
            'eprintln!("{:.3}", v)',
            'tr!("a", x = format!("{:.3}", info.scale.x))',
            'format!(\n    "{a} and {b:.2}",\n    a = 1,\n)',
            'panic!("{:.1}", v)',
        ]
        for text in bad:
            self.assertTrue(cnf.raw_floats(text), text)
        good = [
            'format!("{v:.0}")',
            'format!("{:.0}", x)',
            'format!("{}", n)',
            'format!("{x:>6}")',
            'tr!("a", x = 1)',
            'format!("{:?}", v)',
            'let s = "{:.2} is only text";',
        ]
        for text in good:
            self.assertFalse(cnf.raw_floats(text), text)

    def test_the_fix_adds_the_helper_after_the_chain(self):
        text = "ui.add(DragValue::new(v).speed(1.0));\n"
        start, end, chain = cnf.widgets(text)[0]
        fixed = text[:end] + ".ui_number()" + text[end:]
        self.assertEqual(fixed, "ui.add(DragValue::new(v).speed(1.0).ui_number());\n")
        self.assertIn(cnf.IMPORT, cnf.with_import("use a::b;\nfn f() {}\n"))
        self.assertEqual(cnf.with_import(cnf.IMPORT + "\n"), cnf.IMPORT + "\n")

    def test_the_real_tree_passes(self):
        self.assertEqual(cnf.main([]), 0)


if __name__ == "__main__":
    unittest.main()
