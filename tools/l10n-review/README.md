# Translation review tool

A small local web app. It shows the English text and the target text side by side. You mark weird words, write comments and ask Claude for better texts. It runs only on your machine.

It needs Python 3 for the page. No packages, no network, no CDN. The Suggest button also needs the Claude Code CLI (`claude`) on your PATH and a logged-in account.

## Start

```
python3 tools/l10n-review/server.py
```

Then open http://127.0.0.1:8765/ in a browser.

Options:

- `--port 8765` sets the port.
- `--target et` sets the language to review. The page has a language selector too.
- `--repo <path>` sets the repository. The default is the repository of the script.
- `--state <file>` sets the state file.
- `--timeout 120` sets the seconds for one Claude call.
- `--claude-bin claude` sets the name or path of the Claude Code CLI.

The server binds to 127.0.0.1 only. It answers only the Host names `127.0.0.1:<port>` and `localhost:<port>`. A request with another Origin gets 403. Every API call needs a random token that the page carries. Every POST needs `Content-Type: application/json`. The server never answers CORS preflights.

## State file

Your flags, comments, highlights, suggestions and glossary go to one JSON file. The file is outside the repository.

The default is `~/.kadakas-l10n-review/<target>.json`. The file is written atomically. If the file is not valid JSON, has the wrong shape or belongs to another language, the tool moves it to `<name>.broken-<time>` and raises an error at start-up. A second locale with a broken file gives HTTP 400. The tool never drops your data silently.

The repository files change only when you press Apply.

## Review

Each row shows the file and key, the English text, the target text and the status.

- Click a row to see the context. It has the group comment of the file, the comment above the message, the variables with example values, the neighbouring keys and up to five places in the Rust sources that use the key.
- Select words in the target text and press Weird (or key `w`). The words get a yellow highlight. Click a highlight to remove it.
- Write a comment in the box. Press `f` to flag or unflag the message.
- Statuses are unreviewed, flagged, suggested, accepted and fixed.
- Keys: `j` and `k` move, `f` flags, `c` goes to the comment, `a` asks for suggestions, `w` marks the selected words.
- Filters: file, status, text search in both languages, and only messages that have a different suggestion.

If a highlighted text changes in the file, the highlights of that message are hidden. Their offsets no longer fit.

## Suggest

Suggest runs `claude -p` on this machine. It uses your Claude usage. Pick the model (sonnet, opus or haiku) next to the button.

1. The server builds a prompt. It has the English source, the current text, the weird words, your comment, up to six neighbours, the variables and the glossary.
2. The server starts the CLI with an argument list. It sends the prompt on stdin. The working folder is a temporary folder, not the repository. The CLI runs without tools, MCP servers, settings, slash commands and session files, and with a system prompt that says the texts are data. The prompt wraps the texts in `<data>` tags for the same reason.
3. The prompt asks for strict JSON with three alternatives. The server finds the first JSON object, even inside code fences.
4. The server checks every alternative. The variables are the same as the English ones. The selector is intact. Terms, string literals and functions in braces are the same as in the current text. The text has no leading or trailing space. The line count matches. A bad alternative stays in the list with the reason. You accept it only after you edit it.

Each alternative is a card. Changed words are marked against the current text. Buttons:

- Use this accepts the alternative and sets the status to accepted.
- Edit changes the text in place and then accepts it.
- Reject marks it rejected.
- More options asks again. The rejected texts go into the prompt as texts to avoid.

Type your own suggestion under the cards.

Suggest for all flagged runs the flagged messages one after another. It starts one process at a time. A progress bar shows the state. Cancel stops the running process and all its child processes.

If the CLI is missing, not logged in or fails, the card area shows the exit code and the last lines of stderr. Copy prompt stays available. It copies the same prompt. Paste it into any chat.

The timeout is 120 seconds for each call. On a timeout, a cancel or a server stop, the tool ends the whole process group of the CLI. It sends SIGTERM first and SIGKILL after two seconds.

## Glossary and terms

The Glossary tab holds fixed pairs of an English term and a target term, with a note. The pairs go into every prompt. The first start seeds a few pairs. Edit them as needed.

The Terms tab shows two heuristic checks.

- The glossary check counts messages that have the English term but not the fixed target term.
- The second list shows English words with several target renderings. It compares word stems of five letters. It finds many false hits. Use it as a hint only.

## Apply

The button Apply accepted to files shows a diff of the exact lines that change. After you confirm, it rewrites the value text of the accepted messages in `crates/kadakas-l10n/locales/<target>/*.ftl`.

Safety rules:

- Keys, comments, attributes and indentation stay as they are.
- The new text has exactly the variables of the English message. A dropped or new `$variable` is refused. Variables in attributes count apart from the value.
- The selector (the expression, the variant keys, the default mark, the braces, the lines and their indentation) equals the current text. Otherwise it is refused.
- Term references, message references, string literals and function calls in braces equal those of the current text. An empty `{}` is refused. A line that starts with `[`, `*` or `.` outside a variant is refused.
- The line count of a plain message stays the same.
- After the rewrite the tool parses the file again. The keys are the same. Every other message is unchanged. If a check fails, the file is not written. If the package `fluent.syntax` is installed, the official parser checks the result too. It is optional and the tool does not need it.
- Every suggestion stores a hash of the target and English text from the time you asked. If the text changed since then, the suggestion is refused.
- The preview lists the exact changes with hashes. Apply sends them back. If anything differs from the preview, nothing is written.
- The file keeps its mode.
- If Git is missing or fails, the tool treats the files as possibly changed and asks for the tick box.
- Files are written atomically. There are no backup files. Git is your backup.
- If the target files have uncommitted changes, Apply refuses until you tick the box in the dialog.
- A message becomes fixed after it is written.

## Export

Export review as Markdown lists every message that is not unreviewed. It has the English text, the current text, the weird words, the comment and the suggestions. It also lists the glossary.

## Tests

```
python3 -m unittest discover -s tools/l10n-review
```

The tests cover the parser (also on every real file), key parity, the usage index, the state file, the prompt, the answer parsing with a fake `claude` script, Apply on temporary copies and a server smoke test. The JavaScript offset logic has its own test. It runs with `node` and is skipped when node is missing. The tests never call the real Claude.

## Limits

- The parser reads the shapes this repository uses. It is not a full Fluent parser.
- The usage search looks for exact key strings in `crates/**/*.rs`. Names built at run time are matched by prefix and are marked as a guess.
- The tool does not add missing keys.
- It reads files with LF line ends. A file with CRLF is not rewritten.
