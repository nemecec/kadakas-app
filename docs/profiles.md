# Workspace profiles (PLAT-04)

A workspace profile makes Kadakas feel like another CAD program. One app holds several profiles, and you switch between them while it runs. A profile bundles:

- a keymap (all key chords),
- command aliases (short names that you type in a small field),
- the tool sets of the Tool bar: which sets exist, their order, and the tools in each,
- the menus: which commands are in which menu, and in which order,
- a terminology overlay: some terms of the UI replaced, per language,
- optional default units and formats.

Two profiles are built in:

| Profile | Id | What it is |
|---|---|---|
| Vectorworks-like | `vectorworks` | The default. Keys, menus, tool sets and terms follow Vectorworks 2026. It is what Kadakas had before profiles, unchanged. |
| AutoCAD-like | `autocad` | About 90 command aliases that you type in the command field, Draw, Modify and Annotate menus and tool sets, and terms such as Layer, Drawing and Dynamic input. |

AutoCAD is a trademark of Autodesk, Inc. Kadakas names it only to say what the profile is like. The profile uses public general knowledge of common command aliases. It has no Autodesk text, icons or files. See `docs/attribution.md`.

## Use a profile

- **Preferences > Workspace profile** shows the profiles. Pick one and it applies at once: keys, menus, tool sets, terms and the command field.
- **Start with a profile.** The launcher passes `--profile NAME` and sets `KADAKAS_PROFILE` (PLAT-06). The app also accepts them directly. `NAME` is the id or the name of a profile, ignoring case and punctuation (`autocad`, `AutoCAD-like` and `autocad_like` all work). An unknown name shows a note and the app uses the last profile or the default. Without a name, the app starts with the profile of the last session (the id in `profile.txt` in the config folder).
- **Duplicate** makes a copy that you own. The copy remembers its source (`based_on`). **Rename**, **Delete** and **Export** work on the active profile. **Delete** asks first. **Import** reads a profile file and adds it with a free id and name. A file over 1 MB is refused before it is read. The web build has no user profiles on disk.
- Built-in profiles are read-only. When you change one (a key, the order of tools), Kadakas makes your own copy first, switches to it, and says so.
- **Shortcuts and tools...** opens the editor. For each action it lists the keys, with a button to remove a key, add a key (press the new key; Esc cancels) and reset the action. **Reset** gives the keys of the profile that this one was copied from (the Vectorworks-like defaults for a profile without a source). If the new key belongs to another action, the editor names that action and asks before it moves the key. Below the keys, each tool set lists its tools with up and down buttons.

Where the files are:

| System | Folder |
|---|---|
| macOS | `~/Library/Application Support/kadakas/profiles/` |
| Linux | `$XDG_CONFIG_HOME/kadakas/profiles/`, else `~/.config/kadakas/profiles/` |
| Windows | `%APPDATA%\kadakas\profiles\` |

`KADAKAS_PREFS_DIR` moves the config folder. A user profile is `profiles/<id>.json`, and **the id is the file name without `.json`** (the `id` inside the file is ignored). Kadakas writes and deletes the file it read. A file whose name is no valid id (`My Profile.json`), or whose id is taken, is renamed to a valid free id when it loads.

The old `keymap.json` is read once at start and becomes the keymap of the built-in Vectorworks-like profile. So the Shortcuts window shows the keys that work, and a copy of that profile starts with them.

## The command alias field

Some programs let you type a command name. Kadakas has no command line, so a profile with aliases shows a compact field in the status bar. Press Enter (when nothing is being drawn) or the key of `ui.command_line` (Cmd/Ctrl+9 in the AutoCAD-like profile) to focus it. Type letters and press Enter. The field shows the action that Enter will run, or the aliases that start with your letters. An empty field repeats the last command. Up and Down step through earlier commands. Esc empties the field. An unknown alias stays in the field and shows a message.

**Type to command.** In a profile with aliases, a letter or digit that no key of the profile uses goes to the field, and the field takes the focus. You type `PL` anywhere and press Enter. Single keys never fire while the field has focus. A key of the keymap that has a modifier still works (`Cmd/Ctrl+F`, `Shift+T`).

The AutoCAD-like profile has no single-letter tool keys on purpose. A key such as `L` would run the Line tool before you could type `LE`, so a key for `L` and an alias `LE` cannot live together. Every tool is an alias instead. Your own profile may bind a letter; the letter then runs its action at once and never reaches the field. Cmd/Ctrl+9 is a browser shortcut in the web build (it picks a tab). In the browser, press Enter to focus the field.

The state machine of the field (`CommandLine`) is in `kadakas-ui-core`, so another UI can reuse it. The app draws the field.

## The profile file

A profile is a JSON object. `format`, `id` and `name` come first; every other part is optional. A missing part takes the default of the Vectorworks-like profile (keys, tool sets, menus) or is empty (aliases, terms, units). So a file can hold only what differs.

```json
{
  "format": 1,
  "id": "my-profile",
  "name": "My profile",
  "description": "Optional text.",
  "keymap": { "tool.line": ["L"], "tool.rect": [], "view.zoom_fit": ["Z", "Home"] },
  "aliases": { "PL": "tool.polyline", "ZE": "view.zoom_fit" },
  "tool_sets": [ { "id": "draw", "tools": ["tool.select", "tool.line"] } ],
  "menus": [ { "id": "draw", "items": ["tool.line", "-", "tool.circle"] } ],
  "terms": { "en-US": "panels-class = Layer\n", "et": "panels-class = Kiht\n" },
  "units": { "length": "m", "angle": "decimal" }
}
```

| Key | Content |
|---|---|
| `format` | Required. The version of the file format. Today it is `1`. A file with a higher number is refused, with a message. When the format changes in a way that old files cannot read, the number goes up and the app converts old files. |
| `id` | Lower case letters, digits, `-` and `_`, 1 to 24 characters. It is the file name without `.json`. Without it, the file name counts. |
| `name` | Required, at most 60 characters. Shown in the list (the built-in profiles show a translated name). |
| `description` | Optional text, at most 500 characters (longer text is cut). |
| `based_on` | Optional id of the profile that this one was copied from. Used by Reset. |
| `keymap` | Action ids and chords, as in `keymap.json`: `"tool.line": "G"` or `["G", "Alt+2"]`. An empty list removes all keys of the action. An action that the part names loses its default keys first. An action that it does not name keeps the Vectorworks-like default. A key that is also a default key of an action that the part does not name moves to the named action, and the loader lists it as a problem. The list of action ids is the first column of `docs/keyboard-shortcuts.md`. An exported file holds all actions. |
| `aliases` | Alias to action id. An alias is 1 to 16 letters or digits (upper case is made from lower case). The target is any action id (`tool.polyline`, `edit.undo`, `cmd.duplicate_array`, ...). |
| `tool_sets` | A list of `{ "id", "name"?, "tools": [...] }`. A tool is the action id of the tool (`tool.line`). The order in the list is the order in the Tool bar. A set with a known id (`basic`, `modify`, `landscape`, `site`, `3d`, `irrigation`, `annotation`, `draw`, `annotate`) has a translated title; any other set needs a `name`. The options of the current tool show under the set that holds it. |
| `menus` | A list of `{ "id", "name"?, "items": [...] }`. An item is `-` (a separator), a menu command or a tool id. Known menu ids with a translated title: `file`, `edit`, `modify`, `model`, `site`, `view`, `window`, `help`, `draw`, `annotate`. Any other menu needs a `name`. |
| `terms` | Language tag to a Fluent text. See below. |
| `units` | The same object as `units` in `prefs.json` (`length`, `area`, `angle`, ...). The app applies it when the person picks the profile. It does not change again when the app restarts. |

Menu commands: `file.new`, `file.open`, `file.save`, `file.save_as`, `file.export_svg`, `file.export_pdf`, `file.import`, `file.export`, `file.map_sources`, `file.change_crs`, `file.scripts`, `file.versions`, `file.compare`, `view.agent_activity`, `file.collaboration` (the submenu File > Collaboration), `file.preferences`, `edit.undo`, `edit.redo`, `edit.delete`, `edit.select_all`, `ui.quick_search`, `cmd.duplicate_array`, `cmd.rotate_left_90`, `cmd.rotate_right_90`, `cmd.flip_horizontal`, `cmd.flip_vertical`, `cmd.add_surface`, `cmd.subtract_surface`, `cmd.clip_surface`, `cmd.intersect_surface`, `cmd.compose`, `cmd.extrude`, `cmd.sweep`, `cmd.create_from_shapes`, `cmd.split_hardscape`, `cmd.check_unit_patterns`, `site.create_model`, `site.update_model`, `site.create_texture_bed`, `view.zoom_fit`, `view.history_previous`, `view.history_next`, `view.show_grid`, `view.snapping`, `view.theme`, `view.icon_size`, `window.panels`, `window.layouts`, `help.show_welcome`, `help.about`. The browser build leaves out the commands that need files.

Limits: a file is at most 1 MB; a profile has at most 24 menus, 24 tool sets, 200 entries per menu or set and 500 aliases; one terms text is at most 64 KB. Extra entries are ignored and listed as a problem.

How the loader treats mistakes: it never stops for a wrong part. It uses the default for that part and lists a problem (an unknown action, a bad key, an unknown tool or menu item, a duplicate id, a wrong type, a bad alias, a bad terms text). The problems show in the status line when the app loads a file, and after an import. Only a file that cannot be a profile at all is refused: not JSON, not an object, no `format`, a format that is too new, or no name.

The file `crates/kadakas-ui-core/profiles/vectorworks.json` is a full example. A test keeps it equal to the default profile.

## Terminology overlays

`terms` renames terms through Fluent. Each language tag holds a `.ftl` text. Its messages replace the messages with the same id:

```
panels-class = Layer
panels-classes = Layers
panels-add-class = Add Layer
```

- The ids are the message ids in `crates/kadakas-l10n/locales/en-US/`.
- **Only terms can be replaced.** The list is `OVERLAYABLE` in `crates/kadakas-l10n/src/overlay.rs`: names of classes, layers, tools and dialogs. Messages about licenses (`about-*`), scripts (`script-ui-*`), autosave (`autosave-ui-*`), the profile windows (`profiles-ui-*`, `uic-profile-*`) and the common buttons (`common-*`) are never replaceable. `set_overlay` and `with_overlay` drop other messages, whatever the caller.
- A message may use only the variables of the original message. It may not refer to other messages or terms and may not have attributes. A message that fails to format falls back to the normal text.
- The overlay applies while its language is active. Give a text for each language that you want to change. A profile with only `en-US` leaves Estonian as it was.
- The loader drops the text of a language that has a syntax error or a wrong variable, and lists a problem. An id that does not exist does no harm and is listed.
- The overlay is a preference of the person, like the language. It is not document data and the API never sees it.

The AutoCAD-like profile changes Class to Layer (Kiht) and Design Layer to Drawing (Joonis), so the bare word "Layer" means the class only. It changes every message that names a class or a design layer (Object Info, Navigation, the import and export dialogs, snapping, plants). It also changes Sheet Layers to Layouts (Paigutused), Selection tool to Select (Valik), Duplicate Array to Array (Massiiv), and Data bar to Dynamic input (Dünaamiline sisend). A test checks that labels that sit next to each other never read the same. The texts are `crates/kadakas-ui-core/profiles/autocad.en-US.ftl` and `autocad.et.ftl`.

## For developers

- Model: `kadakas_ui_core::profile` (`Profile::parse`, `to_json`, `Profile::builtins`, `ProfileSet`, `CommandLine`, `layout`). It is pure and builds for wasm. The app crate holds the files and the windows (`crates/kadakas-app/src/profiles.rs`).
- Key edits: `Keymap::set_chords` and `Keymap::conflict_for` find and resolve conflicts. `Profile::set_chords`, `reset_chords`, `move_tool` and `set_alias` edit a user profile.
- Terms: `kadakas_l10n::set_overlay`, `check_overlay`, and `with_overlay` (one thread; for tests).
- Add a profile to the program: put the JSON in `crates/kadakas-ui-core/profiles/`, add it to `Profile::builtins`, and add its name and description to `uic-profile.ftl` in both languages.
