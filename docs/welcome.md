# The welcome dialog (PLAT-26)

The launcher shows a welcome dialog once, on the very first start. It opens before the launcher starts any version of Kadakas. It always waits for a button, also when the first start was a double click on a `.kad` file. The launcher keeps the file in its inbox. The file opens right after Start or Use defaults.

## What it asks

The first screen has three choices.

- **How do you want to work?** 3D Modeler (simple push and pull shape modeling, the default) or Landscape CAD (plans, sheets and plants). The choice decides which program the launcher starts. Without the modeler in the installed version, only Landscape CAD can be chosen.
- **Language.** The languages of the program, in their own names (English, Eesti). The default is the language of the system, or English when we have no text for it. The dialog changes its language at once.
- **Units.** Metric (mm, m) or Imperial (ft, in). The default is imperial for the regions US, LR (Liberia) and MM (Myanmar), else metric. A line shows an example.
- **Number format** (next to Units, with the same example line). `1,234.56`, `1.234,56`, `1 234,56`, `1234.56` or `1234,56`. The default comes from the language of the system: `en`, `ja` and unknown languages give `1,234.56`. `de`, `nl`, `es`, `it`, `pt` give `1.234,56`. `et`, `fi`, `fr`, `sv`, `nb`, `pl`, `cs`, `ru` give `1 234,56`.

The fold **More options** is closed at first:

- **Theme**: System default, Light, Dark.
- **Mouse or trackpad**: how the wheel and the two-finger scroll move the 3D views. *Mouse*: the wheel zooms to the pointer. *Trackpad*: a two-finger scroll moves the view, with Shift it orbits, a pinch zooms.
- **Crash reports**: Ask each time (default), Always send, Never send (PLAT-20).
- **Updates**: Auto-update (default), Auto-download, Notify only (PLAT-16).

The text "You can change all of this later in Settings." is on the screen. Buttons: **Start** (Enter) and **Use defaults** (Esc). Use defaults stores everything as detected, whatever the screen shows. The close button of the window is the same as Use defaults.

## Where the choices go

| Choice | File | Key |
|---|---|---|
| How to work | `launcher.json` (install folder of the launcher) | `flavor` (`kadakas-modeler` or `kadakas-app`) |
| Updates | `launcher.json` | `mode` |
| Dialog seen | `launcher.json` | `welcome_done` (the version of the dialog, now 1) |
| Language | `prefs.json` and `modeler.json` (config folder, or `KADAKAS_PREFS_DIR`) | `language` (`system` or a tag; `null` in `modeler.json`) |
| Units | `prefs.json` | `units.length`, `units.area` |
| Number format | `prefs.json` | `units.decimal`, `units.grouping` |
| Theme | `prefs.json` and `modeler.json` | `theme` |
| Mouse or trackpad | `prefs.json` (the modeler reads it too) | `navigation` (`mouse` or `trackpad`) |
| Crash reports | `settings.json` in the feedback folder (`KADAKAS_FEEDBACK_DIR`) | `crash_policy` |

Every file is changed key by key. Keys that the dialog does not know stay. A file that is damaged is replaced, and the old text stays as `<name>.bak`. A problem with one file does not stop the others; the launcher prints it. The program reads the preferences when it starts, so a change applies at the next start. The installation ID (PLAT-20) is made when it is first needed, as before; the dialog does not make it.

The number format is the one preference that the whole UI uses (`kadakas_units`). Typed numbers accept both `.` and `,` as the decimal separator when the symbol cannot be a digit group. `1,234` is ambiguous: with a format that groups with a comma it is 1234, with a decimal comma it is 1.234. The format decides. Files, the JSON-RPC API, the command line and exports always use the plain format (dot, no grouping).

## When it is shown

- **First start**: nothing is stored and the install folder has no trace of an earlier use (`launcher.json`, `versions`, `current`, `runs`). The dialog starts with the defaults of the system.
- **A newer dialog**: `welcome_done` is lower than the version of the dialog. This also holds for an install from before the dialog (it has the traces above and no `welcome_done`), and for a person who deleted the key. The dialog starts with the current settings. The second button is Cancel (it only marks the dialog as seen). Start stores what the screen shows.
- **Show welcome** (Help in the app and in the modeler, and a button in `kadakas-launcher manage`): the dialog starts with the current settings. The buttons are Save and Cancel, and Cancel changes nothing. The app and the modeler write the request file `runs/<id>.welcome`. The launcher, which watches the run, opens the dialog in a new process. This works only when the launcher started the program. Otherwise the status line says so.

It is never shown:

- in headless use (the `kadakas` command line, MCP, agents, `serve`), because those programs do not call the launcher;
- when `KADAKAS_SKIP_WELCOME=1` (CI and tests; also `true` or `yes`);
- in the browser build, which has no launcher and uses its own defaults;
- when the launcher has no display (Linux without `DISPLAY` or `WAYLAND_DISPLAY`).

## How it runs

The dialog is a window of its own process: `kadakas-launcher welcome [--first-run|--update|--manual]`. The launcher starts it and waits, then starts the app. This is the same on every system. It matters on macOS, where the first process runs the system event loop on its main thread. Files that arrive while the dialog is open wait in the inbox of the launcher.

## How to reset

Use Show welcome. Or delete the key `welcome_done` in `launcher.json`: the next start shows the dialog with the current settings. To see the dialog with the defaults of the system, as on a first start, remove the install folder of the launcher (`KADAKAS_LAUNCHER_ROOT`, else `launcher` in the state folder). That also removes the downloaded versions.

## Code

- `kadakas-welcome` (no window code): the choices, the defaults from the locales of the system, `gate` (when the dialog is due, what each button does), `stores` (the table above).
- `kadakas-launcher`: `welcome_ui.rs` (the widget, with frame tests), `welcome.rs` (the process, the step before the start, Show welcome).
- `kadakas-update`: `Settings::welcome_done` and the unknown keys, `handshake` (`request_welcome`), `Supervisor::with_welcome`.
- `kadakas_ui_core::nav3d`: `NavStyle`, `scroll_motion`.

## Limits

- The system's own number separators are not read; the number format follows the language. The region for the units comes from the first locale of the system. Where the system gives no region (a bare `en`), the units are metric.
- Mouse or trackpad changes the wheel and the scroll. It does not change the drag buttons.
- The look of the dialog (the cards, the focus ring, the sizes at 125 and 150 percent on a real screen) has been checked by frame tests only. It needs a look in a real window.
