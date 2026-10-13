# Feedback: bug reports and feature requests (API-47, API-48, UI-14)

Agents and people can tell the Kadakas team what is missing or broken. A report goes to a local outbox first. It leaves the computer only with the consent of a person. This page describes the reports, the outbox, the consent rules and the HTTP protocol that the feedback server (the separate `kadakas-server` repository) will implement.

The code is in these places:

| Part | Where |
|---|---|
| Model, scrubbing, dedupe, outbox, payload, sender with retry (pure, wasm-compatible) | `crates/kadakas-feedback` |
| Folder store, HTTP sender, sending thread (native only) | `crates/kadakas-feedback` (`store`, `http`, `dispatch`) |
| `feedback.*` methods, error hints, model excerpt, review model | `crates/kadakas-api/src/feedback_api.rs` and `feedback_api/` |
| MCP tool `feedback`, instructions, `--feedback-send`, `kadakas feedback review` | `crates/kadakas-cli` |
| Form logic and consent flow (no egui) | `crates/kadakas-ui-core/src/feedback.rs` |
| Status bar button, form, Help > Feedback, review, consent dialog | `crates/kadakas-app/src/feedback_ui.rs` |
| Python | `doc.feedback` in `python/kadakas/document.py` |

## For agents

- The MCP `initialize` instructions say it: if a task needs a feature that Kadakas lacks, file a feature request with the `feedback` tool. If a method fails in a way it must not, file a bug report.
- The `feedback` tool is in both MCP modes (domain tools and `--mcp-flat`), second in the list after `describe`. Its `action` is `submit` (default), `list`, `get` or `withdraw`.
- `describe` has a `feedback` section with the rules and examples.
- An error of an unknown method, an unsupported option (`unknown field`, `unknown variant`, `unsupported`, `not supported`) or an internal error (code -32603, kind `internal`) has `data.feedback`:

```json
{"hint": "If a task needs this method, file a feature request with the feedback tool. Submit the draft or edit it.",
 "draft": {"kind": "feature_request", "title": "Method plants.teleport is missing",
           "method": "plants.teleport", "error": "unknown method: plants.teleport"}}
```

  The draft is a valid `feedback.submit` call. Over MCP, the text of the failed tool call ends with the hint and the draft, because a client shows the text only. Other errors have no hint, so the token budget stays small.
- A panic inside a method does not end `kadakas serve`. The JSON-RPC layer turns it into an internal error with this hint. The document may be in a state that the method did not expect.

## Methods

`feedback.submit {kind, title, body?, wanted?, tried?, method?, error?, severity?, client?, attach?}`

- `kind`: `bug`, `feature_request` or `question`. `severity`: `low`, `medium`, `high`.
- A bug report without `method` and `error` takes them from the last failed call of the session.
- Kadakas adds: version, operating system, the MCP client name and version (from `initialize`; the CLI adds them to the call), and the origin (`agent`, `user`, `crash`).
- The result is `{id, merged, count, status, attachments, note}`. The id is local (`fb-12`).
- `feedback.list {status?, limit?}` gives `{reports, needs_review, sending: {enabled, asked}}`.
- `feedback.get {id}` gives the report, the kinds and sizes of its attachments, and the status. It never gives the attachment content.
- `feedback.withdraw {id}` takes back a report that was not sent.

No method approves or sends an attachment. Extra fields such as `approve` or `status` are ignored.

## Limits

| What | Limit |
|---|---|
| Title / text / wanted / tried / error / method | 120 / 4000 / 2000 / 2000 / 2000 / 80 characters |
| Reports in the outbox | 200 (the oldest sent or withdrawn report makes room; else the call fails) |
| Reports one session may make (merged ones count) | 30 |
| Attachments that wait for review | 64 MiB in all |
| Picture (PNG) | 1 MiB |
| Model excerpt | 512 KiB and 200 objects |
| Call log | 40 calls, 32 KiB |
| Log lines | 50 lines, 16 KiB |
| Message to the server | 6 MiB |
| Tries per report over all runs | 8, then a person must press "Try again" |

A report with the same key (kind, normalized title, method) is merged: the counter of the first report goes up. A merged report drops its new attachments. A withdrawn report does not absorb a new one.

## Scrubbing

Every text of a report, and the call log, goes through `kadakas_feedback::scrub` when the report is made, whatever the caller did.

- Absolute paths (`/home/ann/x.kad`, `~/x`, `C:\Users\ann\x.kad`, `\\server\share\x`) become `<path>/x.kad`. The file name stays. A path without a file name becomes `<path>`.
- Bearer tokens, values of keys such as `token`, `secret`, `password`, `api_key`, `authorization` and `cookie`, the password of an address, known token shapes (`sk-`, `ghp_`, JWT, `AKIA`) and long words that mix letters and digits become `<secret>`.
- E-mail addresses become `<email>`.
- Object ids (UUIDs) stay: they help to find the problem.
- Control characters go. A text is cut to its limit.

Known limits: a folder name with a space that is not followed by another path part can leak its last word, and free prose can hold a name that no rule knows. The review (below) is the safeguard for attachments, and the writer is responsible for the text.

## Attachments and consent

By default a report attaches as much useful context as it can:

| Kind of report | Attached by default |
|---|---|
| bug | model excerpt (the objects of the failing call, else the selection, else the objects nearest to the middle of the drawing), a picture of the view (`render.png`), the recent calls, the recent log lines |
| feature request | model excerpt of the selection (if any), the picture, the recent calls |
| question | nothing |

`attach` overrides this: `{excerpt?, screenshot?, calls?, log?: false}` removes one part, `attach: false` removes all, `{objects: [id]}` or `{selection: true}` names the objects, `{view: {render.png params}}` sets the picture.

- The model excerpt is a small, valid `.kad` document: the objects with their parent groups, the layers and classes they use, and the resources they need. It leaves out unknown shapes, picture underlays and resources that hold pictures. Georeference is kept until the privacy options remove it.
- The call log holds the method and scrubbed, cut parameters of the last calls of the session. It is a ring of 60 calls. The log lines are the failed calls and the lines that the host adds.
- The app form adds a screenshot of the window (taken before the form opens, shown in the form, sent only if ticked).

**Consent.**

1. A text-only report goes to the server only if sending is on: the person said yes once (the app asks at the first report; the choice is in `settings.json` of the outbox folder), or the run has `--feedback-send`.
2. A report with attachments gets the status `needs_review`. It stays in the outbox until a person approves it. The person sees the exact message, unticks what they do not want to share, sets the privacy options and presses Send:
   - in the app: the badge on the Feedback button, then Help > Feedback > Review;
   - in a terminal: `kadakas feedback review`. It refuses when stdin or stdout is not a terminal.
3. The only code that moves a report out of `needs_review` is `Hub::approve`. The review dialog and `kadakas feedback review` call it. The API, the MCP tools, the Python package and the flags do not. `approve` stores the exact payload that the person saw, and the sender sends those bytes.
4. A form report with a screenshot is approved by the Send button of the form, because the person has seen the text and the picture. It waits for the answer of the consent dialog first.

**Privacy options of a model excerpt.** They apply to the message that is sent, and the review shows the result:

- *Move to the origin* (default on): the objects move so that the middle of their bounds is at (0, 0), including the data of hardscape objects.
- *Remove the georeference* (default on).
- *Remove names, notes and record data*: layer, class and resource names become `Layer 1`, `Class 2`, `Resource 3`; the fields `name`, `note`, `notes`, `text`, `label`, `description`, `comment`, `title`, `tag` and `remarks` become empty in objects, the drawing data and the call log; the record data of objects is deleted.

## The outbox

A folder of files, owner-only on Unix:

- `outbox.json`: `{"version": 1, "next_seq": n, "reports": [...]}`. At most 2 MiB. A broken or newer file is copied to `outbox.bad.json` and the outbox starts again.
- `settings.json`: `{"send": true|false|null, "url": null|"https://..."}`.
- `fb-N.excerpt.kad.json`, `fb-N.render.png`, `fb-N.screenshot.png`, `fb-N.calls.json`, `fb-N.log.txt`: the attachments.
- `fb-N.payload.json`: the approved message.

The folder is `KADAKAS_FEEDBACK_DIR` if that is set, else `feedback` in the data folder of the user (`kadakas-sys` `dirs::data_dir`). Tests always set the variable or use memory. A `Session` keeps its outbox in memory until the host calls `session.feedback.hub.set_storage(...)`; the CLI and the app do that. The web build keeps the outbox in `localStorage` (keys `kadakas.feedback/...`, base64, at most 2 MB), or in memory if the browser refuses.

Every call reads and writes the files, so the app and a headless run that share the folder see each other's reports.

Statuses: `pending` (text only, waits for sending), `needs_review`, `approved`, `sent`, `withdrawn`.

## Sending

Sending runs in a helper thread (`kadakas_feedback::dispatch::Dispatcher`). The host calls `pump` after each request (CLI) or about every two seconds (app). The sender is a trait (`Sender`); `HttpSender` is the real one, `FakeSender` the one for tests. Tests never use the network. `HttpSender` accepts `https://` addresses only (and `http://127.0.0.1` in its own tests), follows no redirects, and has a 10 s connect and a 30 s total timeout.

Retry: 4 tries per start, with waits of 0.5 s, 1 s and 2 s (doubling, at most 8 s). A transient error counts the tries. After 8 tries over all runs, or after a refusal, the report waits for a person ("Try again" in the window).

Flags of `kadakas serve` and `kadakas mcp`:

- `--feedback-send`: text-only reports go to the server in this run. Approved reports go too. Reports with attachments still need `kadakas feedback review`.
- `--feedback-url URL` (`KADAKAS_FEEDBACK_URL`): the server for this run. In the app, Help > Feedback has the address.

The default address is a placeholder: `https://feedback.kadakas.invalid`. The `.invalid` name never resolves, so nothing leaves by accident until the server exists.

## HTTP protocol, version 1

```
POST <url>/v1/feedback
Content-Type: application/json
User-Agent: Kadakas/<version>
```

The body is one JSON object, at most 6 MiB (the server may answer 413 above its own limit):

```json
{
  "protocol": 1,
  "client_id": "fb-12",
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

- `wanted`, `tried`, `method`, `error`, `severity` and each attachment are missing when empty. `attachments` is `{}` for a text-only report. A person may have removed any attachment in the review.
- `client_id` is local to one installation and one outbox. The server uses it together with the sender (address and token, once the server has accounts) to make a repeat of the same request harmless.
- All texts are UTF-8, scrubbed, and within the limits above. The server must still treat everything as untrusted input.
- Images are PNG, at most 1 MiB each before base64. The excerpt is at most 512 KiB.

Answers:

| Status | Meaning | Body | Client action |
|---|---|---|---|
| 200, 201, 202 | Stored | `{"id": "srv-id", "status": "received"}` | The report is `sent`; `id` and `status` are shown. `status` may also be `duplicate` (the server merged it). |
| 408, 425, 429, 5xx | Try later | any | Transient: retry with the back-off |
| other 4xx (400, 401, 403, 404, 413, 422) | The server refuses the report | any | Permanent: no more tries until a person presses "Try again" |

A 3xx answer counts as a refusal (the client follows no redirects). The server should rate-limit by address and answer 429 with `Retry-After`; the client treats 429 as transient. The server stores the report and may later copy it to an issue tracker. A `DELETE` to withdraw a sent report is not part of version 1: `feedback.withdraw` works on unsent reports only.

A new version of the protocol uses a new path (`/v2/feedback`); version 1 stays.

## Where things are not done

- The server side (`kadakas-server`): endpoint, rate limits, storage, issues.
- Withdrawal of a report that was sent.
- The screenshot of the window in the web build (the form has no screenshot there).
- A window screenshot for reports of agents: they get a `render.png` of the current view instead, also in the app.
- "Near the current view" for the excerpt of a bug without a failing call or a selection: a session has no 2D view, so the objects nearest to the middle of the drawing are taken.
