# The welcome dialog (PLAT-26)

The launcher shows a welcome dialog once, on the very first start. It opens before the launcher starts any version of Kadakas. It always waits for a button. This holds when the first start was a double click on a `.kad` file. The file opens after Start or Use defaults.

## What it asks

The first screen has these choices.

- **How do you want to work?** 3D Modeler (simple push and pull shape modeling, the default) or Landscape CAD (plans, sheets and plants). The choice decides which program the launcher starts. If the installed version has no modeler, only Landscape CAD is open for choice.
- **Language.** The languages of the program, in their own names (English, Eesti). The default is the language of the system. If we have no text for it, the default is English. The dialog changes its language and its window title at once.
- **Units.** Metric (mm, m) or Imperial (ft, in), with a line that shows an example.
- **Number format** (next to Units, with the same example line). `1,234.56`, `1.234,56`, `1 234,56`, `1’234.56` (Swiss), `1234.56` or `1234,56`.

The fold **More options** is closed at first.

- **Theme**: System default, Light, Dark.
- **Mouse or trackpad**: how the wheel and the two-finger scroll move the 3D views. *Mouse*: the wheel zooms to the pointer. *Trackpad*: a two-finger scroll moves the view. With Shift, a vertical scroll tilts the view and a horizontal scroll turns it. A pinch zooms.
- **Crash reports**: Ask each time (default), Always send, Never send (PLAT-20).
- **Updates**: Auto-update (default), Auto-download, Notify only (PLAT-16).

The text names the places for later changes: Preferences, and the View menu in the 3D modeler. It says that Help > Show welcome opens the window again. A line under it tells what the keys do. On a first start: Enter starts and Esc uses the defaults. In the other two cases: Enter starts or saves and Esc cancels.

## Buttons

- **Start** (Enter) stores the choices.
- **Use defaults** (Esc) is the second button on a first start. It stores the defaults for the keys that are missing and replaces no value. The close button of the window does the same.
- In the other two cases the second button is **Cancel**. After a newer dialog, Cancel only marks the dialog as seen. After Show welcome, Cancel changes nothing.
- If a file is not writable, the dialog stays open and shows the problem. The next press of a button closes it. If the mark was not written, the process exits with an error.

## Where the defaults come from

The defaults come from what the system says about the **regional formats**. The language of the interface does not decide them. An Estonian with an English Windows gets metric units and `1 234,56`. A GNOME with English text and "Formats: Estonia" gets the same.

| System | Source |
|---|---|
| Windows | `GetUserDefaultLocaleName`, and `GetLocaleInfoEx` with `LOCALE_IMEASURE` (metric or US), `LOCALE_SDECIMAL`, `LOCALE_STHOUSAND`. The overrides of the user in the Control Panel count. |
| macOS | `NSLocale.currentLocale` (System Settings > General > Language & Region): the identifier, `usesMetricSystem`, the decimal and grouping separators. |
| Linux and others | `LC_ALL`, then `LC_MEASUREMENT` (units) or `LC_NUMERIC` (number format), then `LANG`. The first one that is set and is not `C` or `POSIX` names the locale. |

The answer of the system wins. If the system gives no answer, the region of the formats locale decides the units. Then the region of the first interface language decides. No region (a bare `en`) is metric. **Imperial is for the United States, Liberia and Myanmar. The United Kingdom is metric.** This is a decision of the project.

The number style comes from the separators that the system reports, when the pair is one of the six styles above. Otherwise one table in `kadakas-units` (`number_convention`) decides. The program uses the same table for the language of the interface.

| Style | Locales |
|---|---|
| `1,234.56` | English (all regions), Japanese, Chinese, Korean, Hebrew, Thai, Hindi, Spanish in Mexico, the United States and most of Latin America (`es-419`), and every language that the table does not name |
| `1.234,56` | German, Dutch, Spanish, Italian, Portuguese (Brazil), Indonesian, Turkish, Danish, Greek, Romanian, Croatian, Slovenian |
| `1 234,56` | Estonian, Finnish, French, Swedish, Norwegian, Polish, Czech, Slovak, Russian, Ukrainian, Bulgarian, Hungarian, Lithuanian, Latvian, German in Austria, Portuguese in Portugal, French in Switzerland, English in South Africa |
| `1’234.56` | German and Italian in Switzerland |

The Indian lakh grouping (`12,34,567.89`) is not supported. `en-IN` gives `1,234,567.89`.

## What the dialog shows and what it stores

- It shows what the programs really use. A missing key has the default of the program that reads it: millimeters (not the region), and the number format of the language. For the 3D modeler, the language, the theme and the navigation of `modeler.json` come first. Its own values win for the modeler.
- When the number format is not stored, the programs take it from the language. Then the dialog follows the language too. If the person picks Estonian, the dialog shows `1 234,56` at once. This counts as no change, so no key is written. A pick of a number style by the person ends this.
- On a first start there is nothing to compare with. **Start** stores every choice.
- Otherwise only the choices that the person **changed** are stored. A choice that stays as it was is not written. The modeler's own `navigation` is removed only when the person changes the navigation. `units.decimal` and `units.grouping` are written only when the person changes the number format.

| Choice | File | Key |
|---|---|---|
| How to work | `launcher.json` (install folder of the launcher) | `flavor` (`kadakas-modeler` or `kadakas-app`) |
| Updates | `launcher.json` | `mode` |
| Dialog seen | `welcome.json` (install folder) | `done` (the version of the dialog, now 1) |
| Language | `prefs.json` and `modeler.json` (prefs folder) | `language` (`system` or a tag, `null` in `modeler.json`) |
| Units | `prefs.json` | `units.length`, `units.area` |
| Number format | `prefs.json` | `units.decimal`, `units.grouping` |
| Theme | `prefs.json` and `modeler.json` | `theme` |
| Mouse or trackpad | `prefs.json` (the modeler reads it too) | `navigation` (`mouse` or `trackpad`) |
| Crash reports | `settings.json` in the feedback folder (`KADAKAS_FEEDBACK_DIR`) | `crash_policy` |

The prefs folder is the same for the app, the modeler and the dialog (`kadakas_sys::dirs::prefs_dir`). It is `KADAKAS_PREFS_DIR`, else the folder of `KADAKAS_KEYMAP` (a relative name means the current folder), else the config folder.

### How files are written

- Every writer changes **only the keys it changed**. It does this in the file as it is now, under a lock (`kadakas_sys::jsonfile`, a `.lock` file next to the file). The app, the modeler and the window `kadakas-launcher manage` keep their settings in memory. A later save of an old copy used to undo what the dialog had stored. Now it does not. Keys that a program does not know stay, in every file.
- The writer waits up to one second for the lock. This wait happens on the thread of the window. The holders keep the lock for a few milliseconds, so the wait is short in practice. On a file system without locks (some network shares) the write goes on without a lock, with one line on stderr.
- The running app and modeler **read their preferences again** when the dialog stores a change. The dialog moves the stamp `changed` in `welcome.json`. The programs look at it twice a second. The app reads the update settings again at the same time. The window `manage` reads `launcher.json` again when its dialog ends.
- A save that fails keeps the changes unsaved. The status line shows the failure at each try. The program tries again after 1, 2, 4 ... seconds, at most 30. A window that is idle wakes up for the try. The same holds for the update settings of the app.
- A reload (the dialog stored a choice) keeps the changes that were not saved yet. They stay on top of the file, and the next frame writes them.
- When the program ends with unsaved changes, it waits a moment and tries once more. If that fails, the reason goes to stderr and to the file `save-failed.log` in the state folder.
- A file with text that is not a JSON object is copied to `<name>.damaged-<seconds>`. This name is never replaced. The same damaged text makes one copy, not one per try. Only the newest 10 copies stay. If no copy is written, the file is not replaced.
- The last good version is `<name>.bak`. When a file is empty or damaged, the program loads the `.bak` and tells the person in the status line. A write after the damage starts from the `.bak`, so the settings come back. It also keeps the `.bak` as `<name>.good-<seconds>`, because the next writes move `.bak` on. A damaged or empty file never moves into `.bak`.
- A read error that is not "not found" (permission, disk) stops the write. The launcher prints the problem. The file is not replaced by defaults. A write keeps the permissions of the file that it replaces. A problem with one file does not stop the others.
- The mark that the dialog was seen is in its **own file**, `welcome.json`. An older version of the app writes `launcher.json` without the keys that it does not know. A mark in that file is lost when the person goes back to an older version. The old key `welcome_done` of `launcher.json` is still read.
- Values that this version does not know give the default for that one field. The other keys of the file stay readable. This holds for the units (including the Swiss grouping), the theme, the navigation and the crash policy. A write of the crash policy changes that one key and keeps the rest of the feedback settings.
- The installation ID (PLAT-20) is made when it is first needed, as before. The dialog does not make it.

## When it is shown

- **First start**: nothing is marked and there is no trace of an earlier use. A trace is the install folder of the launcher (`launcher.json`, `versions`, `current`, `runs`). A settings file of the programs is a trace too (`prefs.json`, `modeler.json`, the feedback `settings.json`). The dialog starts with the defaults of the system.
- **Update**: the mark is lower than the dialog, or there are traces and no mark. This holds after a reinstall, after a moved launcher folder, and for an install from before the dialog. The dialog starts with the **current** values. If you delete `welcome.json` (and the old key), you get this dialog again.
- **Show welcome** (Help in the app and in the modeler, and a button in `kadakas-launcher manage`): the dialog starts with the current values. The buttons are Save and Cancel. The program writes the request file `runs/<id>.welcome`. The launcher watches the run and opens the dialog in a new process. This works only when the launcher started the program. Otherwise the status line says so.

It is never shown in these cases.

- Headless use (the `kadakas` command line, MCP, agents, `serve`). Those programs do not call the launcher.
- `KADAKAS_SKIP_WELCOME=1` is set (CI and tests). `true` and `yes` work too.
- The browser build. It has no launcher and uses its own defaults.
- The launcher has no display (Linux without `DISPLAY` or `WAYLAND_DISPLAY`).

## How it runs

The dialog is a window of its own process: `kadakas-launcher welcome [--first-run|--update|--manual]`. The launcher starts it and waits, then starts the app. This is the same on every system. It matters on macOS, where the first process runs the system event loop on its main thread.

- **A file that arrives while the dialog is open**: on macOS the system sends a second double click to the one launcher. Its inbox keeps the file, and the app gets it after the dialog. On Windows and Linux a second double click starts a second launcher. There is no hand-over of the file between two launchers. The second launcher writes one line to stderr and waits for the lock `welcome.lock` of the first one (up to an hour). Then it looks again, finds the dialog done, and starts its own app with its own file.
- **The lock** is held while the dialog is open. The operating system releases it when a program ends, so a lock is never stale.
- **A dialog that fails** does not stop the start. The window does not open, the process crashes, or it ends with a non-zero status. The launcher prints the reason, writes no mark and starts the app with the settings as they are. The dialog comes again at the next start. A dialog that is open but hidden holds the start by design. `KADAKAS_WELCOME_TIMEOUT_SECS=N` kills the dialog process after N seconds. It is for tests and has no default.

## Mouse or trackpad

The style is `navigation` in `prefs.json`. The modeler keeps its own choice in `modeler.json` (View > Mouse or trackpad). That choice wins for the modeler until the person changes the navigation in the dialog.

- *Mouse*: the wheel zooms to the pointer, exactly as before the style existed. egui turns a scroll with Shift into a horizontal one, so the wheel with Shift does not zoom.
- *Trackpad*: the app reads the raw wheel events of the window system. egui does not serve here. It turns a vertical scroll with Shift into a horizontal one. It also keeps the modifier for a whole gesture. A scroll pans. With Shift, a vertical scroll tilts (pitch) and a horizontal scroll turns (yaw). The pinch zooms.
- **The Shift choice of a gesture** is made when the gesture starts. If Shift goes up in the middle, an orbit stays an orbit. If Shift goes down in the middle, the rest becomes an orbit.
- **Momentum**: winit reports the momentum of a macOS trackpad as a new `Start` after the `End`. A `Start` within 0.4 seconds after an `End` of an orbit keeps the orbit. So the momentum of an orbit stays an orbit when Shift is up. This join is used once. The end of the momentum does not arm it again. A new gesture within 0.4 seconds after an orbit is read as momentum. This is a known limit.
- **The gesture is forgotten** in two cases. The pointer leaves the canvas. Or 3 seconds pass without a wheel event while a gesture is open (the window system lost the `End`). A rest of the fingers that is shorter keeps the gesture.

## Numbers in the UI

The number format is the one preference that the whole UI uses. It has a decimal symbol (`.` or `,`) and a digit grouping (none, `,`, `.`, space, `’`).

- **Shown**: lengths, areas, angles, the Measurements box and the fields of the programs. The live labels of the tools. The numbers inside messages: `tr!` writes a number argument with the decimal symbol (`kadakas_l10n::set_decimal_symbol`). Byte counts have digit grouping.
- **Typed**: every number field (`DragValue`, `Slider`) writes and reads the number in the format (`.ui_number()`, crate `kadakas-ui-number`). It accepts both `.` and `,` as the decimal separator. The criteria and the default of a field accept `1,5`. A worksheet cell entry that is a plain number accepts it too. In a **formula** the comma separates arguments, so numbers in formulas are written with a point.
- **`1,234`** is ambiguous. With a format that groups with a comma it is 1234. With a decimal comma it is 1.234. The format decides. Every place that reads a number follows this rule. This includes the cells, the criteria and the field defaults. So `1.500` is 1500 with German formats and 1.5 with English formats.
- **The Swiss apostrophe** groups digits only in the Swiss format. In every other format `’` and `'` are the foot mark (`5’6”`).
- **Not affected**: files, the JSON-RPC API, MCP, the command line, exports, ids, logs and the formula grammar. The exports are DXF, IFC, CSV, STEP, glTF, SVG and the coordinates in PDF. All of these use the plain format (a point, no grouping).
- **The check**: `scripts/check-number-format.sh` (in CI, job `number-format`) fails for a number widget without `.ui_number()` or a unit extension. It also fails for a fixed precision in a text macro. The macros are `format!`, `write!`, `println!` and the like. The forms are `{:.2}`, `{:>6.1}`, `{0:.2}` and `{:.prec$}`. It scans the UI crates and the launcher crates. `scripts/number-format-allow.txt` lists the exceptions, with a reason each.

### Limits

- **Drawing text is not covered** (PLAT-27). These follow the format of the drawing, not the preference: dimension text, worksheet display and printed cells, plant schedules, irrigation reports, site labels. The UI covers the fields and labels of the windows.
- Messages that the engine crates write with numbers inside the text keep the plain format. This holds for the irrigation network and extract reports, plant placement, and the `notes` of commands (English text). The core crates have no dependency on `kadakas-units`.
- Integers inside messages have no digit grouping, because a count is sometimes an id or a year. A size in bytes has grouping at its places (the file limit, the picture limit, the plug-in limit).
- A float that is written with `format!("{}")` and no precision is not found by the check.
- The Indian lakh grouping is not supported.

## Test mode (smoke test of the installers)

The smoke test of the installers (PLAT-28, `docs/release.md`) needs a dialog that nobody clicks. Two variables turn the test mode on. Without `KADAKAS_SMOKE_STATE` nothing changes. A normal start ignores `KADAKAS_SMOKE_HOLD_SECS`.

| Variable | Meaning |
|---|---|
| `KADAKAS_SMOKE_STATE=<path>` | The dialog writes a JSON file to this path after its third frame. Then it waits and answers like the second button ("Use defaults" on a first start). An empty value turns the mode off. |
| `KADAKAS_SMOKE_HOLD_SECS=N` | Seconds between the state file and the answer. The default is 3, the most is 60. A screenshot is taken in this time. |

The state file has this form. It is written in one step, so a reader never sees half of it.

```json
{
  "version": "0.1.0-beta.1",
  "kind": "first_run",
  "modeler_installed": true,
  "shown": {
    "mode": "kadakas-modeler", "language": "en-US", "units": "metric", "number": "1,234.56",
    "theme": "system", "navigation": "mouse", "crash": "ask", "updates": "autoupdate"
  }
}
```

`version` is the version of the launcher. `kind` is `first_run`, `update` or `manual`. The answer goes through the same code as a click. So `welcome.json` and the settings files are written as for a person. The code is `kadakas_welcome::smoke` (the state and the steps, with unit tests) and `smoke_step` in `welcome.rs` of the launcher.

## Check in a real window

The frame tests do not show these. A person looks at them on a real screen before a release. The smoke test of the installers covers the install, the first start and the ping. It does not cover these points.

- **macOS bundle**: no second Dock icon stays after the dialog. The dialog comes in front of Finder when `Kadakas.app` starts from a double click. A second `.kad` double click while the dialog is open opens after the button. A Dock click while the dialog is open brings the dialog forward.
- **Windows**: two double clicks on a `.kad` file at the first start show one dialog. The dialog comes in front of Explorer.
- **Linux**: X11 and Wayland. No dialog without a display.
- **Scale**: 100, 125 and 150 percent. 1366x768 at 125 percent and 1080p at 150 percent with the long Estonian texts. The buttons stay in reach and the content scrolls.
- **Focus and keys**: the focus ring on the cards and the buttons. The Tab order: cards, language, units, number format, More options, the options inside, Use defaults, Start. Enter and Esc with a drop-down open (the drop-down takes them). The close button of the window (it is Use defaults on a first start).
- **Screen reader**: the cards are read as radio buttons with their description and state. The drop-downs are read with their name and value.
- **Live language**: the dialog changes language and window title at once when the language drop-down changes. The number style follows the language when the format is not stored.
- **Trackpad**: a real Mac trackpad and a Windows precision touchpad. Look at the pan direction and Shift for tilt and turn. Look at the pinch speed and the momentum after the fingers leave. Check that a mouse wheel is unchanged.
- **Show welcome**: from the app, from the modeler and from the manage window. The language, the theme, the units, the navigation and the update mode apply at once in the running program. The changes survive continued use: toggle a snap in the app, change the channel in the manage window, and look in the files.

## How to reset

Use Show welcome. Or delete `welcome.json` (and the key `welcome_done` of `launcher.json`) in the install folder. The next start shows the dialog with the current settings. To see the dialog as on a first start, with the defaults of the system, remove two things. The first is the install folder of the launcher (`KADAKAS_LAUNCHER_ROOT`, else `launcher` in the state folder). The second is the settings files (`prefs.json`, `modeler.json`, the feedback `settings.json`). Removing the install folder also removes the downloaded versions.

## Going back to an older version

An older version of the app does not know the Swiss grouping `apostrophe`. If the person chose it, the older version does not read `prefs.json`. It ignores the whole file, shows a warning and starts with the defaults. Its next save replaces the file. No release exists yet, so this is only a risk for builds from before this change. Versions from this one on read an unknown value field by field (see "How files are written"), so the same case later costs one field.

An older app also writes `launcher.json` and `prefs.json` without the keys that it does not know (`navigation`, the new `units` keys). The mark in `welcome.json` stays. When the person comes back to the new version, the dialog does not come again. The choices that the older version dropped have their defaults.

## Code

- `kadakas-welcome` has no window code. `choices` holds the choices and their defaults. `sysloc` holds what the system says, per operating system, with injected answers for tests. `gate` decides when the dialog is due, what each button does, and holds the lock. `stores` holds the table above.
- `kadakas-launcher`: `welcome_ui.rs` (the widget, with frame tests), `welcome.rs` (the process, the step before the start, Show welcome).
- `kadakas-sys`: `jsonfile` (changes key by key under a lock, copies of damaged and good versions), `fsio::lock_file`, `dirs::prefs_dir`, `savelog` (the last try at exit).
- `kadakas-update`: `welcome` (the mark file and the stamp), `Settings` (unknown keys, strict update), `handshake` (`request_welcome`), `Supervisor::with_welcome`.
- `kadakas-units`: `number_convention`, `region_of`, `imperial_region`, `lenient`, and the helpers `num`, `num_range`, `parse_num`, `int`.
- `kadakas-ui-number`: `.ui_number()` for `DragValue` and `Slider`.
- `kadakas-view3d`: `scroll` (the raw wheel events).
- `kadakas_ui_core::nav3d`: `NavStyle`, `scroll_motion`, `ScrollLatch`. `kadakas_ui_core::retry`: the back-off of a failed save.
