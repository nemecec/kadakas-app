# Icons

Kadakas has its own icon set (feature UI-12). The files are in `assets/icons/`. The gallery is
`docs/icons.html` (made by `python3 scripts/icons_gallery.py`).

## Legal rules

- Do not download, open, view, trace or copy any Vectorworks icon or screenshot.
- Draw each tool icon from a written idea only (for example "a tree crown seen from above"), in our own style.
- Generic action icons come from Tabler Icons (MIT). They are in `assets/icons/tabler/` with the `LICENSE` file. The attribution is in `THIRD_PARTY_NOTICES.txt` (the fixed section in `docs/notices-extra.txt`).
- Do not put "Vectorworks" in a file name. A test checks this.

## Style rules

- 24 x 24 `viewBox`, stroke `currentColor`, stroke width 2 (design for 16 px first), round caps and joins.
- No fills, except small accents: dots (`fill="currentColor"`) and light areas (`fill-opacity` 0.5 or less, `stroke="none"`).
- No colors, no raster images, no text elements. A file must be smaller than 4 KB.
- Keep the drawing inside the 3 to 21 range. A test checks the bounding box. Fill accents are 0.8 or less. The result area of a surface icon is 0.5 or more.
- Tabler files also use stroke width 2, so the whole set matches.

## Names and ideas

One file for each icon, in `assets/icons/kadakas/`. Names are lower case with hyphens.

| Group | Icons (visual idea) |
|---|---|
| Draw | select (arrow pointer), lasso (loop with a tail), line (diagonal with end dots), rectangle, circle (center cross), arc (three points), polyline (zigzag with dots), polygon, text (A with baseline) |
| Annotate | dimension, callout (bubble with leader), data-tag, north-arrow, scale-bar, revision-cloud |
| Landscape | plant (crown with spokes), existing-tree (dashed crown, trunk dot), landscape-area (outline with plant dots), hardscape (pavers), site-model (contours), site-modifier (pad on contours), grade (slope arrow, percent), landscape-wall (coping), spot-elevation, section-line, irrigation-outlet (spray fan), irrigation-line (pipe with joints), massing (block) |
| Model / 3D | extrude (a prism), sweep (a profile turned around a dashed axis), polygon3d (a polygon with a drop line and corner dots) |
| Document | viewport, sheet-layer, design-layer, class, symbol, group, hatch, worksheet, record, versions (a clock face with an arrow that turns back), issue-pin (a drop-shaped map pin with a dot), issues (a speech bubble with two lines of text) |
| View | view-3d, flyover, top-plan, render-shaded, render-wireframe, render-hidden-line, render-dashed, smart-cursor |
| Surface | surface-add, surface-subtract, surface-intersect, surface-clip, compose |
| Modify | offset, fillet, chamfer, trim, extend, split, mirror, rotate, scale, array |
| Files | dxf, ifc |

Tabler icons that we use (by our idea): file-plus (new), folder-open, device-floppy (save), file-import, file-export, file-type-pdf (PDF), file-type-svg, arrow-back-up (undo), arrow-forward-up (redo), trash, select-all, search, settings, zoom-scan, grid-dots, map (GIS), world-www (WMS layer), world-latitude (CRS), code (script), plug (plug-in object), terminal-2 (Python console), magnet (snapping set), arrow-narrow-left and arrow-narrow-right (view history).

## How the program uses icons

`crates/kadakas-app/src/icons.rs` includes all files with `include_bytes!`. It draws each file with resvg. The size comes from the screen (points times the pixel ratio). It draws it in white. The widget tints it with the text color, so hover, selection and theme need no new texture. It caches the texture for each name and size. It frees the textures when the pixel ratio changes. If an icon is missing, the button shows the text label only.

Icons are in the tool palette (with the name and key as tooltip), the view bar (views and render modes), the menus, the selection mode buttons and the Resource Manager (types and rows).

## How to add an icon

1. Draw `assets/icons/kadakas/<name>.svg` by the style rules. Start from a written idea.
2. Add one line to `ICONS` in `crates/kadakas-app/src/icons.rs`.
3. For a tool, add the name to `tool_icon`. For a menu item, add it to `menu_icon`. Both are exhaustive `match` expressions. The compiler tells you when a new tool or menu item has no icon.
4. Run `cargo test -p kadakas-app icons`. The tests check four things. Every file parses. Every file follows the style. Every file is in the table. Every tool has an icon. The test `golden_icon_sheet_is_not_blank` writes `kadakas-icon-sheet.png` to the folder in `ICON_SHEET_DIR` (default: the temp folder). Look at it. Do not commit it.
5. Run `python3 scripts/icons_gallery.py` and commit `docs/icons.html`.
6. For a Tabler icon: copy the file from the `@tabler/icons` npm package (`npm pack @tabler/icons`, folder `icons/outline`) into `assets/icons/tabler/`. The `LICENSE` file stays there.
