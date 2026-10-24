# Real-world DXF: what Vectorworks writes and reads (IOP-15)

Scope: public sources only. We did not use Vectorworks, AutoCAD or SketchUp. The research step downloaded nothing. The tests of IOP-15 fetch the public sample files of section 8 at run time (see `docs/interop.md`). Every claim has a source. A claim marked unverified rests on one forum post or on a search excerpt. We did not open that source in full.

Pages of app-help.vectorworks.net are public help pages.

Several forum pages (forum.vectorworks.net, controlbooth.com) returned 403 to our fetch. We saw only search excerpts of them. We mark those items unverified.

## 1. Releases and file kinds

| Claim | Source |
|---|---|
| Vectorworks 2026 imports DXF/DWG 2.5 through 2026 and exports 12 through 2026. | https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_formats.htm |
| The 2022 guide gives the same range with 2022 as the upper end. | https://app-help.vectorworks.net/2022/eng/VW2022_Guide/LandingPage/..\DXFDWG\DXF_DWG_and_DWF_file_formats.htm (search excerpt) |
| Export offers three kinds: text DXF, binary DXF and DWG. Text DXF is called the universally supported choice. | https://app-help.vectorworks.net/2024/eng/VW2024_Guide/DXFDWG/DXF_DWG_and_DWF_export_options.htm |
| Binary and text DXF share the `.dxf` extension. A reader can think a binary file is corrupt. Vectorworks does not support `.dxb`. | same page |
| The help says to export the lower of two versions: the highest version the recipient reads, or the default version. The default version number is not named on the page. | same page |
| Import reads DWG, text DXF and binary DXF. | https://app-help.vectorworks.net/2023/eng/VW2023_Guide/DXFDWG/DXF_DWG_and_DWF_file_import.htm |
| A user was unable to open a Vectorworks DXF in LightBurn because it was binary. LightBurn reads text DXF only. Unverified, one post. | https://forum.lightburnsoftware.com/t/vectorworks-dxf-file-problem/136817 |
| The help says some recent AutoCAD versions fail to read certain DXF files, for example they lose image links. DWG is the safer choice. | https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_formats.htm |

The pages partly show the default release and the release that each toggle needs. No page that we read lists the exact selectable versions (R12, 2000, 2004, 2007, 2010, 2013, 2018). So the claim "Vectorworks writes release X" is unverified for 2007 and later. The help names these thresholds. Version 13 covers multiline text. Version 14 covers hatches, images and solid hatch. Version 2000 covers units, wipeouts and Unicode names. Version 2004 covers true colour.

## 2. Entities

The export facts come from the 2026 export page and the 2024 options page (https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_export.htm and https://app-help.vectorworks.net/2024/eng/VW2024_Guide/DXFDWG/DXF_DWG_and_DWF_export_options.htm):

- Symbols become named blocks. Groups become anonymous blocks, or named blocks by default. Plug-in objects and layer links get generic names such as "Group-2".
- Linked text in symbols becomes block attributes.
- Solid fills become solid HATCH entities (version 14 and later) or unassociative WIPEOUT (version 2000 and later).
- Hatches become associative hatches (version 14 and later) or anonymous blocks (13 and lower). Associativity needs `.pat` files. The export writes them next to the DXF file.
- Tile fills become clipped blocks. Image fills become clipped images.
- Raster images are saved as JPEG next to the drawing. The DXF has an image object with the file name and insertion point. Version 14 and later support this.
- When the option is on, solids and NURBS surfaces export as ACIS solids. Walls, slabs, filled 3D polygons, meshes and NURBS curves cannot become ACIS.
- Meshes export as DXF meshes. The page does not say MESH or POLYFACE. Unverified which one.
- When filled, 3D polygons export as triangles. Otherwise, they export as 3D polygons. The option "Triangulate to preserve fills" splits big faces.
- NURBS curves export as SPLINE.
- Walls export as triangulated 3D polylines in 3D views and as lines and arcs in 2D views.
- The option "Approximate polylines with spline vertices using arcs" turns Bezier and cubic splines into polylines with arcs.
- Complex line types export as anonymous blocks, or as true line types with a `.shx` shape file. Simple types always export as true line types.
- Text and dimensions: the export page does not say whether it writes TEXT or MTEXT. Version 13 and later support styled multiline text. Older versions split wrapped text into lines. Source: file formats page above. The entity names TEXT, MTEXT and DIMENSION are our reading, not a quote. Unverified.
- The page does not name LWPOLYLINE. Whether Vectorworks writes LWPOLYLINE or POLYLINE is not public. Unverified. Do not assume either.

Import, from the import page (https://app-help.vectorworks.net/2023/eng/VW2023_Guide/DXFDWG/DXF_DWG_and_DWF_file_import.htm):

- Points become symbols or loci. Multilines become grouped lines or walls.
- Hatch definitions import as hatch definitions. A single associative hatch without islands becomes an object with a fill. A non-associative or multi-object hatch becomes zero-weight objects with a fill. A hatch with islands becomes a polyline with holes.
- Referenced images become images. Wipeouts become polygons with a background colour fill.
- AutoCAD Architecture objects become grouped lines or meshes. Civil 3D objects become groups.
- These do not import: named views, lights, rtext, SHX fonts, OLE objects, UCS and DXF groups. Same page.
- Proxy objects are not discussed on the 2023 page. The 2017 page says industry objects without saved proxy graphics can fail (search excerpt, https://app-help.vectorworks.net/2017/eng/VW2017_Guide/DXFDWG/DXF_DWG_and_DWF_Items_Which_Cannot_Import_to_Vectorworks.htm).
- Block clipping: ignore it, or import as cropped viewports or layer links (import options, https://app-help.vectorworks.net/2025/eng/VW2025_Guide/DXFDWG/DXF_DWG_and_DWF_import_options.htm).
- Rays and xlines can become finite lines in a Guides class. Same page.
- Dimension styles import as custom dimension standards. Same page and file formats page.
- A single-file import binds xrefs. A missing xref is reported. A multi-file import does not do that. Import page above.

Not exported at all (https://app-help.vectorworks.net/2023/eng/VW2023_Guide/DXFDWG/Items_which_cannot_export_to_DXF_DWG_or_DWF.htm): textures, lights, image props, individual layer scales, layer graying.

Not exported either: perspective design layers, oblique projections, object names and record data on arbitrary objects, transfer modes other than Paint, background images.

## 3. Units and scale

- Only version 2000 and later carry units. A unitless file needs the sender to confirm the unit. Vectorworks offers five unit guesses. Source: file formats page (2026).
- By default, import uses the unit found in the file. The user can force a unit or a ratio. A second option changes the document unit without resizing objects. Source: import options (2025).
- When the file unit and the document unit differ, dimensions look wrong. The help warns of this. Its example: a metre file in a feet and inches document shows a 1 m dimension as 3'3.37". Source: import options (2025, search excerpt).
- Model space scale ("fit to page" or a set value) controls dash length and polyline width to line weight. It does not resize geometry. Source: import options (2025).
- Export: with sheet layers, the referenced design layers go to model space in top view with no scale. Without sheet layers, each design layer's scale goes to model space as one scale. The option "Rescale layers to" gives mixed scales one common scale. Source: export page (2026) and options (2024).
- The pages do not name `$INSUNITS`. That a Vectorworks file sets it is likely and unverified.
- A user got a 375 x 347 mm size where 210 x 194 mm was expected. This happened with a Vectorworks DXF opened in LightBurn. The cause was not found. Unverified, one post (LightBurn link above).
- DXF without `$INSUNITS` has no unit. Importers guess. Source: Onshape forum summary (search excerpt, unverified). The field `$INSUNITS` is in the Autodesk DXF reference: https://help.autodesk.com/view/OARX/2024/ENU/?guid=GUID-A85E8E67-27CD-4C59-BE61-4DC9FADBF74E

## 4. Layers, classes, line types, weights, colours

- Import: layers become classes by default. Other choices are the active class or the active design layer. A prefix and a mapping set are available. Source: import options (2025).
- Export: classes become DXF layers (recommended). Layers become DXF layers as the other choice. Invisible classes go to invisible layers or are dropped by option. Fills and hatches can go to a separate layer with a suffix such as `_Hatch`. Source: options (2024).
- With sheet layers, design layers become blocks named like "Foundation (2D)" and "Foundation (3D)". Their layers start with an underscore and viewports freeze or thaw them. Source: export page (2026).
- Mapping sets are `.xml` or `.txt` files. A mapping file has a header line with a tab. Pairs are separated by a tab. Source: https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/Mapping_DXFDWG_layer_and_class_names.htm
- Names: version 2000 and later keep case, spaces and Unicode. Illegal characters are the exception. They change. Earlier versions go upper case and use underscores. Attribute tags never take spaces. Source: file formats page (2026).
- Line weights: export converts to the nearest DXF weight. Examples: 0.38 to 0.45 mm gives 0.40 mm and 2.06 to 6.48 mm gives 2.11 mm. A weight of 0 is deleted. Version 14 and earlier have no true line weights. Source: options (2024) and file formats page.
- Colours: the user picks true colours (RGB, group code 420) or indexed colours (ACI). Indexed colours allow a `.ctb` file with "map line weights to colors". Versions before 2004 use the fixed palette. Source: options (2024).
- If a `.ctb` file is present, import maps colours to line weights with it. Source: import options (2025).
- Pattern fills export as plain solid colours in some cases. Source: file formats page.
- Layer transfer modes other than Paint are lost.

## 5. Code pages and Unicode

- Vectorworks pages say names keep Unicode in version 2000 and later. They do not describe the code page of DXF text. Unverified for text. Source: file formats page (2026).
- ezdxf: release R2004 (`AC1018`) and older are ASCII in the code page of `$DWGCODEPAGE`, default `ANSI_1252`. R2007 (`AC1021`) and newer are UTF-8. The escape `\U+nnnn` is used for characters outside the code page. Source: https://ezdxf.readthedocs.io/en/stable/dxfinternals/fileencoding.html
- The export option "Preserve mapped font on export" writes the replacement font. Otherwise it writes the original. Source: options (2024). SHX fonts do not import.

## 6. Paper space and model space

- Import: each paper space layout becomes a sheet layer. Viewports keep scale, view and projection. Model space becomes design layers. Source: import page (2023).
- Export: sheet layers become paper space layouts. Section viewports export as blocks. Annotations become paper space objects above the viewports. Source: export page (2026).
- AutoCAD wants paper space objects fully above or fully below viewports. Objects under a viewport in Vectorworks end up above it. Source: export page (2026).
- By default, design layer viewports export as referenced DWG files. Alternatively, they are bound into the master file. Then class and layer overrides can be lost. Source: options (2024).
- The option "export viewports as 2D graphics in model space" flattens them. Source: options (2024).

## 7. Reported problems and workarounds

| Problem | Workaround | Source and status |
|---|---|---|
| Binary DXF is not read by some tools. | Export text DXF or DWG. | https://forum.lightburnsoftware.com/t/vectorworks-dxf-file-problem/136817 and the Vectorworks help. Verified by the help. |
| Image links are lost in some AutoCAD versions with DXF. | Use DWG. | File formats page (2026). Verified. |
| If fills are lost in old releases, unwanted lines appear after export. | Triangulate option or newer release. Check the file in a second program. | Search excerpt of the 2022 export procedure page. Unverified. |
| A contour broke on R12 export from Vectorworks 2019. | Export a newer DWG, then convert to R12 with DWG TrueView, the ODA converter or DraftSight. | https://forum.vectorworks.net/topic/62391-dxf-r12-export-problems/ (search excerpt, unverified). |
| 8,400 duplicate objects and many short segments in a laser cutting export. | Join paths and reduce nodes in a vector editor. | LightBurn thread, unverified. |
| Wrong overall size in the receiving tool. | Rescale, or set the unit on import. | LightBurn thread, unverified. |
| Wipeouts become polygons with smoothness set by the 2D conversion resolution. | Raise the resolution preference. | File formats page (2026). Verified. |
| Named blocks come back as symbols and groups as anonymous blocks. | Choose anonymous blocks on export. | File formats page. Verified. |
| Splines fail in laser tools and CAM programs. Common elsewhere. | Convert splines to polylines. | Several non-Vectorworks forums (search excerpt). Unverified for Vectorworks. |

## 8. Public sample files with a clear licence

Checked on 2026-10-23 with the GitHub API. The tests fetch the files from a fixed commit and keep them out of the repository.

| Corpus | URL | Licence | What it covers |
|---|---|---|---|
| ezdxf example DXF files | https://github.com/mozman/ezdxf/tree/master/examples_dxf | MIT (repo `LICENSE`) | `Minimal_DXF_AC1006/AC1009/AC1021`, `3dface`, `colors`, `valid_lineweights`, hatches (several), `hatch_issues`, MTEXT columns R2000/R2007/R2018, text alignments and fonts, nested block inserts, attribs, dimensions in blocks, wipeouts, `xclip`, viewports and frozen layers, `caret_encoding`, `uncommon`, `transparency` |
| ezdxf integration test data | https://github.com/mozman/ezdxf/tree/master/integration_tests/data | MIT | binary DXF R12 and R2000, other regression files |
| GDAL OGR DXF test data | https://github.com/OSGeo/gdal/tree/master/autotest/ogr/data/dxf | GDAL licence (MIT style, see `LICENSE.TXT`). Two files are copies from ezdxf and are MIT. Check each file before reuse. | 55 files: `3d`, `3dface`, `bin_dxf_r12`, `bin_dxf_r2000`, `BINARY_wipeout`, `LWPOLYLINE-OCS`, OCS to WCS, `closed_polyline_with_bulge`, hatch variants, `spline_*`, `dimension*`, `leader-mleader`, `insert-recursive-pair`, `block-*`, `byblock-bylayer`, `frozen-off`, `linetypes`, `text-fancy`, `utf-8`, `transparency`, `wipeout`, fuzzer cases |
| LibreCAD and QCAD samples | https://github.com/LibreCAD/LibreCAD | LibreCAD files are GPL v2. We do not copy them into the repository. A test can download them at run time into a cache folder that git ignores. | Block library and samples |

We found no public Vectorworks written DXF with a clear licence. Our test corpus must be files that we write ourselves from the specification. A Vectorworks file posted by a user in a forum has no licence and stays out.

Other sources seen and not used: Burkardt DXF examples (https://people.math.sc.edu/Burkardt/data/dxf/dxf.html) say GNU LGPL, dated 2003, old R12 style.

## 9. Our code before and after IOP-15 and IOP-16

From `docs/interop.md`, rows IOP-01, IOP-09, IOP-15 and IOP-16 of `docs/feature-spec.md`, and `crates/kadakas-io/src/dxf`.

Before this work (state of the research):

- Export: text DXF, release 2004 (`AC1018`), `$INSUNITS`, `$DWGCODEPAGE`, true colour (420), `\U+` escapes, blocks, GEODATA, POLYFACE, 3DFACE and MESH.
- Import: LINE, LWPOLYLINE, POLYLINE (2D and 3D), ARC, CIRCLE, ELLIPSE, SPLINE, TEXT, MTEXT, POINT, 3DFACE, HATCH and INSERT.
- Binary DXF was refused with a message.
- Paper space entities were skipped and counted.
- A tilted extrusion direction was skipped.
- ATTRIB, ATTDEF and VIEWPORT were skipped.
- `$INSUNITS` and `$DWGCODEPAGE` were read.
- Text was UTF-8 when valid. Otherwise it used the code page, or else Latin-1.
- The import report counted skipped entities by kind.

After this work:

- Import reads binary DXF and the releases 12 to 2018. The report names the release.
- A file without a unit gets a guess (inches if `$MEASUREMENT` is 0, else millimetres). The report says so.
- The export has the option `version` for the releases 12, 2000, 2004, 2013 and 2018.
- WIPEOUT, SOLID, TRACE, MESH, LEADER, ATTRIB and DIMENSION blocks are imported. IMAGE, ACIS solids and multileaders are reported by name.
- Circles, arcs, polylines and ellipses in a tilted plane become 3D polylines.
- A paper space layout becomes a sheet layer, and a sheet layer becomes a layout.
- The report accounts for every entity: `entities_read` equals `entities_imported` plus `entities_skipped`.
- The corpus of public files and our own files test all of it. See `docs/interop.md`.

## 10. Gaps, ranked, with the state of each

Rank = chance that a real Vectorworks or AutoCAD file hits it, times the harm. Each item has its test and its state.

1. Binary DXF import was refused. Vectorworks can write binary DXF, and users can do so by mistake.
   - Test: convert `bin_dxf_r12.dxf` and `bin_dxf_r2000.dxf` (GDAL data, MIT) and check that import reads them.
   - Until then, the refusal text must name "save as text DXF".
   - Done. The reader handles one-byte and two-byte codes. Six public binary files and our own two give the objects of the ezdxf reader. A damaged file is refused with the byte.
2. Releases R12 to 2000 to 2007 to 2018 were only known from the ezdxf rules.
   - Test: read `Minimal_DXF_AC1006`, `AC1009`, `AC1021` and the MTEXT R2018 file from ezdxf.
   - The result must have the same entity counts as the ezdxf reader.
   - Done. The counts of 118 public files equal those of the ezdxf reader.
3. Entities that are likely in a Vectorworks export and were not imported: 3DSOLID and BODY (ACIS), MESH, DIMENSION, IMAGE, WIPEOUT, LEADER, MLEADER, SOLID, TRACE, XLINE and RAY.
   - Test: one file with each entity (GDAL data: `wipeout`, `dimension`, `leader-mleader`, `solid`).
   - Each must give a report line with the entity name, never a silent loss.
   - Add MESH import (ezdxf can write one).
   - Done. MESH, SOLID, TRACE, WIPEOUT, LEADER and the dimension blocks are imported. IMAGE (with its file), ACIS solids, MLEADER, XLINE and RAY are reported by name.
4. Wipeout and solid hatch are the way Vectorworks writes fills. We read HATCH and skipped WIPEOUT.
   - Test: import `hatches_1.dxf`, `hatches_2.dxf` and `hatch_as_multipolygon.dxf`. Check the filled areas against the expected area.
   - Add a test where a wipeout covers a hatch.
   - Done. A solid hatch is filled, and an island is a white filled outline. The areas equal the ezdxf reader. A wipeout is a white polygon above the hatch.
5. Unitless files and wrong units. A file without `$INSUNITS` (R12 files, AC1006 and AC1009) needs a clear default and an option.
   - Test: import a file without a unit with and without `unit: mm`, and check the box size in both cases.
   - Test: import a metre file as feet.
   - Done. The default is millimetres, or inches when `$MEASUREMENT` is 0. The report gives the reason. `Minimal_DXF_AC1009.dxf` has no entities, so our own files `r12_unitless.dxf` and `r2018_meters.dxf` carry the size checks.
6. Tilted extrusion (OCS) was refused. Real files have 3D rotated blocks and arcs from other programs.
   - Test: GDAL `ocs2wcs1.dxf`, `ocs2wcs2.dxf`, `ocs2wcs3.dxf`, `LWPOLYLINE-OCS.dxf` and `ellipse_z_extrusion_minus_1.dxf`.
   - Import must place geometry at the right 3D position.
   - Done for circles, arcs, polylines, SOLID, ELLIPSE and the paths of a HATCH, with the arbitrary axis algorithm. IOP-27 added the block reference (placed in 3D, checked against `virtual_entities` of ezdxf) and TEXT, ATTRIB and MTEXT (placed flat by their projection).
7. Paper space and viewports were skipped. Vectorworks puts sheet layers in paper space and the plan in model space, so the model space data survived. Annotations and title blocks were lost.
   - Test: `VP4.dxf` and `viewport_frozen_layers.dxf` (ezdxf).
   - The report must say how many entities were in paper space and the layout names.
   - Test frozen layers: the layer must stay hidden.
   - Done (IOP-16). A layout is a sheet layer with its viewports. The entity counts of the sheets equal the ezdxf reader. A frozen layer stays hidden in its viewport.
8. Block handling: anonymous blocks (`*U12`), nested inserts, rotated and scaled inserts, MINSERT, block base point, ATTRIB text and `xclip`.
   - Test: `insert-recursive-pair.dxf`, `block-basepoint.dxf`, `block-insert-order.dxf`, `multi_insert_with_attribs.dxf` and `xclip.dxf`.
   - Recursion must stop with a report line.
   - Insert counts must match ezdxf.
   - Done (IOP-27 added the block clip). The geometry of a clipped reference is cut by the polygon, with the inverted clips and the arrays. The cut length of `xclip.dxf` equals the length that shapely finds. The INSERT counts equal the ezdxf reader. The recursion stops with the line `blocks are nested too deep`.
9. Colours and weights: ACI (including 0 ByBlock and 256 ByLayer), true colour, layer colour, and the lineweight table of Vectorworks (nearest value).
   - Test: `colors.dxf`, `valid_lineweights.dxf`, `byblock-bylayer.dxf` and `transparency.dxf`.
   - Check the colour per entity and the 0.40 mm mapping.
   - Done. The palette is the table of ezdxf (1 to 248), so every index equals the ezdxf colour. The weight 40 gives 0.40 mm. A transparency is not imported, and a note counts it.
10. Text: MTEXT inline codes, `%%c` `%%d` `%%p` codes, `\U+` escapes, columns, and fonts that are SHX.
    - Test: `mtext_complex_inline_codes.dxf`, `caret_encoding.dxf`, `text_fonts.dxf`, GDAL `utf-8.dxf`, and a CP1250 and CP1251 file made with ezdxf.
    - Check that the text equals the expected Unicode string.
    - Done. The plain text of 121 public files equals the plain text of the ezdxf reader. The code pages work for ANSI, DOS and ISO names. Columns are separate MTEXT entities, and empty ones are reported.
11. Export version: we wrote only `AC1018`. Some tools (old LibreCAD, laser software) read R12 or R2000 only.
    - Test: export R12 and R2000 variants, read them with the ezdxf audit, and open them in LibreCAD by hand.
    - Done for the ezdxf audit in AC1009, AC1015, AC1018, AC1027 and AC1032. The check in LibreCAD by hand is not done.
12. Splines and ellipses become polylines. This is right for most uses and loses exactness.
    - Test: `spline_closed.dxf`, `spline_weight.dxf`, `spline_qcad.dxf` and `spline_with_very_close_neg_to_zero_knot.dxf`.
    - Check that the deviation is under the tolerance and that a closed spline stays closed.
    - Done. The cut is now adaptive within the tolerance. The curves stay within 0.05 of the ezdxf curve at that tolerance. A note counts splines and ellipses. Missing knots get uniform knots.
13. Xref and external references. Vectorworks binds xrefs on import. We do not read them.
    - Test: a file with an XREF block. The report must name the missing file.
    - Done. The report line has the block name and the path.
14. Hostile and damaged files. Real-world files have errors.
    - Test: both GDAL `clusterfuzz-*` files and `insert-too-many-errors.dxf`.
    - No panic, a bounded run time and a report.
    - Done. These files, huge counts, nesting without end, long lines and damaged binary files have tests.

## 11. Next steps

1. Check the exports of the releases 12, 2013 and 2018 by hand in a CAD program or a laser program.
2. Check the sign of the viewport twist and the pattern hatches by hand in a CAD program. The tests follow the ezdxf reader.
3. When a Vectorworks forum page becomes readable, re-check this note. The unverified items in sections 2, 5 and 7 are the first to confirm.

## 12. The leftovers of IOP-15 (IOP-27)

Public sources only. The ezdxf source is MIT licensed. Nothing of a CAD program was used.

| Topic | Source | What we took |
|---|---|---|
| HATCH pattern data | Autodesk DXF reference (HATCH group codes 52, 41, 77, 78, 53, 43 to 46, 79, 49) | The pattern lines are stored after the scale and the turn. The offset is a vector in the world frame. |
| Pattern names and lines | `ezdxf/tools/pattern.py` (metric table) | The table of 106 standard patterns in `std_patterns.txt`, and the names `ANSI31` to `ANSI38`, `AR-*`, `EARTH`, `GRASS`, `GRAVEL`, `STEEL`. |
| Drawing a pattern | `ezdxf/render/hatching.py` | An independent drawing of the pattern lines. Our line length is compared with it for nine patterns. |
| Block clip | `ezdxf/xclip.py` and `ezdxf/entities/spatial_filter.py` | The clip polygon in the space of the reference, the inverse matrix (the last 24 numbers of code 40), the switch (code 71), the inverted part in an XRECORD. |
| Viewport twist | `ezdxf/entities/viewport.py` (`get_transformation_matrix`) | A positive twist turns the model counterclockwise on the sheet. The view center is in the display axes. |
| Tilted plane | Autodesk DXF reference (arbitrary axis algorithm), `Insert.virtual_entities` of ezdxf | The place of a block reference in 3D. |

Decisions:

1. Pattern hatches become hatch fills of the document. The pattern lines in the entity are exact, so the name is only a name. A table is for entities without lines and for the names on export.
2. A polyline has no holes. An island of a pattern hatch is a white outline on top, as for a solid hatch. The export does not turn it back into a hole.
3. A block is exploded, so a clip cuts geometry. Lines are exact at the edge. Curves become chords. A clip that lies on the edge keeps the geometry.
4. The sheet has a twist for plan views. The 3D camera has no roll, so a 3D view only keeps the number. A picture cannot follow a turn. It is not drawn, and a note says so.
5. A perspective view works for a 3D viewport through the camera of Kadakas. The lens length is not read.
6. A text has no work plane. It is placed flat by its projection. A block reference in a tilted plane is placed in 3D.

Open: the sign of the twist is taken from the ezdxf reader. A check in a CAD program by hand is not done.
