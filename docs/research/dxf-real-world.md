# Real-world DXF: what Vectorworks writes and reads (IOP-15)

Scope: public sources only. Nothing was downloaded or run. We did not use Vectorworks, AutoCAD or SketchUp. Every claim has a source. A claim marked unverified rests on one forum post or a search excerpt that we could not open in full. Pages of app-help.vectorworks.net are public help pages.

Several forum pages (forum.vectorworks.net, controlbooth.com) returned 403 to our fetch. We saw only search excerpts of them. Those items are marked unverified.

## 1. Releases and file kinds

| Claim | Source |
|---|---|
| Vectorworks 2026 imports DXF/DWG 2.5 through 2026 and exports 12 through 2026. | https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_formats.htm |
| The 2022 guide gives the same range with 2022 as the upper end. | https://app-help.vectorworks.net/2022/eng/VW2022_Guide/LandingPage/..\DXFDWG\DXF_DWG_and_DWF_file_formats.htm (search excerpt) |
| Export offers three kinds: text DXF, binary DXF and DWG. Text DXF is called the universally supported choice. | https://app-help.vectorworks.net/2024/eng/VW2024_Guide/DXFDWG/DXF_DWG_and_DWF_export_options.htm |
| Binary and text DXF share the `.dxf` extension. A reader may think a binary file is corrupt. Vectorworks does not support `.dxb`. | same page |
| The help says to export the highest version the recipient reads, or the default version, whichever is lower. The default version number is not named on the page. | same page |
| Import reads DWG, text DXF and binary DXF. | https://app-help.vectorworks.net/2023/eng/VW2023_Guide/DXFDWG/DXF_DWG_and_DWF_file_import.htm |
| A user could not open a Vectorworks DXF in LightBurn because it was binary. LightBurn reads text DXF only. Unverified, one post. | https://forum.lightburnsoftware.com/t/vectorworks-dxf-file-problem/136817 |
| The help says some recent AutoCAD versions fail to read certain DXF files, for example they lose image links. DWG is the safer choice. | https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_formats.htm |

Which release is the default and which release each toggle needs is partly shown on the pages. The exact list of selectable versions (R12, 2000, 2004, 2007, 2010, 2013, 2018) is not on any page we could read. So the claim "Vectorworks writes release X" is unverified for 2007 and later. The help names these thresholds: version 13 (multiline text), 14 (hatches, images, solid hatch), 2000 (units, wipeouts, Unicode names) and 2004 (true colour).

## 2. Entities

Export, from the 2026 export page and the 2024 options page (https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_export.htm and https://app-help.vectorworks.net/2024/eng/VW2024_Guide/DXFDWG/DXF_DWG_and_DWF_export_options.htm):

- Symbols become named blocks. Groups become anonymous blocks, or named blocks by default. Plug-in objects and layer links get generic names such as "Group-2".
- Linked text in symbols becomes block attributes.
- Solid fills become solid HATCH entities (version 14 and later) or unassociative WIPEOUT (version 2000 and later).
- Hatches become associative hatches (version 14 and later) or anonymous blocks (13 and lower). Associativity needs `.pat` files that the export writes next to the drawing.
- Tile fills become clipped blocks. Image fills become clipped images.
- Raster images are saved as JPEG next to the drawing. The DXF has an image object with the file name and insertion point. This works for version 14 and later.
- Solids and NURBS surfaces export as ACIS solids when the option is on. Walls, slabs, filled 3D polygons, meshes and NURBS curves cannot become ACIS.
- Meshes export as DXF meshes. The page does not say MESH or POLYFACE. Unverified which one.
- 3D polygons export as 3D polygons, or as triangles when filled. The option "Triangulate to preserve fills" splits big faces.
- NURBS curves export as SPLINE.
- Walls export as triangulated 3D polylines in 3D views and as lines and arcs in 2D views.
- The option "Approximate polylines with spline vertices using arcs" turns Bezier and cubic splines into polylines with arcs.
- Complex line types export as anonymous blocks, or as true line types with a `.shx` shape file. Simple types always export as true line types.
- Text and dimensions: the export page is silent on whether it writes TEXT or MTEXT. Version 13 and later support styled multiline text. Older versions split wrapped text into lines. Source: file formats page above. The entity names TEXT, MTEXT and DIMENSION are our reading, not a quote. Unverified.
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
- A single-file import binds xrefs. A missing xref is reported. Multi-file import does not do that. Import page above.

Not exported at all (https://app-help.vectorworks.net/2023/eng/VW2023_Guide/DXFDWG/Items_which_cannot_export_to_DXF_DWG_or_DWF.htm): textures, lights, image props, individual layer scales, layer graying, perspective design layers, oblique projections, object names and record data on arbitrary objects, transfer modes other than Paint, background images.

## 3. Units and scale

- Only version 2000 and later carry units. A unitless file needs the sender to confirm the unit. Vectorworks offers five unit guesses. Source: file formats page (2026).
- Import uses the unit found in the file by default. The user can force a unit or a ratio. A second option changes the document unit without resizing objects. Source: import options (2025).
- The help warns that dimensions look wrong when file unit and document unit differ. Its example: a metre file in a feet and inches document shows a 1 m dimension as 3'3.37". Source: import options (2025, search excerpt).
- Model space scale ("fit to page" or a set value) controls dash length and polyline width to line weight. It does not resize geometry. Source: import options (2025).
- Export: with sheet layers, the referenced design layers go to model space in top view with no scale. Without sheet layers, each design layer's scale goes to model space as one scale. The option "Rescale layers to" gives mixed scales one common scale. Source: export page (2026) and options (2024).
- The pages do not name `$INSUNITS`. That a Vectorworks file sets it is likely and unverified.
- A user got a 375 x 347 mm size where 210 x 194 mm was expected, in a Vectorworks DXF opened in LightBurn. The cause was not found. Unverified, one post (LightBurn link above).
- DXF without `$INSUNITS` has no unit. Importers guess. Source: Onshape forum summary (search excerpt, unverified). The field `$INSUNITS` is in the Autodesk DXF reference: https://help.autodesk.com/view/OARX/2024/ENU/?guid=GUID-A85E8E67-27CD-4C59-BE61-4DC9FADBF74E

## 4. Layers, classes, line types, weights, colours

- Import: layers become classes by default. Other choices are the active class or the active design layer. A prefix and a mapping set are available. Source: import options (2025).
- Export: classes become DXF layers (recommended). Layers become DXF layers as the other choice. Invisible classes go to invisible layers or are dropped by option. Fills and hatches can go to a separate layer with a suffix such as `_Hatch`. Source: options (2024).
- With sheet layers, design layers become blocks named like "Foundation (2D)" and "Foundation (3D)". Their layers start with an underscore and viewports freeze or thaw them. Source: export page (2026).
- Mapping sets are `.xml` or `.txt`. A mapping file has a header line with a tab, then pairs separated by a tab. Source: https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/Mapping_DXFDWG_layer_and_class_names.htm
- Names: version 2000 and later keep case, spaces and Unicode, except for illegal characters which change. Earlier versions go upper case and use underscores. Attribute tags never take spaces. Source: file formats page (2026).
- Line weights: export converts to the nearest DXF weight. Examples: 0.38 to 0.45 mm gives 0.40 mm and 2.06 to 6.48 mm gives 2.11 mm. A weight of 0 is deleted. Version 14 and earlier have no true line weights. Source: options (2024) and file formats page.
- Colours: the user picks true colours (RGB, group code 420) or indexed colours (ACI). Indexed colours allow a `.ctb` file with "map line weights to colors". Versions before 2004 use the fixed palette. Source: options (2024).
- Import maps colours to line weights, with a `.ctb` file if one is present. Source: import options (2025).
- Pattern fills export as plain solid colours in some cases. Source: file formats page.
- Layer transfer modes other than Paint are lost.

## 5. Code pages and Unicode

- Vectorworks pages say names keep Unicode in version 2000 and later. They do not describe the code page of DXF text. Unverified for text. Source: file formats page (2026).
- ezdxf: release R2004 (`AC1018`) and older are ASCII in the code page of `$DWGCODEPAGE`, default `ANSI_1252`. R2007 (`AC1021`) and newer are UTF-8. The escape `\U+nnnn` is used for characters outside the code page. Source: https://ezdxf.readthedocs.io/en/stable/dxfinternals/fileencoding.html
- Export option "Preserve mapped font on export" writes the replacement font, else the original. Source: options (2024). SHX fonts do not import.

## 6. Paper space and model space

- Import: each paper space layout becomes a sheet layer. Viewports keep scale, view and projection. Model space becomes design layers. Source: import page (2023).
- Export: sheet layers become paper space layouts. Section viewports export as blocks. Annotations become paper space objects above the viewports. Source: export page (2026).
- AutoCAD wants paper space objects fully above or fully below viewports. Objects under a viewport in Vectorworks end up above it. Source: export page (2026).
- Design layer viewports export as referenced DWG files by default, or are bound into the master file (class and layer overrides may be lost). Source: options (2024).
- Option "export viewports as 2D graphics in model space" flattens them. Source: options (2024).

## 7. Reported problems and workarounds

| Problem | Workaround | Source and status |
|---|---|---|
| Binary DXF is not read by some tools. | Export text DXF or DWG. | https://forum.lightburnsoftware.com/t/vectorworks-dxf-file-problem/136817 and the Vectorworks help. Verified by the help. |
| Image links are lost in some AutoCAD versions with DXF. | Use DWG. | File formats page (2026). Verified. |
| Unwanted lines appear after export when fills are lost in old releases. | Triangulate option or newer release. Check the file in a second program. | Search excerpt of the 2022 export procedure page. Unverified. |
| A contour broke on R12 export from Vectorworks 2019. | Export a newer DWG, then convert to R12 with DWG TrueView, the ODA converter or DraftSight. | https://forum.vectorworks.net/topic/62391-dxf-r12-export-problems/ (search excerpt, unverified). |
| 8,400 duplicate objects and many short segments in a laser cutting export. | Join paths and reduce nodes in a vector editor. | LightBurn thread, unverified. |
| Wrong overall size in the receiving tool. | Rescale, or set the unit on import. | LightBurn thread, unverified. |
| Wipeouts become polygons with smoothness set by the 2D conversion resolution. | Raise the resolution preference. | File formats page (2026). Verified. |
| Named blocks come back as symbols and groups as anonymous blocks. | Choose anonymous blocks on export. | File formats page. Verified. |
| Splines fail in laser tools and CAM programs. Common elsewhere. | Convert splines to polylines. | Several non-Vectorworks forums (search excerpt). Unverified for Vectorworks. |

## 8. Public sample files with a clear licence

Checked on 2026-10-23 with the GitHub API. We did not download the files.

| Corpus | URL | Licence | What it covers |
|---|---|---|---|
| ezdxf example DXF files | https://github.com/mozman/ezdxf/tree/master/examples_dxf | MIT (repo `LICENSE`) | `Minimal_DXF_AC1006/AC1009/AC1021`, `3dface`, `colors`, `valid_lineweights`, hatches (several), `hatch_issues`, MTEXT columns R2000/R2007/R2018, text alignments and fonts, nested block inserts, attribs, dimensions in blocks, wipeouts, `xclip`, viewports and frozen layers, `caret_encoding`, `uncommon`, `transparency` |
| ezdxf integration test data | https://github.com/mozman/ezdxf/tree/master/integration_tests/data | MIT | binary DXF R12 and R2000, other regression files |
| GDAL OGR DXF test data | https://github.com/OSGeo/gdal/tree/master/autotest/ogr/data/dxf | GDAL licence (MIT style, see `LICENSE.TXT`). Two files are copies from ezdxf and are MIT. Check each file before reuse. | 55 files: `3d`, `3dface`, `bin_dxf_r12`, `bin_dxf_r2000`, `BINARY_wipeout`, `LWPOLYLINE-OCS`, OCS to WCS, `closed_polyline_with_bulge`, hatch variants, `spline_*`, `dimension*`, `leader-mleader`, `insert-recursive-pair`, `block-*`, `byblock-bylayer`, `frozen-off`, `linetypes`, `text-fancy`, `utf-8`, `transparency`, `wipeout`, fuzzer cases |
| LibreCAD and QCAD samples | https://github.com/LibreCAD/LibreCAD | GPL v2. Not for copying into our repo. Use only as local manual test data, never ship. | Block library and samples |

We found no public Vectorworks written DXF with a clear licence. Our test corpus must be files that we write ourselves from the specification. A Vectorworks file posted by a user in a forum has no licence and stays out.

Other sources seen and not used: Burkardt DXF examples (https://people.math.sc.edu/Burkardt/data/dxf/dxf.html) say GNU LGPL, dated 2003, old R12 style.

## 9. Our code today

From `docs/interop.md`, rows IOP-01 and IOP-09 of `docs/feature-spec.md`, and `crates/kadakas-io/src/dxf`.

- Export: ASCII, release 2004 (`AC1018`), `$INSUNITS`, `$DWGCODEPAGE`, true colour (420), `\U+` escapes, blocks, GEODATA, POLYFACE, 3DFACE and MESH.
- Import: LINE, LWPOLYLINE, POLYLINE (2D and 3D), ARC, CIRCLE, ELLIPSE, SPLINE, TEXT, MTEXT, POINT, 3DFACE, HATCH, INSERT. Binary DXF is refused with a message. Paper space entities are skipped and counted. A tilted extrusion direction is skipped. ATTRIB, ATTDEF and VIEWPORT are skipped. `$INSUNITS` and `$DWGCODEPAGE` are read. Text is UTF-8 when valid, else the code page, else Latin-1.
- The import report counts skipped entities by kind.

## 10. Gaps, ranked

Rank = chance that a real Vectorworks or AutoCAD file hits it, times the harm.

1. Binary DXF import is refused. Vectorworks can write binary DXF and users may do so by mistake. Test: convert `bin_dxf_r12.dxf` and `bin_dxf_r2000.dxf` (GDAL data, MIT) and check that import reads them. Until then, check the refusal text names "save as text DXF".
2. Releases R12 to 2000 to 2007 to 2018 are only known from the ezdxf rules. Test: read `Minimal_DXF_AC1006`, `AC1009`, `AC1021` and the MTEXT R2018 file from ezdxf. The result must have the same entity counts as the ezdxf reader.
3. Entities that are likely in a Vectorworks export and not imported: 3DSOLID and BODY (ACIS), MESH, DIMENSION, IMAGE, WIPEOUT, LEADER and MLEADER, SOLID and TRACE, XLINE and RAY. Test: one file with each entity (from GDAL data: `wipeout`, `dimension`, `leader-mleader`, `solid`). Each must give a report line with the entity name, never a silent loss. Add MESH import (ezdxf can write one).
4. Wipeout and solid hatch are the way Vectorworks writes fills. We read HATCH and skip WIPEOUT. Test: import `hatches_1.dxf`, `hatches_2.dxf`, `hatch_as_multipolygon.dxf` and check filled areas against the expected area. Add a test where a wipeout covers a hatch.
5. Unitless files and wrong units. A file without `$INSUNITS` (R12 files, AC1006 and AC1009) needs a clear default and an option. Test: import `Minimal_DXF_AC1009.dxf` without options and with `unit: mm`, then check the box size in both cases. Also test a metre file in a feet document.
6. Tilted extrusion (OCS) is refused. Real files have 3D rotated blocks and arcs from other programs. Test: GDAL `ocs2wcs1.dxf`, `ocs2wcs2.dxf`, `ocs2wcs3.dxf`, `LWPOLYLINE-OCS.dxf`, `ellipse_z_extrusion_minus_1.dxf`. Import must place geometry at the right 3D position.
7. Paper space and viewports are skipped. Vectorworks puts sheet layers in paper space and the plan in model space, so model space data survives. Annotations and title blocks are lost. Test: `VP4.dxf` and `viewport_frozen_layers.dxf` (ezdxf). The report must say how many entities were in paper space and the layout names. Also test frozen layers: the layer must stay hidden.
8. Block handling: anonymous blocks (`*U12`), nested inserts, rotated and scaled inserts, MINSERT, block base point, ATTRIB text, `xclip`. Test: `insert-recursive-pair.dxf`, `block-basepoint.dxf`, `block-insert-order.dxf`, `multi_insert_with_attribs.dxf`, `xclip.dxf`. Recursion must stop with a report line. Insert counts must match ezdxf.
9. Colours and weights: ACI (including 0 ByBlock and 256 ByLayer), true colour, layer colour, the lineweight table of Vectorworks (nearest value). Test: `colors.dxf`, `valid_lineweights.dxf`, `byblock-bylayer.dxf`, `transparency.dxf`. Check colour per entity and the 0.40 mm mapping.
10. Text: MTEXT inline codes, `%%c` `%%d` `%%p` codes, `\U+` escapes, columns, fonts that are SHX. Test: `mtext_complex_inline_codes.dxf`, `caret_encoding.dxf`, `text_fonts.dxf`, GDAL `utf-8.dxf`, and a CP1250 and CP1251 file made with ezdxf. Check the text equals the expected Unicode string.
11. Export version: we write only `AC1018`. Some tools (old LibreCAD, laser software) read R12 or R2000 only. A version option is cheap for plain geometry. Test: export R12 and R2000 variants, read with ezdxf audit, and open in LibreCAD by hand.
12. Splines and ellipses become polylines. This is right for most uses and loses exactness. Test: `spline_closed.dxf`, `spline_weight.dxf`, `spline_qcad.dxf`, `spline_with_very_close_neg_to_zero_knot.dxf`. Check the deviation is under the tolerance and a closed spline stays closed.
13. Xref and external references. Vectorworks binds xrefs on import. We do not read them. Test: file with an XREF block. The report must name the missing file.
14. Hostile and damaged files. Real-world files have errors. Test: both GDAL `clusterfuzz-*` files and `insert-too-many-errors.dxf`. No panic, a bounded run time and a report.

## 11. Recommended next steps

1. Fetch the ezdxf and GDAL files into `tests/data/dxf-public/` with a `README` that keeps the licence text. Keep only MIT files. Do not copy the LibreCAD files.
2. Write the import tests in the order of the ranked list. Items 1, 3, 4 and 6 are the most likely to hit a user.
3. Re-check this note when a Vectorworks forum page becomes readable. The unverified items in sections 2, 5 and 7 are the first to confirm.
