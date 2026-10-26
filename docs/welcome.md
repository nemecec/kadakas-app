# The welcome dialog (PLAT-26)

The launcher shows a welcome dialog once, on the very first start. It opens before the launcher starts any version of Kadakas. It always waits for a button, also when the first start was a double click on a `.kad` file. The launcher keeps the file in its inbox. The file opens right after Start or Use defaults.

## What it asks

The first screen has these choices.

- **How do you want to work?** 3D Modeler (simple push and pull shape modeling, the default) or Landscape CAD (plans, sheets and plants). The choice decides which program the launcher starts. Without the modeler in the installed version, only Landscape CAD can be chosen.
- **Language.** The languages of the program, in their own names (English, Eesti). The default is the language of the system, or English when we have no text for it. The dialog changes its language at once, and so does the title of its window.
- **Units.** Metric (mm, m) or Imperial (ft, in), with a line that shows an example.
- **Number format** (next to Units, with the same example line). `1,234.56`, `1.234,56`, `1 234,56`, `1’234.56` (Swiss), `1234.56` or `1234,56`.

The fold **More options** is closed at first:

- **Theme**: System default, Light, Dark.
- **Mouse or trackpad**: how the wheel and the two-finger scroll move the 3D views. *Mouse*: the wheel zooms to the pointer. *Trackpad*: a two-finger scroll moves the view; with Shift a vertical scroll tilts the view and a horizontal scroll turns it; a pinch zooms.
- **Crash reports**: Ask each time (default), Always send, Never send (PLAT-20).
- **Updates**: Auto-update (default), Auto-download, Notify only (PLAT-16).

The text says that all of this can be changed later in Preferences (in the 3D modeler: the View menu) and that Help > Show welcome opens the window again. Buttons: **Start** (Enter) and **Use defaults** (Esc). The close button of the window is the same as Use defaults.

## Where the defaults come from

The defaults come from what the system says about the **regional formats**, not from the language of the interface. An Estonian with an English Windows, or a GNOME with English text and "Formats: Estonia", gets metric units and `1 234,56`.

| System | Source |
|---|---|
| Windows | `GetUserDefaultLocaleName`; `GetLocaleInfoEx` with `LOCALE_IMEASURE` (metric or US), `LOCALE_SDECIMAL`, `LOCALE_STHOUSAND`. The overrides of the user in the Control Panel count. |
| macOS | `NSLocale.currentLocale` (System Settings > General > Language & Region): the identifier, `usesMetricSystem`, the decimal and grouping separators. |
| Linux and others | `LC_ALL`, then `LC_MEASUREMENT` (units) or `LC_NUMERIC` (number format), then `LANG`. The first one that is set and is not `C` or `POSIX` names the locale. |

The system's own answer wins. When the system does not give one, the region of the formats locale decides the units, then the region of the first interface language. No region (a bare `en`) is metric. **Imperial is for the United States, Liberia and Myanmar. The United Kingdom is metric** (a decision of the project).

The number style comes from the separators that the system reports when the pair is one of the six styles above. Otherwise it comes from one table in `kadakas-units` (`number_convention`), which the program also uses for the language of the interface:

| Style | Locales |
|---|---|
| `1,234.56` | English (all regions), Japanese, Chinese, Korean, Hebrew, Thai, Hindi, Spanish in Mexico, the United States and most of Latin America (`es-419`), and every language that the table does not name |
| `1.234,56` | German, Dutch, Spanish, Italian, Portuguese (Brazil), Indonesian, Turkish, Danish, Greek, Romanian, Croatian, Slovenian |
| `1 234,56` | Estonian, Finnish, French, Swedish, Norwegian, Polish, Czech, Slovak, Russian, Ukrainian, Bulgarian, Hungarian, Lithuanian, Latvian, German in Austria, Portuguese in Portugal, French in Switzerland, English in South Africa |
| `1’234.56` | German and Italian in Switzerland |

The Indian lakh grouping (`12,34,567.89`) is not supported: `en-IN` gives `1,234,567.89`.

## What the dialog shows and what it stores

- It shows what the programs really use. A key that is missing has the default of the program that reads it: millimeters (not the region), and the number format of the language. For the 3D modeler, the language, the theme and the navigation of `modeler.json` come first; its own values win for the modeler.
- On a first start there is nothing to compare with: **Start** stores every choice. **Use defaults** stores the defaults for the keys that are **missing** and replaces no value.
- Otherwise only the choices that the person **changed** are stored. A choice that stays as it was is not written. The modeler's own `navigation` is removed only when the person changes the navigation. `units.decimal` and `units.grouping` are written only when the person changes the number format; until then the format follows the language.

| Choice | File | Key |
|---|---|---|
| How to work | `launcher.json` (install folder of the launcher) | `flavor` (`kadakas-modeler` or `kadakas-app`) |
| Updates | `launcher.json` | `mode` |
| Dialog seen | `welcome.json` (install folder) | `done` (the version of the dialog, now 1) |
| Language | `prefs.json` and `modeler.json` (prefs folder) | `language` (`system` or a tag; `null` in `modeler.json`) |
| Units | `prefs.json` | `units.length`, `units.area` |
| Number format | `prefs.json` | `units.decimal`, `units.grouping` |
| Theme | `prefs.json` and `modeler.json` | `theme` |
| Mouse or trackpad | `prefs.json` (the modeler reads it too) | `navigation` (`mouse` or `trackpad`) |
| Crash reports | `settings.json` in the feedback folder (`KADAKAS_FEEDBACK_DIR`) | `crash_policy` |

The prefs folder is the same for the app, the modeler and the dialog (`kadakas_sys::dirs::prefs_dir`): `KADAKAS_PREFS_DIR`, else the folder of `KADAKAS_KEYMAP`, else the config folder.

### How files are written

- Every writer changes **only the keys it changed**, in the file as it is now, under a lock (`kadakas_sys::jsonfile`, a `.lock` file next to the file). The app, the modeler and the window `kadakas-launcher manage` keep their settings in memory; before this rule a later save of an old copy undid what the dialog had stored. Keys that a program does not know stay, in every file.
- The running app and modeler **read their preferences again** when the dialog stores a change: the dialog moves the stamp `applied` in `welcome.json`, and the programs look at it twice a second. The window `manage` reads `launcher.json` again when its dialog ends.
- A file that cannot be parsed is copied to `<name>.bak`, then replaced. A read error that is not "not found" (permission, disk) stops the write and the launcher prints the problem; the file is not replaced by defaults. A write keeps the permissions of the file that it replaces. A problem with one file does not stop the others.
- The mark that the dialog was seen is in its **own file**, `welcome.json`. An older version of the app writes `launcher.json` without the keys that it does not know. A mark in that file would be lost when the person goes back to an older version, and the dialog would come again. The old key `welcome_done` of `launcher.json` is still read.
- The installation ID (PLAT-20) is made when it is first needed, as before. The dialog does not make it.

## When it is shown

- **First start**: nothing is marked and there is no trace of an earlier use. A trace is the install folder of the launcher (`launcher.json`, `versions`, `current`, `runs`), or a settings file of the programs (`prefs.json`, `modeler.json`, the feedback `settings.json`). The dialog starts with the defaults of the system.
- **Update**: a lower mark than the dialog, or traces and no mark. This also holds after a reinstall or a moved launcher folder, when the settings of the person are still there, and for an install from before the dialog. The dialog starts with the **current** values. Cancel only marks the dialog as seen. Start stores what the person changed. Deleting the mark file `welcome.json` (and the old key) gives this dialog again.
- **Show welcome** (Help in the app and in the modeler, and a button in `kadakas-launcher manage`): the dialog starts with the current values. The buttons are Save and Cancel, and Cancel changes nothing. The program writes the request file `runs/<id>.welcome`. The launcher, which watches the run, opens the dialog in a new process. This works only when the launcher started the program. Otherwise the status line says so.

It is never shown:

- in headless use (the `kadakas` command line, MCP, agents, `serve`), because those programs do not call the launcher;
- when `KADAKAS_SKIP_WELCOME=1` (CI and tests; also `true` or `yes`);
- in the browser build, which has no launcher and uses its own defaults;
- when the launcher has no display (Linux without `DISPLAY` or `WAYLAND_DISPLAY`).

## How it runs

The dialog is a window of its own process: `kadakas-launcher welcome [--first-run|--update|--manual]`. The launcher starts it and waits, then starts the app. This is the same on every system. It matters on macOS, where the first process runs the system event loop on its main thread. Files that arrive while the dialog is open wait in the inbox of the launcher.

- **Two launchers at the first start** (a second double click): a lock file `welcome.lock` in the install folder is held while the dialog is open. The second launcher waits, looks again, finds the mark and starts. The operating system releases the lock when a program ends, so a lock is never stale.
- **A dialog that fails** (the window cannot open, a crash at start, a non-zero exit) does not stop the start. The launcher prints the reason, writes no mark and starts the app with the settings as they are. The dialog comes again at the next start. A dialog that is open but hidden still holds the start, by design. `KADAKAS_WELCOME_TIMEOUT_SECS=N` sets a limit (the process is killed after N seconds); it is for tests and has no default.

## Mouse or trackpad

The style is `navigation` in `prefs.json`. The modeler can also keep its own choice in `modeler.json` (View > Mouse or trackpad); that choice wins for the modeler until the person changes the navigation in the dialog.

- *Mouse*: the wheel zooms to the pointer, exactly as before the style existed. (egui turns a scroll with Shift into a horizontal one, so the wheel with Shift does not zoom.)
- *Trackpad*: the app reads the raw wheel events of the window system. egui turns a vertical scroll with Shift into a horizontal one, and keeps the modifier for a whole gesture, so it cannot be used for this. A scroll pans. With Shift, a vertical scroll tilts (pitch) and a horizontal scroll turns (yaw). The choice of Shift is made when the gesture starts: letting go of Shift in the middle, or during the momentum after the fingers left, does not turn an orbit into a pan; pressing Shift in the middle does. The pinch zooms.

## Numbers in the UI

The number format is the one preference that the whole UI uses: a decimal symbol (`.` or `,`) and a digit grouping (none, `,`, `.`, space, `’`).

- **Shown**: lengths, areas, angles, the Measurements box and the fields of the programs; the live labels of the tools; the numbers inside messages (`tr!` writes a number argument with the decimal symbol, `kadakas_l10n::set_decimal_symbol`); byte counts with digit grouping.
- **Typed**: every number field (`DragValue`, `Slider`) writes and reads the number in the format and accepts both `.` and `,` as the decimal separator (`.ui_number()`, `kadakas_view3d::numfmt`). The criteria and the default of a field accept `1,5`; a worksheet cell entry that is a plain number accepts it too. In a **formula** the comma separates arguments, so numbers in formulas are written with a point.
- **`1,234`** is ambiguous. With a format that groups with a comma it is 1234. With a decimal comma it is 1.234. The format decides.
- **Not affected**: files, the JSON-RPC API, MCP, the command line, exports (DXF, IFC, CSV, STEP, glTF, SVG, PDF coordinates), ids, logs and the formula grammar. They use the plain format (a point, no grouping).
- **The check**: `scripts/check-number-format.sh` (in CI, job `number-format`) fails for a number widget without `.ui_number()` or a unit extension, and for `format!("{:.2}", x)` in the UI crates. `scripts/number-format-allow.txt` lists the exceptions, with a reason each.

### Limits

- **Drawing text is not covered** (PLAT-27): the dimension text of the drawing (`kadakas-core` annot), the display and the printed cells of a worksheet, the plant schedule and lists, the irrigation report and the site labels follow the format of the drawing, not the preference. The UI covers the fields and labels of the windows.
- Messages that the engine crates write with numbers inside the text (irrigation network and extract reports, plant placement) and the `notes` of commands (English text) keep the plain format. The core crates have no dependency on `kadakas-units`.
- Integers inside messages have no digit grouping (a count can be an id or a year). A size in bytes is formatted with grouping at its two places (`file-io-too-big`, the picture limit, the plug-in limit).
- A float in `format!("{}")` without a fixed number of decimals is not found by the check.

## Check in a real window

The frame tests cannot show these. A person must look, on a real screen, before a release.

- **macOS bundle**: the dialog does not make a second Dock icon that stays; it comes in front of Finder when `Kadakas.app` starts from a double click; a second `.kad` double click while the dialog is open opens after the button; a Dock click while the dialog is open brings the dialog forward.
- **Windows**: two double clicks on a `.kad` file at the first start show one dialog; the dialog comes in front of Explorer.
- **Linux**: X11 and Wayland; no dialog without a display.
- **Scale**: 100, 125 and 150 percent; 1366x768 at 125 percent and 1080p at 150 percent with the long Estonian texts; the buttons stay in reach and the content scrolls.
- **Focus and keys**: the focus ring on the cards and the buttons; Tab order (cards, language, units, number format, More options, the options inside, Use defaults, Start); Enter and Esc with a drop-down open (the drop-down takes them); the close button of the window (it is Use defaults on a first start).
- **Screen reader**: the cards are read as radio buttons with their state; the drop-downs have names.
- **Live language**: the dialog changes language and window title at once when the language drop-down changes.
- **Trackpad**: a real Mac trackpad and a Windows precision touchpad: the pan direction, Shift for tilt and turn, the pinch speed, the momentum after the fingers leave, and that a mouse wheel is unchanged.
- **Show welcome**: from the app, from the modeler and from the manage window; the language, the theme, the units and the navigation apply at once in the running program; changes survive continued use (toggle a snap in the app, change the channel in the manage window, and look in the files).

## How to reset

Use Show welcome. Or delete `welcome.json` (and the key `welcome_done` of `launcher.json`) in the install folder: the next start shows the dialog with the current settings. To see the dialog with the defaults of the system, as on a first start, remove the install folder of the launcher (`KADAKAS_LAUNCHER_ROOT`, else `launcher` in the state folder) and the settings files (`prefs.json`, `modeler.json`, the feedback `settings.json`). Removing the install folder also removes the downloaded versions.

## Going back to an older version

An older app writes `launcher.json` and `prefs.json` without the keys that it does not know (`navigation`, the new `units` keys). The mark in `welcome.json` and the settings the older version knows stay. When the person comes back to the new version, the choices that the older version could not keep (the mouse or trackpad style) are lost and have their defaults. The dialog does not come again.

## Code

- `kadakas-welcome` (no window code): `choices` (the choices and their defaults), `sysloc` (what the system says, per operating system, with injected answers for tests), `gate` (when the dialog is due, what each button does, the lock), `stores` (the table above).
- `kadakas-launcher`: `welcome_ui.rs` (the widget, with frame tests), `welcome.rs` (the process, the step before the start, Show welcome).
- `kadakas-sys`: `jsonfile` (changes key by key under a lock), `fsio::lock_file`, `dirs::prefs_dir`.
- `kadakas-update`: `welcome` (the mark file and the stamp), `Settings` (unknown keys, strict update), `handshake` (`request_welcome`), `Supervisor::with_welcome`.
- `kadakas-units`: `number_convention`, `region_of`, `imperial_region`, and the helpers `num`, `num_range`, `parse_num`, `int`.
- `kadakas-view3d`: `numfmt` (`.ui_number()`), `scroll` (the raw wheel events).
- `kadakas_ui_core::nav3d`: `NavStyle`, `scroll_motion`, `ScrollLatch`.
