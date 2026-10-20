# Crash reports (PLAT-20)

Kadakas can send a report to the developers when it stops because of an error. The person sees the report first. The person sees the exact text that goes out. Nothing is sent without the decision of the person.

Code: `crates/kadakas-crash` holds the file, the scrubbing, the signature, the folder of kept reports and the report. The app side is `crash_ui.rs` and `privacy_ui.rs` in `kadakas-app`. The launcher side is in `kadakas-update` (`runs.rs`, `supervisor.rs`). Sending uses the feedback channel of UI-14 (`kadakas-feedback`, `docs/feedback.md`).

## The path of a report

1. **The app dies.** The app calls `kadakas_crash::capture::install` at the start (`crash_ui::install`, in `main`).
   - A **panic** runs the panic hook. The hook writes the crash file `<launcher folder>/runs/<run id>.crash.json`. The write is atomic and owner only. Then the hook calls the hook that was there before, so the message still goes to the terminal. Kadakas catches some panics on purpose (`catch_unwind` in `Session::guarded`). A `CatchGuard` tells the hook that such a panic is no crash. No file is written for it.
   - A **native crash** is a signal or an exception in a driver or a library, for example `SIGSEGV`. The handler of the `crash-handler` crate (Embark Studios, MIT OR Apache-2.0) runs. It writes the same file and lets the process die. On Unix a timer of 5 seconds ends the process when the handler hangs.
2. **The launcher sees the end.** The supervisor looks for the clean-exit mark when the app ends (PLAT-16).
   - No mark and a crash file: the file moves to `<launcher folder>/crash-reports/<run id>.json`. The folder keeps 10 files at most. When an eleventh file comes, the oldest file goes.
   - No mark, no file, and the system names a crash signal: the launcher writes a small report of its own with the name of the signal. The signals are `SIGSEGV`, `SIGABRT`, `SIGBUS`, `SIGILL`, `SIGFPE` and `SIGTRAP`. On Windows the exit code names an exception.
   - No mark, no file, and no crash signal: there is no report. This is a force quit, a shutdown or a power loss.
   - A mark (a clean exit): the crash file of the run is deleted. The app deletes it too (`clean_exit`). A thread that panicked while the app lived on is not reported.
   - The same unclean end sets the crash mark of PLAT-16. The next start falls back to the previous version.
   - A person can start the app by hand, without a launcher. The app then makes a run id of its own (`5ca1ab1e-<pid>-<time>`). At its next start the app keeps the files of such runs when their process is gone.
3. **The next start asks.** `CrashUi::start` reads the kept reports and follows the setting.
   - *Ask each time* is the default. A window shows the summary, a field for a note, a field for a contact and the **exact text** that goes out. The buttons are **Send**, **Do not send** and **Always send**. Closing the window decides nothing. The reports stay for the next start. Nothing stays in the outbox for a report that has no decision.
   - *Always send*: the reports go without a question. The scrubbing is the same. There is no note and no contact.
   - *Never send*: the reports are deleted.
   - **Always send** in the window sets the setting, and sends the reports that wait. The person takes it back in Preferences > Privacy > Crash reports.
4. **Sending.** The report goes into the outbox of UI-14 as a report that waits for review. Nothing is sent at this point. **Send** stores the shown text as the approved message. The sending thread of the app sends it with the retries and the back-off of the outbox. When the computer is offline, the report waits in the outbox, in the feedback folder of the user data folder. At most 10 crash reports wait there. When one more comes, the oldest is withdrawn (`report::trim_outbox`).
5. **Grouping.** The title of the report holds the signature: `Crash <signature> in <version>`. Two equal crashes in the same version merge in the outbox, and the counter goes up. The report is sent once with the field `count`. The server groups the reports by the signature.

## Rules added after review

- **Send sends what is on the screen.** Before it stores the message, the window builds the message again. If the text differs from the text on the screen (for example the person turned the installation ID off while the window was open), the window shows the new text, says so, and sends nothing. The person presses Send again. Always send does the same check before it changes the setting.
- **Never send and the ID switch reach queued reports.** Setting *Never send* withdraws every crash report that waits in the outbox (approved or not), and the sending thread skips crash reports while the setting is *Never send*. Turning the switch off removes the field `installation` from every approved message that has not gone out yet, at the moment of sending (`without_installation`, in `outgoing` and `claim`). Turning it on again does not put the ID back into a message that was approved without it.
- **The native handler does no work that can hang.** A background thread rebuilds the bytes of the native report every 2 seconds into one of two fixed buffers (log lines, command names, memory, run time). The handler only writes the active buffer, plus the name and the code of the signal or exception, with `open`, `write` and `rename` (Unix). It does not allocate, format, serialise or take a lock. On Unix a timer of 5 seconds (`alarm`) ends a handler that hangs; the launcher names that end `SIGALRM` and writes a small report. On Windows the handler uses `std::fs` (it allocates for the path), and a watchdog thread ends the process after 5 seconds with the exit code `0xE0000001`; the launcher names that end `KADAKAS_CRASH_HANDLER_HUNG`. The log lines of a native report can be up to 2 seconds old. A native report has no stack.
- **A panic followed by an abort keeps its backtrace.** The panic hook sets a flag. The native handler writes nothing when the flag is set. A panic in code that cannot unwind (`extern "C"`) is the case: the hook runs, then the program aborts with SIGABRT.
- **Only the first uncaught panic of a run is written.** It is the cause. Later panics are often its effect.
- **A survived panic is not a crash.** The launcher keeps a panic file as the crash only when the process failed: exit code 101 or a crash signal. If the process ended in another known way (a kill, a shutdown, another exit code), the report says `survived panic, the app ended later (N s after the panic)`, and the object `crash` has `survived_gap_secs`. When the launcher does not know how the process ended (a launcher that died), the file is kept as it is.
- **The size of the file is proved.** A function name or a path in a frame is cut to 400 bytes, the other texts to 2000 bytes. JSON escaping can make a text six times longer, so `CrashFile::bounded` then takes away old log lines, old command names and deep frames until the written file is below 252 KiB (the limit is 256 KiB). The fixed fields alone are at most about 110 KB even when every character is escaped. A property test checks that any bounded file parses.
- **An oversized or damaged file is never deleted silently.** The launcher moves it to `crash-reports/<id>.unreadable` and writes a small report (`UNREADABLE_CRASH_FILE`, with the size).
- **URLs.** Only the official Kadakas domain (the domain of the feedback address) keeps its host name. Every other host is `<host>`.
- **File names with spaces.** A name with a known extension (`.kad`, `.dxf`, `.json` and others) is removed whole, also without quotes: the text goes back to the last quote, bracket, `:`, `,`, `;` or `=`.
- **The home folder** is read from `HOME`, `USERPROFILE` and `HOMEDRIVE` with `HOMEPATH`, all of them.

## What the file holds

The file holds raw facts. It stays on the computer. It is not sent as it is.

- the message, the thread and the frames (function, file, line)
- the Kadakas version and channel, the system and the architecture
- the graphics adapter, the driver and the wgpu backend (`Gpu`, set when the adapter is known)
- the last 50 log lines
- the names of the last 40 commands and API methods
- the peak memory (Unix) and the uptime

The names of commands come from `Session::guarded` (the JSON-RPC method) and from the key actions of the window. `ring::note_command` accepts a plain name only. A parameter or a piece of text cannot get in.

`report::assemble` makes the report from the file. It scrubs every text. It builds the message of the feedback channel (protocol 1) with one more object, `crash`.

## Scrubbing

`kadakas_crash::scrub` is a pure function with tests. It uses the rules of the feedback channel (`kadakas_feedback::scrub::harsh`): e-mail addresses, keyed secrets, bearer tokens, token shapes, long mixed words, names in quotes and stems of file names. It adds these rules:

| What | Becomes |
|---|---|
| An absolute path (`/Users/ann/My Plan.kad`, `C:\Users\Ann Lee\x`, `\\server\share\x`, `~/x`) | `<path>`. A path runs to the end of the line, a quote, a closing bracket, `: ` or `, `. More text goes than needed, never less. |
| A URL | Scheme and host only, as `https://host/<url-path>`. The user, password, path, query and fragment go. A `file:` URL is `file://<path>`. |
| The login name of the computer (a word of 3 letters or more, any case) | `<user>` |
| The home folder | `<home>` |
| A string between two double quotes in a line | `"<text>"`. The Debug print of an error holds names and paths, also with escaped quotes. |
| The source file of a frame | `crates/kadakas-...` for our sources, `registry/name-1.2.3/src/lib.rs` for a crate, `rustc/library/...` for the standard library. Any other file is `<path>/<file>.ext`. |
| The hash of a symbol (`::h0123456789abcdef`) | Removed. |

The scrubbing keeps the standard fragments of panic messages: `` `Option::unwrap()` ``, `` `None` `` and `` `Err` ``. Document content never enters the file. Kadakas does not put it into a log line or a command name. The panic message of a Rust program is the only text that can hold some, and it is scrubbed as above.

What stays: names of functions of Kadakas and of its libraries, line numbers of our files, the version, the system, the adapter and numbers.

## The signature

`signature::of` is the first 8 bytes (16 hex digits) of a SHA-256 hash. The input is a format tag, the version, the kind (`panic` or `native`) and the top 5 frames of the backtrace. The panic machinery and the runtime are skipped first (`std::panicking`, `core::panicking`, `rust_begin_unwind`, `std::rt`, the crash handler and so on). A frame is its function name without the hash. Files, lines, addresses, the message, the thread and the uptime are not part of the input. A native crash without frames uses the name of the signal or exception (the first word of the message).

## Settings and the installation ID

Preferences > Privacy has these settings:

- **Crash reports**: Ask each time (default), Always send, Never send. Kadakas stores it in the feedback settings (`settings.json` in the feedback folder, key `crash_policy`).
- **Installation ID**: a random UUID version 4. Kadakas makes it at the first need (the first report or the first look at Preferences > Privacy). It is not derived from the hardware, the user, the network or the time. Preferences shows it with **Reset** and a switch **Include the installation ID in reports** (on by default). Reset makes a new random ID. Reports with the old ID can no longer be linked to the new ones. With the switch off, the field `installation` is left out of every message, of crash reports and of the reports of UI-14. The ID stays, so the switch does not make a new ID when it goes on again.

The spec says the state folder. The ID is not in the state folder of the launcher. It is in `settings.json` of the feedback folder (`KADAKAS_FEEDBACK_DIR`, else `<data folder>/feedback`). UI-14 made it first. Crash reports use the same ID.

### Privacy notice (GDPR)

The installation ID is pseudonymous personal data under the GDPR. It names an installation. A person can be found behind it when other data is added, for example a contact in a note. The privacy notice of the project must say these things:

- The ID is random. Kadakas makes it on the computer. It is not derived from hardware, user or network.
- Kadakas sends it with crash reports and feedback reports unless the person turns the switch off. Kadakas never sends it alone.
- The person can reset it or switch it off in Preferences > Privacy.
- A request to delete the reports of an installation uses the ID. The person copies it from Preferences > Privacy. A Reset removes the old ID from the computer, so the person keeps a copy before a Reset.
- The legal basis is the consent of the person. The person gives it with Send or with Always send. The person takes it back with the setting.

The contact field is optional. Kadakas sends it only when the person types it, and only in that report. It is not scrubbed, because an e-mail address is the purpose of the field. The note is scrubbed like the other texts.

## Limits (what is not done)

- **No minidump.** A native crash leaves the signal or exception with its code and address, the log lines and the command names. It has no stack. On Linux the handler tries the stack of the crashing thread. On macOS and Windows the handler runs on another thread. A real minidump needs a second process that waits for the crash (`minidumper` of Embark Studios, same license). That changes the start of the program. A dump also holds memory of the program, and that memory can hold document content. A dump needs its own scrubbing design. This is open.
- **No version of the operating system.** The report has the name of the system and the architecture only.
- **Memory** is the peak resident size on Unix. Windows gives no value.
- `resume_unwind` does not call the panic hook, so a resumed panic inside a `CatchGuard` writes no file.
- Some code catches a panic with `catch_unwind` and no `CatchGuard`. These places are in `kadakas-core`, `kadakas-render` and the scripting layer, outside `Session::guarded`. Such a panic writes a crash file. A clean exit deletes the file. If the app is killed later without a clean exit, the old file is reported. It is a real bug of Kadakas, but it is perhaps not the cause of the end.
- The tests run a real panic and a real null write in child processes on the computer that runs the tests. The Linux and Windows branches of the native handler compile only there. They were not run.
- The server that stores the reports does not exist yet (`kadakas-server`). The address is the placeholder of UI-14.

## Tests

- `scrub`: paths in backtraces and messages, home folders of macOS, Windows and Linux, URLs with tokens, e-mail patterns, user names and generated names (property tests).
- `signature`: stability, and what changes it.
- `queue`: the limit of 10 and the order, ids that cannot name other paths, standalone runs.
- `report`: a fake crash file becomes a scrubbed report. The shown text is the sent text. The installation ID follows the switch. The limit of 10 in the outbox.
- `kadakas-update`: the supervisor keeps, deletes and writes reports by the way the run ended.
- `tests/capture_child.rs`: a real panic and a real null write in a child process leave a crash file. The file becomes a scrubbed report.
- `kadakas-app` (`crash_ui`, `privacy_ui`): Never, Always and Ask. The dialog does not send before the choice. Always send in the dialog sets the setting. Closing decides nothing. The same crash counts once. The windows draw in en-US and et.
- `kadakas-feedback`: reset, switch and policy in the hub.
