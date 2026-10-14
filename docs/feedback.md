# Feedback: bug reports and feature requests (API-47, API-48, UI-14)

Agents and people can tell the Kadakas team what is missing or broken. A report goes to a local outbox first. It leaves the computer only with the consent of a person. This page describes the reports, the outbox, the consent rules and the HTTP protocol that the feedback server (the separate `kadakas-server` repository) will implement.

The code is in these places:

| Part | Where |
|---|---|
| Model, scrubbing, dedupe, outbox, locking, payload, sender with retry (pure, wasm-compatible) | `crates/kadakas-feedback` |
| Folder store, HTTP sender, sending thread (native only) | `crates/kadakas-feedback` (`store`, `http`, `dispatch`) |
| `feedback.*` methods, error hints, panic guard, model excerpt, review model | `crates/kadakas-api/src/feedback_api.rs` and `feedback_api/` |
| MCP tool `feedback`, instructions, `--feedback-send`, `kadakas feedback review`, `list`, `retry` | `crates/kadakas-cli` |
| Form logic and consent flow (no egui) | `crates/kadakas-ui-core/src/feedback.rs` |
| Status bar button, form, Help > Feedback, review, consent dialog | `crates/kadakas-app/src/feedback_ui.rs` |
| Python | `doc.feedback` in `python/kadakas/document.py` |

## For agents

- The MCP `initialize` instructions say it: if a task needs a feature that Kadakas lacks, file a feature request with the `feedback` tool. If a method fails in a way it must not, file a bug report.
- The `feedback` tool is in both MCP modes (domain tools and `--mcp-flat`), second in the list after `describe`. Its `action` is `submit` (default), `list`, `get` or `withdraw`.
- `describe` has a `feedback` section with the rules and examples.
- An error of an unknown method, an unsupported option (`unknown field`, `unknown variant`, `unsupported`, `not supported`) or an internal error (code -32603, kind `internal`) has `data.feedback`:

```json
{"hint": "If the name is right according to `describe` and the feature is missing, file a feature request with `feedback`.",
 "draft": {"kind": "feature_request", "title": "Method plants.teleport is missing",
           "method": "plants.teleport"}}
```

  The draft is a valid `feedback.submit` call. The title of an unsupported option names the option. The draft has no error text: a bug report takes the error of the last failed call by itself. Over MCP, the text of the failed tool call ends with the hint and the draft, because a client shows the text only. Other errors have no hint, so the token budget stays small.

## Methods

`feedback.submit {kind, title, body?, wanted?, tried?, method?, error?, severity?, client?, attach?}`

- `kind`: `bug`, `feature_request` or `question`. `severity`: `low`, `medium`, `high`.
- A bug report without `method` and `error` takes them from the last failed call of the session. That error text is scrubbed harder than the rest (see Scrubbing).
- Kadakas adds: version, operating system, the MCP client name and version (from `initialize`. The CLI adds them to the call), and the origin (`agent`, `user`, `crash`).
- The result is `{id, merged, count, status, attachments, note}`. The id is local (`fb-12`). An equal report is found before any attachment is made, so a merge costs no picture.
- `feedback.list {status?, limit?}` gives `{reports, needs_review, reports_of_people, sending}`.
- `feedback.get {id}` gives the report, the kinds and sizes of its attachments, and the status. It never gives the attachment content.
- `feedback.withdraw {id}` takes back a report that was not sent.

**Agents see their own reports only.** `list`, `get` and `withdraw` work on reports with the origin `agent`. A report of a person (`user` or `crash`) is "not found" for `get` and `withdraw`, and in `list` it is only a count (`reports_of_people`). An agent never merges into the report of a person, and a person never merges into the report of an agent.

No method approves or sends an attachment. Extra fields such as `approve` or `status` are ignored.

## Limits

| What | Limit |
|---|---|
| Title / text / wanted / tried / error / method | 120 / 4000 / 2000 / 2000 / 2000 / 80 characters |
| Reports in the outbox | 200 (the oldest sent or withdrawn report makes room, else the call fails) |
| Reports one session of an agent may make (merged ones count) | 30. People are not limited. |
| Reports in the outbox for each side | 100 for agents and 100 for people, so agents cannot fill the space of a person |
| Attachments that wait for review | 32 MiB for agents and 32 MiB for people |
| Picture (PNG) | 1 MiB |
| Model excerpt | 512 KiB and 200 objects |
| Call log | 40 calls, 32 KiB |
| Log lines | 50 lines, 16 KiB |
| Message to the server | 6 MiB |
| Counted failed starts per report | 8, then a person must press "Try again" or run `kadakas feedback retry` |

A report with the same key (kind, normalized title, method) and the same kind of writer (agent or person) is merged: the counter of the first report goes up. A merged report drops its new attachments. A withdrawn report does not absorb a new one, and neither does a report that was sent: the new report is a report of its own.

## Scrubbing

Every text of a report, and the call log, goes through `kadakas_feedback::scrub` when the report is made, whatever the caller did.

- Absolute paths (`/home/ann/x.kad`, `~/x`, `C:\Users\ann\x.kad`, `\\server\share\x`, `file:///home/ann/x.kad`) become `<path>/x.kad`. The file name stays. A path without a file name becomes `<path>`.
- Bearer tokens, values of keys such as `token`, `secret`, `password`, `api_key`, `authorization` and `cookie`, the password of an address, known token shapes (`sk-`, `ghp_`, JWT, `AKIA`) and long words that mix letters and digits become `<secret>`.
- E-mail addresses become `<email>`.
- Object ids (UUIDs) stay: they help to find the problem.
- Control characters go. A text is cut to its limit.

**Harder scrub for what Kadakas fills in.** The error of the last failed call, the entries of the call log and the log lines come from the session, not from a writer who chose them. They get `scrub::harsh`: a file name keeps its extension and loses its stem (`<file>.kad`), and a name in quotes becomes `<name>`, unless it looks like an API word (`bogus`, `edit.fillet`). Layer and object names in error messages are in quotes, so most of them go. A name in an error message without quotes can stay.

Known limits: a folder name with a space that is not followed by another path part can leak its last word, and free prose can hold a name that no rule knows. The review (below) is the safeguard for attachments, and the writer is responsible for the text.

## Attachments and consent

By default a report attaches as much useful context as it can:

| Kind of report | Attached by default |
|---|---|
| bug | model excerpt (the objects of the failing call, else the selection, else the objects nearest to the middle of the drawing), a picture of the view (`render.png`), the recent calls, the recent log lines |
| feature request | model excerpt of the selection (if any), the picture, the recent calls |
| question | nothing |

`attach` overrides this: `{excerpt?, screenshot?, calls?, log?: false}` removes one part, `attach: false` removes all, `{objects: [id]}` or `{selection: true}` names the objects, `{view: {render.png params}}` sets the picture. The picture is rendered once. If it is over 1 MiB it is left out.

- The model excerpt is a small, valid `.kad` document: the objects with their parent groups, the layers and classes they use, and the resources they need. It leaves out unknown shapes, picture underlays, and every object or resource that holds a picture. A picture is found by its first bytes (`iVBOR`, `/9j/`, `R0lGOD`, `UklGR`, `data:image`) or by a long text under a key such as `png`, `image`, `thumbnail` or `base64`. Georeference is kept until the privacy options remove it.
- The call log holds the method and scrubbed, cut parameters of the last calls of the session. It is a ring of 60 calls. The log lines are the failed calls and the lines that the host adds. A new document (File > New, Open, `document.load`, `file.open`) clears the calls and the last failure. The outbox, the consent, the rate limit and the flags of the run stay.
- The app form adds a screenshot of the window (taken before the form opens, shown in the form, sent only if ticked).

**Consent.**

1. A text-only report goes to the server only if sending is on: the person said yes once (the app asks at the first report. The choice is in `settings.json` of the outbox folder), or the run has `--feedback-send`.
   - A stored "no" beats everything. `--feedback-send` never overrides it, and nothing is returned for sending while it stands, not even an approved report.
   - `--feedback-send` covers the reports that this run made or merged into. Older reports in the outbox stay.
2. A report with attachments gets the status `needs_review`. It stays in the outbox until a person approves it. The person sees the exact message, unticks what they do not want to share, sets the privacy options and presses Send:
   - in the app: the badge on the Feedback button, then Help > Feedback > Review;
   - in a terminal: `kadakas feedback review`. It refuses when stdin or stdout is not a terminal.
   - A review cannot approve while sending is off: the person turns it on first.
3. The only code that moves a report out of `needs_review` is `Hub::approve`. The review dialog and `kadakas feedback review` call it. The API, the MCP tools, the Python package and the flags do not. `approve` stores the exact payload that the person saw, and the sender sends those bytes. An approved report whose stored message is missing goes back to `needs_review` with a visible error.
4. A form report with a screenshot is approved by the Send button of the form, because the person has seen the text and the picture. It waits for the answer of the consent dialog first. If the person said no, it stays in the outbox as `needs_review`.

Errors of `file.open` and `file.save` have the same hint and the same `tainted` mark as the errors of every other method, and the calls are in the call log.

**What this protects, and what it does not.** The guarantee holds for an agent that is limited to the MCP tools and to JSON-RPC. Nothing in them approves or sends an attachment, and a restricted session (`--allow-dir`, the app's agent server) cannot write into the config, data or feedback folders of the user, even if an allowed folder contains them, so it cannot forge the outbox. An agent that has a shell, or that is allowed to run scripts (`--allow-agent-scripts`) or converters (`--allow-converters`), runs as the same user as the person. It can read and write the same files, and it can type into a terminal that it started itself. A terminal check cannot stop such an agent, and Kadakas does not claim to. Do not give a shell to an agent that you do not trust with your drawings.

**Approving is consent for that report.** When a person approves a report (the review, or the Send button of the form with a screenshot), that is their consent for that report, also when the global question ("send my reports?") was never answered. The one exception is a stored "no": then nothing is sent and the approval is refused. Approval does not turn the global switch on.

**Privacy options of a model excerpt.** They apply to the message that is sent, and the review shows the result:

- *Move to the origin* (default on): the objects move so that the middle of their bounds is at (0, 0), including the data of hardscape objects. The frames of viewports and title blocks are on paper and stay, and the model point that a viewport shows moves with the objects.
- *Remove the georeference* (default on).
- *Remove names, notes and record data* (default off): layer, class and resource names become `Layer 1`, `Class 2`, `Resource 3`. The texts of resources go (worksheet cells, symbol texts, defaults of record fields), except object ids and structure words such as `type` and `kind`. Other texts of layers and classes go. The fields `name`, `note`, `notes`, `text`, `label`, `description`, `comment`, `title`, `tag` and `remarks` become empty in objects, the drawing data and the call log. The maps `fields`, `values`, `record`, `records`, `properties`, `props` and `data` (title block fields, record values) are emptied. The record data of objects is deleted. If a blank text breaks a rule of the document, the resources are left out.

With "move to origin" or "remove the georeference" on, the entries of record data that hold a place (`lat`, `lon`, `x`, `y`, `address`, `street`, `city`, `postcode` and similar keys, also inside other keys) are removed from the objects of the excerpt, also when the names stay. The first two options also apply to the call log and the log lines. The keys `bbox`, `extent` and `bounds` and four-number values (boxes) are removed from the calls. A pair or triple of numbers (`[385000.5, 6672000.25]`, `(1, 2)`) becomes `<coordinates removed>`, numbers under keys such as `x`, `y`, `lat` and `lon` are hidden, a value under a key such as `crs`, `epsg` or `georef` or a text with `EPSG:` or `PROJCS` is hidden, and a call about the georeference (a method with `georef` or `crs` in its name) and a log line that names one are dropped. The review says so.

## The outbox

A folder of files, owner-only on Unix:

- `outbox.json`: `{"version": 1, "next_seq": n, "reports": [...]}`. At most 2 MiB. A broken file is copied to `outbox.bad.json` and the outbox starts again. A file with a newer `version` is never read, copied over or written: the reports of this run go to memory only, `feedback.list` has a `note`, and Help > Feedback says so. A report has, among others, `attempts` (counted failed starts), `retries` (all failed starts), `next_try` and `claimed_until` (seconds since 1970).
- `settings.json`: `{"send": true|false|null, "url": null|"https://...", "installation": null|"<uuid>"}`. `installation` is a random version 4 UUID that is made at the first need. It identifies this installation of Kadakas, not a person, and it is not secret. It goes in every message (see the protocol), so that the server can drop a report that arrives twice. The consent text says so.
- `fb-N.excerpt.kad.json`, `fb-N.render.png`, `fb-N.screenshot.png`, `fb-N.calls.json`, `fb-N.log.txt`: the attachments.
- `fb-N.payload.json`: the approved message.
- `lock`: an empty file that stays where it is. A program that changes the outbox holds an exclusive operating system lock on it (`File::lock`), and the system releases the lock when the program ends, also when it dies. No program removes the file or guesses that a lock is stale.

The folder is `KADAKAS_FEEDBACK_DIR` if that is set, else `feedback` in the data folder of the user (`kadakas-sys` `dirs::data_dir`). Tests always set the variable or use memory. A `Session` keeps its outbox in memory until the host calls `session.feedback.hub.set_storage(...)`. The CLI and the app do that.

**Two programs, one folder.** The app and a headless run can share the folder. Every change takes the exclusive lock on the file `lock` (it waits at most 5 seconds), then reads the outbox, changes it and writes it back. A program that wants to send a report claims it first (`claimed_until`, 5 minutes) under the same lock, so a report is not sent twice. A crashed program holds a claim for 5 minutes at most.

Statuses: `pending` (text only, waits for sending), `needs_review`, `approved`, `sent`, `withdrawn`.

**Web build.** The browser keeps the outbox in `localStorage` (keys `kadakas.feedback/...`, base64, at most 2 MB), or in memory if the browser refuses. The browser build does not send yet: Help > Feedback has no sending switch and no address, it says that reports are saved but not sent, and the consent dialog does not appear. A fetch sender is future work.

## Sending

Sending runs in a helper thread (`kadakas_feedback::dispatch::Dispatcher`). The host calls `pump` after each request (CLI) or about every two seconds (app). The sender is a trait (`Sender`); `HttpSender` is the real one, `FakeSender` the one for tests. Tests never use the network. `HttpSender` accepts `https://` addresses only. Loopback `http://127.0.0.1` and `http://localhost` work in its own tests, and in the app and the CLI only when the variable `KADAKAS_FEEDBACK_ALLOW_LOOPBACK_HTTP` is `1` (for a local test server). It follows no redirects, and has a 10 s connect and a 30 s total timeout.

- **Retry within a start:** a transient error (timeout, dropped connection, 408, 425, 5xx) is tried again at once, 4 tries with waits of 0.5 s, 1 s and 2 s.
- **Back-off between starts:** a report that failed waits. The wait is 1 minute after the first failed start, then doubles, at most 6 hours, with a jitter of plus or minus 20 percent. The time of the next try is stored in the report (`next_try`).
- **Offline:** if the name does not resolve or the connection is refused, the start stops at once. It does not count toward the lifetime limit of 8 failed starts (`attempts`). It still sets the back-off.
- **Busy server:** 429 and 503 are not tried again at once. A `Retry-After` in seconds sets the next time (the longer of that and the back-off). A date in `Retry-After` is not read.
- **Refusal:** another 4xx or a 3xx stops the report until a person says "Try again" in the window or runs `kadakas feedback retry [id]`.
- **A new address** (Help > Feedback, `settings.json`) gives every unsent report a fresh start.
- **The end of a program** waits at most 2 seconds for sends that run. A report that is not done stays in the outbox and its claim ends after a while.

Flags of `kadakas serve` and `kadakas mcp`:

- `--feedback-send`: text-only reports of this run go to the server (unless the person said no). Approved reports go too.
- `--feedback-url URL` (`KADAKAS_FEEDBACK_URL`): the server for this run. In the app, Help > Feedback has the address.

Commands of `kadakas feedback`: `review [id]` (terminal only. The pictures are written to private temporary files, 0600, and their paths are printed before the question. The default answer for a picture is no), `list`, and `retry [id]`.

The default address is a placeholder: `https://feedback.kadakas.invalid`. The `.invalid` name never resolves, so nothing leaves by accident until the server exists.

## When a method panics

`kadakas serve`, `kadakas mcp` and the app's agent server do not end when a method panics. The JSON-RPC layer (`Session::guarded`, also around `file.open` and `file.save`) catches the panic and answers with an internal error (code -32603, kind `internal`) and the hint to file a bug.

- The command that panicked takes its own changes back before the panic goes on (`Editor::execute`). The document, the revision and the undo history are what they were.
- The session is marked tainted: every later error has `data.tainted: true`, because other state can be half way through a change. A new document clears the mark. The safe answer to a tainted session is to save and restart.
- A panic hook (installed once, chained to the previous hook, so the message still goes to stderr) keeps the message and the place. They go into the log lines and the failure of the session, so the bug report has them.
- A program built with `panic = "abort"`, such as the web build, does not unwind. A panic there ends the program, and none of this applies.

## HTTP protocol, version 1

```
POST <url>/v1/feedback
Content-Type: application/json
User-Agent: Kadakas/<version>
```

If the address has a query or a fragment, the path goes before it. The body is one JSON object, at most 6 MiB (the server can answer 413 above its own limit):

```json
{
  "protocol": 1,
  "client_id": "fb-12",
  "installation": "0190a1b2-c3d4-4e5f-8a6b-0123456789ab",
  "kind": "bug",
  "title": "Fillet fails on a closed polyline",
  "body": "...",
  "wanted": "...", "tried": "...", "method": "edit.fillet", "error": "...",
  "severity": "medium",
  "origin": "agent",
  "count": 2,
  "context": {"version": "0.1.0", "os": "macos", "client_name": "claude-code", "client_version": "2.0"},
  "attachments": {
    "excerpt":    {"format": "kad", "document": { "...a valid .kad document..." }},
    "screenshot": {"mime": "image/png", "base64": "..."},
    "render":     {"mime": "image/png", "base64": "..."},
    "calls":      [{"method": "edit.fillet", "params": {}, "error": "..."}],
    "log":        "line 1\nline 2"
  }
}
```

- `wanted`, `tried`, `method`, `error`, `severity` and each attachment are missing when empty. `attachments` is `{}` for a text-only report. A person can have removed any attachment in the review.
- `client_id` is local to one installation and one outbox. `installation` is the random UUID of the installation (it identifies the installation, not the person, and the consent text says so). The server keys a report on the pair (`installation`, `client_id`) and drops a repeat of it.
- All texts are UTF-8, scrubbed, and within the limits above. The server still has to treat everything as untrusted input.
- Images are PNG, at most 1 MiB each before base64. The excerpt is at most 512 KiB.

Answers:

| Status | Meaning | Body | Client action |
|---|---|---|---|
| 200, 201, 202 | Stored | `{"id": "srv-id", "status": "received"}` | The report is `sent`; `id` and `status` are shown. `status` can also be `duplicate` (the server merged it). |
| 429, 503 | Busy | any; header `Retry-After: <seconds>` (optional) | Wait for `Retry-After` or the back-off, whichever is longer |
| 408, 425, other 5xx | Try later | any | Transient: retry with the back-off |
| other 4xx (400, 401, 403, 404, 413, 422) | The server refuses the report | any | Permanent: no more tries until a person presses "Try again" |

A 3xx answer counts as a refusal (the client follows no redirects). The server needs to rate-limit by address and answer 429 with `Retry-After`. The server stores the report and can later copy it to an issue tracker. A `DELETE` to withdraw a sent report is not part of version 1: `feedback.withdraw` works on unsent reports only.

A new version of the protocol uses a new path (`/v2/feedback`). Version 1 stays.

## Where things are not done

- The server side (`kadakas-server`): endpoint, rate limits, storage, issues.
- Withdrawal of a report that was sent.
- Sending from the browser build (a fetch sender), and the screenshot of the window there.
- A window screenshot for reports of agents: they get a `render.png` of the current view instead, also in the app.
- "Near the current view" for the excerpt of a bug without a failing call or a selection: a session has no 2D view, so the objects nearest to the middle of the drawing are taken.
- A shell or script agent: see "What this protects, and what it does not".
