import os
import shutil
import subprocess
import unittest


@unittest.skipUnless(shutil.which("node"), "node is not installed")
class OffsetsJsTest(unittest.TestCase):
    def test_js(self):
        here = os.path.dirname(os.path.abspath(__file__))
        for name in ("test_offsets.js", "test_editing.js", "test_dom.js"):
            r = subprocess.run(["node", os.path.join(here, name)], capture_output=True, text=True, timeout=60)
            self.assertEqual(r.returncode, 0, name + r.stdout + r.stderr)


if __name__ == "__main__":
    unittest.main()
