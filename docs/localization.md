# Localization, units and formats (PLAT-05)

Kadakas shows its user interface in more than one language. Today it has two: English (en-US) and Estonian (et). This page explains how the text works. It also tells you how to add a language and which rules apply to terms.

## What is localized

| Localized (in `.ftl` files) | Not localized (stays English) |
|---|---|
| Menus, palettes, dialogs, tool names, hints, status messages, the Data bar, Object Info labels, Preferences, About. | The JSON-RPC API: method names, parameters, ids, and the texts of errors and results. |
| Texts that `kadakas-ui-core` makes: tool hints, readouts, notices, key labels. | The output of the `kadakas` CLI and the MCP tools. |
| The look of numbers, lengths, areas, angles and dates. | Files. The document format, DXF, IFC and CSV keep plain numbers and ISO dates. |

**The rule for agents.** API and JSON-RPC messages are for agents and scripts. They are not for people. They stay in English, and the language of the window never changes their form. If a dialog shows such a text, the dialog puts it inside a localized sentence, for example `Error: { $e }`.

The core crates know some values only by an API id, for example `miter`, `shrub_mound` or `left_isometric`. The ids stay in English. The UI shows names for them from `enum_names.rs`.

Document content is data. It is not UI text. A new layer is named `Layer 3` and a new plant style is named `Plant style`, in every language. The name goes into the file and into the API.

## The crates

- `kadakas-l10n` holds the text. It uses [Project Fluent](https://projectfluent.org) (`fluent-bundle` and `unic-langid`). The `.ftl` files are in `crates/kadakas-l10n/locales/<tag>/`. The build puts them into the program with `include_str!`, so the web build needs no file access. The crate is pure and builds for wasm.
- `kadakas-units` holds the formats for lengths, areas, volumes, angles, numbers and dates. It writes them and it reads them. It is pure too.
- `kadakas-ui-core` and `kadakas-app` call both crates. The tool code of `ui-core` has no egui. It uses the same two crates.

## How to write a text in code

```rust
use kadakas_l10n::tr;

ui.label(tr!("panels-tools"));                         // a text
ui.label(tr!("area-ui-hole-s", count = holes.len()));  // a text with an argument
```

- `tr!("id")` and `tr!("id", name = value)` give a `String`. A number argument stays a number, so the plural rules work. Any other value that implements `Display` becomes text.
- `msg!("id")` marks an id in a table or in a `const`, where `tr!` cannot run. Later, the code calls `kadakas_l10n::translate(id, &[])`.
- A number that a person reads goes through `kadakas-units` first. Use `kadakas_units::current().length_text(mm)`, `area_text`, `angle_text`, `number_text` or `date_text`. Then pass the result as a text argument. Never write `format!("{:.1} mm", x)` in the UI.
- A number field that a person edits uses `DragValueExt` from `crates/kadakas-app/src/units_ui.rs`. The methods are `.length()`, `.length_m()`, `.degrees()`, `.area_m2()`, `.volume_m3()` and `.per_area()`. The value keeps its unit (mm, m, degrees). The widget shows and reads the units of the person.
- A window title is also the id of the window in egui. Give each window a stable id, for example `Window::new(tr!("...")).id(egui::Id::new("win-..."))`. Then the window stays in place after a change of the language.
- Do not build a sentence from pieces of text. Each language has its own word order. Give the whole sentence one message with arguments.
- Names of values that a core crate knows by an id go in `enum_names.rs` and in `enum-names.ftl`. The message is `enum-<domain>-<id>`. A test compares the list with the real enums.

## Message ids and files

- A message id has lower case letters, digits and hyphens. The form is `<module>-<words>`. The module is the source file, for example `panels` or `plant-ui`. Modules of `ui-core` start with `uic-`. Labels of keys are `action-<action id>`.
- Each source module has one `.ftl` file. The file `locales/en-US/panels.ftl` belongs to `panels.rs`. Add each new file to the list in `crates/kadakas-l10n/src/resources.rs`. If you forget, the test `every_ftl_file_is_listed` tells you.
- Use Fluent variables (`{ $count }`) for everything that changes. Use plural selectors for counts:

```
area-ui-hole-s =
    { $count ->
        [one] { $count } hole
       *[other] { $count } holes
    }
```

- A translator may need context. Write it as a comment with `#` above the message.
- A Fluent text has no markup. A text with a line break is a multi-line message.

## The CI check

The command `cargo test -p kadakas-l10n` runs `tests/l10n_check.rs`. The test fails in these cases:

1. A language lacks a message that en-US has, or it has a message that en-US lacks.
2. A translation uses other variables than the English message.
3. A message does not format without an error, for example because a variable is missing. The test also fails for an id that two files define.
4. The code uses a message id that en-US does not have. The ids are in `tr!("..")`, `msg!("..")` and `translate("..")`.
5. An en-US message is not used by any code. Delete it or use it.
6. A user-visible string literal is left in an egui call of `kadakas-app` or `kadakas-ui-core`. The calls are `.label("..")`, `.button("..")`, `.heading`, `.checkbox`, `.selectable_value`, `.on_hover_text`, `Window::new`, `.hint_text` and similar.

The scan for check 6 is a heuristic. It reads the source text line by line. A literal that is not a word needs an entry in `LITERAL_OK` in `tests/l10n_check.rs`. Examples are a symbol, an example value and the operator of a query language. Write the file, the text and a comment.

Run `cargo test -p kadakas-l10n` before each commit that touches UI text.

## How to add a language

1. Make the folder `crates/kadakas-l10n/locales/<tag>/`, for example `fi`. Use a BCP 47 tag.
2. Copy all `.ftl` files from `locales/en-US/` into it. Translate the texts. Keep the ids and the variables. The plural selectors need the plural categories of the language: `[one]`, `[few]`, `[many]` and `[other]`. The `*` marks the default variant.
3. List the files in `crates/kadakas-l10n/src/resources.rs` with a new `FILES_<TAG>` list. Add the language to `LANGUAGES` in `src/lang.rs`. Each entry has a `tag`, a `native_name` and the `files`.
4. Add the number and date conventions of the language to `Units::conventions` in `crates/kadakas-units/src/lib.rs`. Set the decimal separator, the grouping and the date order.
5. Run `cargo test -p kadakas-l10n -p kadakas-app`. The CI check finds missing messages and wrong variables. The app test `the_window_draws_in_every_language_and_in_every_unit_system` draws the whole window in each language.
6. Add a row to the glossary below. If the translation uses a source, add it to `docs/attribution.md`.

The language list in Preferences comes from `LANGUAGES`. It shows the native name of each language.

## Rules for terms

- The English texts follow the terms of Vectorworks 2026 (see `docs/dev-guide.md`). Examples are Selection tool, Data bar, Object Info, Plant, Landscape Area, Hardscape, Site Modifiers and Duplicate Array.
- A translation uses the established word of its language for the same thing. The same word must appear in the whole program. Keep a glossary for the language. If a word has two meanings, use two messages.
- Do not translate file formats and standards (DXF, IFC, WMS). Do not translate unit symbols (mm, kPa, l/min), the names of keys (Esc, Enter, Tab) or the name Kadakas. Keep API ids and file extensions. Keep the text that a person types in a query language, for example `Kind = "plant"`, `AND` and `OR`.
- Keep the keyboard shortcuts of the keymap. Translate only the label of the action.
- Write short sentences. Use the imperative for buttons and hints, for example "Klõpsa ...". The older Estonian texts use the informal "sa". New texts (the collaboration texts) use impersonal forms, for example "tuleb küsida" and "jäeti alles", and avoid "sa" and "sina".
- A message with a trademark keeps the name of the owner (`Vectorworks`, `AutoCAD`) and the note of the owner. See `docs/attribution.md`.

### Estonian glossary

| English | Estonian | Note |
|---|---|---|
| Landscape Area | Haljastusala | area with planting |
| Collaboration, shared document | Koostöö, ühisdokument | the document that several people edit |
| Token (access) | Luba | the key that the server gives; not "märk" |
| Presence, people here | Kohalolek, Inimesed | the palette is "Inimesed" |
| Hardscape | Kõvakate | paving, walls, hard surfaces |
| Site Modifiers, Site model | Maapinna muutjad, Maapinnamudel | |
| Grading, Grade limits | Vertikaalplaneerimine, Planeerimispiir | vertical planning |
| Grade (line), Slope | Kaldejoon, Kalle | |
| Grid (snap, plant grid), Mesh | Ruudustik, Võrk | |
| Fill (attribute), Fill (earthwork) | Täitevärv, Täide | |
| Cut / Fill / Net | Kaeve / Täide / Bilanss | |
| Spacing | Vahe | |
| Spot elevation | Kõrgusmärk | |
| Contours | Horisontaalid | the word on Estonian base maps |
| Cut / Fill | Kaeve / Täide | |
| Plant style | Taimestiil | |
| Botanical / common name | Ladinakeelne nimi / Rahvapärane nimi | |
| Spread (canopy) | Võra laius | |
| Trunk diameter | Tüve läbimõõt | |
| Root protection | Juurekaitse | |
| Hydrozone | Hüdrotsoon | |
| Irrigation, Outlet, POC | Kastmine, Pihusti, Ühenduspunkt | |
| Data bar, Tool bar | Andmeriba, Tööriistariba | |
| Object Info | Objekti info | |
| Version (a saved state of the drawing) | Versioon | File > Versions; "Restore" is Taasta, "Compare" is Võrdle |
| Plug-in | Lisamoodul | WebAssembly plug-ins and Python plug-in objects |
| Design layer, Sheet layer, Class | Joonestuskiht, Lehekiht, Klass | |
| Snapping | Haaramine | |
| Dimension | Mõõtmestamine (tool), Mõõt (value) | |
| Callout, Data Tag, Revision Cloud | Viide, Andmesilt, Muudatuspilv | |
| North Arrow, Scale Bar | Põhjanool, Mõõtkavariba | |
| Extrude, Sweep (Model menu) | Väljapressimine, Pöördkeha | Sweep is a profile turned around an axis; the AutoCAD-like profile says Revolve / Pööre |
| 3D Polygon | 3D-hulknurk | the AutoCAD-like profile says 3D Polyline / 3D-murdjoon |
| Arc Angle, Start Angle, Segment Angle, Pitch (of a sweep) | Kaare nurk, Alguse nurk, Lõigunurk, Samm | the Sweep dialog; Extrusion is Väljapress |
| Dimension standard, Notes database | Mõõtmestamise standard, Märkuste andmebaas | style editors |
| Area (dimension) | Pindala | the Area mode of the Dimension tool |
| Picture (of a plant style) | Pilt | PNG or JPEG file in a plant style |
| Texture, Tile size (of a texture) | Tekstuur, Mustri kordus | the terrain look of a site model |
| Elevation bands, Color ramp | Kõrgusvööndid, Värviskaala | colors by elevation on the terrain |
| Texture bed | Tekstuuriala | a region of the terrain with its own texture; the plural stays "tekstuuriala" after a number |

## Terminology overlays (PLAT-04)

A workspace profile can rename terms. Its `terms` section holds one `.ftl` text per language tag. The messages in it replace the messages with the same id, for example `panels-class = Layer` for an AutoCAD-like profile. The overlay is process wide, like the language (`kadakas_l10n::set_overlay`), and it applies only while that language is active. Only the messages in the `OVERLAYABLE` list of `overlay.rs` can be replaced, and never the groups `about-*`, `script-ui-*`, `autosave-ui-*`, `profiles-ui-*`, `uic-profile-*` and `common-*`. A message that you want a profile to rename must be in that list. A text is at most 64 KB. A profile that wants a new term in Estonian needs an Estonian text too. `check_overlay` finds a syntax error, an id that does not exist or may not be replaced, a variable that the original message lacks, and a reference to another message; the profile loader shows these as problems. A text with a syntax error is dropped, and the program drops every other message that fails a rule. The built-in texts are `crates/kadakas-ui-core/profiles/autocad.en-US.ftl` and `autocad.et.ftl`. A test checks them. See `docs/profiles.md`.

## The language preference

The default language comes from the system. On the desktop, the program reads the locale list of the operating system (`sys-locale`). In the browser, it reads `navigator.languages`.

The function `kadakas_l10n::negotiate` picks the supported language that fits best. An exact tag wins. Then the same base language wins. If nothing fits, it uses en-US.

The language list is in File > Preferences > Language. It has "System default" and each supported language. The program saves the choice in `prefs.json` as the tag (`"et"`) or as `"system"`. The browser build does not keep preferences between visits.

The language changes at once. The app makes every text again in each frame from the messages of the active language. There is no restart. The function `locale::apply` in `crates/kadakas-app/src/locale.rs` sets the language of the process and the current units. It runs after each change of the preferences.

The active language is the same for the whole process. A worker thread therefore makes messages in the language of the window. The function `with_language("et", || ...)` changes the language for one thread only. Tests use it. The language is not document data and no document stores it.

## Units and formats

`kadakas_units::Units` holds one choice of everything:

| Setting | Choices |
|---|---|
| Length | mm, cm, m, in, ft, ft-in. Inch fractions are 1/2 to 1/64, or decimal inches. |
| Area | m², ha, ft², ac. Volumes follow the length system: m³ or yd³. |
| Angle | decimal degrees (`45.5°`), degrees-minutes-seconds (`45°30'00"`), bearing (`N 45°30'00" E`) |
| Decimal separator | point or comma |
| Digit grouping | none, comma, point, space |
| Date | `2026-10-11`, `11.10.2026`, `10/11/2026` |

Settings that the person does not choose follow the language. Estonian gives `1 234,5` and `11.10.2026`. The en-US language gives `1,234.5` and `10/11/2026`. The struct `Settings` is what the preferences keep. The call `Settings::resolve(language)` makes the `Units`. The current units of the process are `kadakas_units::current()`.

The parser reads the active format. It also accepts the other format, but the meaning must be clear:

- `1,5` and `1.5` both mean one and a half, unless grouping gives the symbol another meaning.
- `1 234,5`, `1.234,5` and `1,234.5` all read as 1234.5. When both symbols appear, the last one is the decimal separator.
- Lengths can be `5' 6 1/2"`, `5'-6"`, `5ft 6in`, `6.5"`, `1,5 m` or `50cm`. A number without a unit counts in the active unit. For feet and inches it counts in inches.
- Angles can be `45,5`, `45°30'`, `-10°15'36"`, `N 45°30' E` or `s80w`.
- Dates use the order of the active format. The form `yyyy-mm-dd` always works.
- Numbers with an exponent (`1e3`) work. A group of digits has no leading zero, so `0,500` is one half and not 500.
- Feet and inches go from large to small and use whole feet. The text `5' -6"`, `6" 5'` and `5 1/2'` is refused.

A number such as `1,500` is a problem in a format whose group symbol is the comma (en-US). It can mean 1500 or 1.5. The Data bar does not guess. It gives no value and shows a message that names the decimal separator. The number fields in the panels show the value that they read, so the person sees it at once.

When a number field loses focus, egui reads its text again. The text has rounded numbers, so this could change the value. Each field therefore remembers the exact value of the text that it showed (`Memo` in `units_ui.rs`). If the text is unchanged, the value is unchanged. A length that is not on the grid of fractions shows as decimal inches (`39.3701"`).

Paper values are not model lengths. A line weight, a text height on paper and a symbol size on paper always show in mm (`.paper_mm()`).

A bearing is for directions only. The methods `.direction()` (rotations, the direction of a fall) and the angle of the Data bar follow the bearing format. If the bearing is active, all other angles show as degrees-minutes-seconds. These are a sweep, a step, a pitch and a sprinkler arc of 360 degrees (`.degrees()`, `Units::plain_angle`).

The decimal separator and the group symbol cannot be the same symbol. `Settings::resolve` uses a space for the groups in that case. Preferences shows the conflicting choice in grey.

The tests include property tests. What the formatter writes, the parser reads back. See `crates/kadakas-units/tests/roundtrip.rs`.

Some texts do not follow these settings. The units of a document, the unit of a dimension style and the text of a drawing follow the document. A dimension text in a drawing follows the style of the document (`annot/dim.rs` in `kadakas-core`). The shared rounding code is in `kadakas-units` (`format_drawing`).

## Where the units show

- The Data bar: fields, hints and typed values.
- The cursor position and the tool readouts.
- Object Info: positions, sizes, areas, volumes, angles and record fields.
- The number fields of all tool option panels.
- The Preferences example, the schedule prices, the status messages that name quantities and the date of the sun.

## Not done yet

- The process language and the current units (`ACTIVE` in `kadakas-l10n`, `CURRENT` in `kadakas-units`) are the only global state that PLAT-12 allows for the UI. They are preferences of the person. A host with many documents uses `kadakas-api` and never sets them.
- The launcher (`kadakas-launcher`) has its own English texts. It is a separate program and has no `tr!` calls yet.
- The hydraulic units of the irrigation tools (kPa, l/min, m/s) are metric only. Only the number format follows the language.
- Paper sizes (A4, Letter) and the "paper mm" of annotation fields are always millimeters of paper.
- Month names and times are not localized. Dates are numbers.
- The docs in `docs/` are English.
