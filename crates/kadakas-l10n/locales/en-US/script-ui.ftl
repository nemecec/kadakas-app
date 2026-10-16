## script-ui: script_ui.rs

script-ui-script = Script: { $e }
script-ui-scripts = Scripts
script-ui-python = Python: { $version }
script-ui-no-python-found-install-python3-or-uv = No Python found. Install python3 or uv, or set KADAKAS_PYTHON.
script-ui-a-script-runs-only-when-you-press = A script runs only when you press Run. It can change the open document. A .js file runs inside Kadakas in a sandbox, as one undo step; a .py file runs as a program next to it.
script-ui-folder = Folder
script-ui-choose = Choose...
script-ui-refresh = Refresh
script-ui-no-py-files-in-this-folder = No .py or .js files in this folder.
script-ui-run = Run
script-ui-stop = Stop
script-ui-clear-console = Clear console
script-ui-console = Console
script-ui-the-script-failed = the script failed
script-ui-plug-in-object-2 = Plug-in object: { $message }
script-ui-plug-in-object-the-script-thread-ended = Plug-in object: the script thread ended without an answer
script-ui-trust-this-plug-in = Trust this plug-in?
script-ui-comes-from = Comes from: { $origin }
script-ui-script-hash-sha-256 = Script hash (SHA-256): { $hash }
script-ui-this-is-a-python-program-if-you = This is a Python program. If you trust it, it runs with your rights, on your files and your network, each time the object is regenerated. Read it first.
script-ui-trust-this-script = Trust this script
script-ui-do-not-trust = Do not trust
script-ui-trust-is-for-this-exact-text-a = Trust is for this exact text. A changed script asks again.
script-ui-plug-in-object-3 = Plug-in object: { $name }
script-ui-the-script-is-not-trusted-so-it = ⚠ The script is not trusted, so it does not run.
script-ui-review-script = Review script...
script-ui-the-script-failed-2 = ⚠ The script failed
script-ui-the-shapes-are-from-the-last-good = The shapes are from the last good run.
script-ui-text = ⚠ { $problem }
script-ui-reset = Reset
script-ui-run-the-script-again = Run the script again
script-ui-waiting-to-run-the-script = Waiting to run the script...
script-ui-running-the-script-seconds = Running the script... { $seconds } s

## script-ui: JavaScript programs (SCR-08)

script-ui-js-running = Script running
script-ui-js-running-detail = { $seconds } s, { $calls } calls
script-ui-js-edits-blocked = The drawing cannot be edited until the script ends. The whole run is one undo step.
script-ui-js-stop = Stop
script-ui-js-stopping = Stopping...
script-ui-js-ran = Script finished: { $calls } calls, { $steps } undo step, { $ms } ms
script-ui-js-failed = Script failed: { $message }
script-ui-js-cannot-read = Cannot read { $path }: { $e }
