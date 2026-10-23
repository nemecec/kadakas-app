# Interop of surface models (MOD-12)

Kadakas reads and writes triangle meshes and writes surface models to the common 3D formats. This page is the guide for the program, for agents and for the people who work on the code.

The SketchUp file format is not read and not written. No program of that kind was used as a source. The formats come from their public specifications.

## Formats

| Format | Import | Export |
|---|---|---|
| OBJ | `import.file`, colors from the MTL file | `export.file` format `obj`, with an MTL file |
| STL | binary and ASCII | binary (default) and ASCII |
| glTF and GLB | `.gltf` with embedded or loaded buffers, and `.glb` | `gltf` and `glb` |
| IFC | the IFC import of IOP-03 | surface models as faceted solids |
| DXF | 3DFACE, POLYFACE and polygon meshes (IOP-09) | 3DFACE, POLYFACE and MESH |

## Two kinds of object

An imported mesh is a `mesh3d` object (3D-20). It is an indexed list of vertices and triangles in millimeters. It holds millions of triangles. It has no edges and no faces to edit.

A surface model (MOD-06) has vertices, edges and faces with holes. It has limits: 55,000 vertices, 150,000 edges, 100,000 faces and 300,000 loop vertices.

An import makes `mesh3d` objects. A command turns a mesh into a surface model when it fits the limits. It refuses with the counts when it does not. The two steps can be one call: `import.file` with `to_surface`.

## Import

```json
{"method": "import.file", "params": {"path": "/data/model.obj", "options": {"unit": "mm", "up_axis": "z"}}}
```

The format comes from the extension or from `format`. One call is one undo step.

### Units and axes

The model has millimeters and Z up. A file says its own unit and axis, or it does not.

| Format | Unit | Up axis |
|---|---|---|
| glTF, GLB | meters, always | Y, always |
| OBJ | `unit`, default `m` | `up_axis`, default `y` |
| STL | `unit`, default `mm` | `up_axis`, default `z` |

A Y up file turns so that Z is up. The point (x, y, z) of the file becomes (x, -z, y). This is the inverse of the glTF and OBJ export of Kadakas. The units are `mm`, `cm`, `m`, `km`, `inch`, `foot`, `yard` and `mile`. A glTF file with a `unit` or an `up_axis` in the call is refused, because the format fixes both.

The result lists the unit, the axis and the box around the model in millimeters (`bounds_mm`), so a wrong guess shows at once. The option `center` moves the middle of the box to x = 0 and y = 0 and the lowest point to z = 0.

### Where the model lands

The 3D scene of an export is local: its numbers are near zero. A file that Kadakas wrote says where its origin was in the model. The OBJ has the comment `# local origin in model mm:` and the glTF has the extension `KADAKAS_georeference` (`local_origin`). The import adds this origin back, so a round trip returns the model to its place. The note in the report says so. The option `restore_origin: false` keeps the local numbers. An STL file has no place for an origin, so the STL export writes the coordinates of the model (the report has `origin_mm` for the record). A file from another program has no origin and lands as it is.

### Parts, colors and names

A file becomes one object for each pair of an object name and a material. The option `join` makes one object. A file with more than 5,000 such pairs has its parts joined by color first. If that is not enough the import is refused with the count.

A face with more than three corners is cut into triangles. A face that is not convex and has more than 500 corners, or that crosses itself, is cut as a fan, and a note counts these faces. The budget of triangles is checked before the face is cut. Facets that follow an `endsolid` line in an ASCII STL file go into a part of their own, with a note. ASCII STL numbers are read as doubles.

- OBJ: the name is the `o` or `g` line. The color is `Kd` of the material in the MTL file, with `d` or `Tr` as opacity. The MTL file is read from the folder of the OBJ file. A missing MTL file is a note.
- STL: one object per `solid` of an ASCII file, one object for a binary file. The header `COLOR=` and four bytes sets the color. Colors of single triangles are not read.
- glTF: the name is the name of the node. The color is the base color factor of the material, converted from linear to sRGB. Node transforms and instances count. A mesh used by several nodes is read once for each node. Textures are not read, and a note counts them.

Normals in a file are not used. The sides of a mesh follow the winding of its triangles, counterclockwise seen from outside. A face with more than three corners is cut into triangles. A concave face is cut by ear clipping.

### Guards

Every limit is checked before the work it protects. A refusal changes nothing and has a stable reason with the counts in its data.

| Limit | Default | Reason |
|---|---|---|
| File size, checked before the file is read | 512 MiB, and never above `import_text_bytes` of the session | `mesh_file_too_large` |
| Triangles in all parts | 8,000,000 (`max_triangles`) | `mesh_too_many_triangles` |
| Vertices in all parts | 4,000,000 (`max_vertices`) | `mesh_too_many_vertices` |
| Memory estimated from the counts | 2 GiB | `mesh_too_much_memory` |
| Parts | 5,000 | `mesh_too_many_parts` |

Other refusals: `mesh_truncated` (the file ends too soon, with `have` and `need`), `mesh_not_finite` (NaN or infinity), `mesh_out_of_range` (a coordinate over 1,000 km), `mesh_bad_index` (a face names a vertex that does not exist), `mesh_malformed`, `mesh_unsupported` (Draco, Meshopt, quantized or sparse glTF data, a glTF version other than 2) and `mesh_empty`. The size limits are `limit` errors. The others are validation errors. A reader never panics. A test feeds it random bytes and damaged files.

A glTF file has more limits. The nodes are walked with a stack that is checked at every push: at most 100,000 uses of nodes in all, at most 256 deep, and a node that is its own descendant is refused. The buffer files next to the glTF file are read once for each name, and all of them together count against `max_bytes` and the memory limit. A name may have letters outside ASCII (`caf%C3%A9.bin` or `café.bin`) and a folder below the glTF file, never `..`, a colon or the root.

## Convert a mesh to a surface model

```json
{"method": "mesh.to_surface", "params": {"id": "<mesh>", "soften_below_angle": 20}}
```

The core command is `mesh3d_to_surface`. The object keeps its id, layer, class and data. The color of the mesh becomes the fill of the object. The change is one undo step. The steps:

1. The mesh moves so that its center is at the local origin. The model transform puts it back. The heights are measured from the elevation of the layer, as for every surface model.
2. Vertices within the tolerance weld. The default is 0.01 mm, the length tolerance of surface models. The option `tolerance` sets it from 0.001 to 1000 mm.
3. Triangles with two equal corners are dropped. So are triangles that repeat another one. A triangle with no area is the glue of a triangulated face when it lies in the plane of a neighbour. Else it is dropped. All are counted.
4. Triangles that share an edge and lie in one plane merge into one face, from the largest triangle outward. The plane test is the one that the edits use. The outer loop and the holes come from the boundary of the region. A region that touches itself at a vertex stays as separate triangles, and the report counts these regions in `unmerged_regions`. With `merge: false` every triangle stays a face.
5. A vertex that has two edges only, in a straight line, goes.
6. The counts are compared with the limits. Merging comes first, so a flat mesh with a million triangles can still fit. A result over a limit is refused with `mesh_too_large_for_surface`. The data has `over` (each count with its limit) and `counts` (what the result would have). A mesh with more than 2,000,000 triangles is refused before the work.
7. With `soften_below_angle`, the soften engine of MOD-08 softens the edges under the angle, soft and smooth. Edges between faces in one plane count too.

A triangle that has another triangle with the same corners the other way round is the back of a double sided mesh. The second one is dropped and counted (`reversed_triangles`). Faces that enclose no volume are not a closed solid.

The faces keep the winding of the triangles, so the front is the outside of a closed mesh. The answer has `softened_edges` and a `report` with the counts: `source_triangles`, `welded_vertices`, `degenerate_triangles`, `duplicate_triangles`, `joined_slivers`, `vertices`, `edges`, `faces`, `faces_with_holes`, `merged_triangles`, `unmerged_regions`, `healed_vertices`, `border_edges`, `nonmanifold_edges`, `closed`, `area_mm2` and `volume_mm3` for a closed mesh. `dry_run: true` converts a copy and changes nothing.

## Soften edges at import

Soft edges are not drawn and shade smoothly. A mesh from a file has many edges that a person does not want to see, for example the sides of a cylinder. The option is off by default, so the edges of the source stay as they are.

For an agent:

- `soften_below_angle` in the options of `import.file` (degrees, 0 to 180). It asks for the surface model, because a mesh has no edges to soften. `to_surface: true` converts without softening.
- The result always has `softened_edges`. It is 0 when nothing was softened.
- A part that does not fit the limits stays a mesh with no edge softened. `report.to_surface.refused` lists it with the counts, and a note says so.
- `preview: true` reads the file, converts it and answers with the count, and changes nothing.
- The IFC import takes the same options (`to_surface`, `soften_below_angle` and `surface_tolerance`) and converts the meshes it made.

For a person, the import window of the app and of the modeler (File > Import) asks "Soften edges under an angle?" for a mesh file. It is off by default. The angle is 20 degrees. When it is on, the window shows how many edges would be softened. The count reads and converts the file in the window, so it runs only while the question is on, and only for files up to 32 MB. A bigger file is imported with the same softening and not counted before. The answer is kept when the person imports, not when the window is cancelled. It is kept in the prefs of the program (`import_soften` in `prefs.json` of the app and in `modeler.json` of the modeler) and comes back at the next import.

## Export

### STL

`export.file` with `format: "stl"` writes the triangles of all 3D objects, or of the objects in `objects`. The options are `encoding` (`binary` by default, or `ascii`), `units` (`mm` by default, or `m`), `y_up` (false by default) and `name`. STL has no colors, materials or lines. The result has `open_models`, the number of exported surface models that are not closed solids, and a note, because a printer needs closed solids. It also has `leaky_models`: closed solids whose triangles are not watertight (an edge that two triangles do not share), with a note. The report holds the coordinates of the model, so reading the file again puts it where it was.

### OBJ, glTF and GLB

These formats are written by `kadakas-x3d` (3D-10 and 3D-11). They write surface models and meshes like every other 3D object. Y up and meters are the default. The options `units` and `y_up` change that. A surface model is written as the triangles of its faces. Soft edges do not show in these files.

### IFC

A surface model is written as `IfcFacetedBrep` when it is a closed solid. The faces keep their holes (`IfcFaceBound`), so a face is not cut into triangles. Every other face is written in an open shell of an `IfcShellBasedSurfaceModel`, and a note says how many faces. A model that has both solids and open faces is one surface model, with its solids as closed shells. Hidden faces are left out, so a solid with a hidden face is an open shell. IFC2x3, IFC4 and IFC4x3 all take the same entities. Component instances are skipped with the advice to explode them first.

### DXF

A surface model is written as a polyface mesh by default. The option `mesh_entity` is `polyface`, `face3d` (one 3DFACE per triangle) or `mesh` (one MESH entity, a subdivision mesh at level 0 with triangle faces). The heights include the elevation of the layer. A hidden face is left out. A component instance is written too (MOD-13), as one mesh for each part of its definition through the transform of the instance, and a part in a class that is switched off is left out. The diagonals that cut a face into triangles and the soft edges are marked invisible (the edge flags of a 3DFACE, the negative indices of a polyface face), so a CAD program draws the edges of the faces only. The file is release 2004 (`AC1018`) with its code page, unless the option `version` says another one (see the DXF files section). The audit of ezdxf accepts the MESH entity in it. A DXF file with a MESH entity is best read by a program that reads DXF 2010 or later. Kadakas reads the MESH entity too (IOP-15): it becomes a `mesh3d` object with the control mesh.

## DXF files (IOP-01, IOP-15, IOP-16)

This section is about real-world DXF files. The research with its sources is `docs/research/dxf-real-world.md`. The code is in `crates/kadakas-io/src/dxf`.

### What is read

- Text DXF and binary DXF, from release 12 (`AC1009`) to 2018 (`AC1032`). `report.release` names the release of the file.
- A binary file starts with `AutoCAD Binary DXF`. Release 12 writes group codes in one byte, newer releases in two. The reader finds the width from the first pair. A damaged binary file is refused with the byte where it is damaged and the advice to save a text DXF.
- Limits that protect the import: the file size (`max_bytes`), 100 million pairs, 20 million records, 1 MiB for one value, 12 levels of blocks, 5 million visits and `max_exploded` objects from blocks. Each limit gives a refusal or a line in the report.
- Text is UTF-8 when it is valid. Else the code page of `$DWGCODEPAGE` is used (`ANSI_1250` to `ANSI_1258`, `ANSI_932`, `ANSI_936`, `ANSI_949`, `ANSI_950`, `DOS437` and the other DOS pages, `ISO8859-n`, `MAC-ROMAN`). Else the text is Latin-1.

### Units

The order is: the option `unit`, then `$INSUNITS`, then a guess. Releases before 2000 have no `$INSUNITS`. A file with `$INSUNITS` 0 has no unit.

- The guess is inches when `$MEASUREMENT` is 0 (English). Else it is millimeters.
- The note names the reason and the guess, for example `the drawing has no unit (DXF release 12 stores none): assuming millimeters`.
- The option `unit` replaces the unit of the file. A note says so when the two differ.
- All codes of length of `$INSUNITS` work, also the US survey units (21 to 24). The astronomical units (17 to 20) are not known.

### Entities

| Entity | What the import does |
|---|---|
| LINE, ARC, CIRCLE, LWPOLYLINE, POLYLINE, POINT, TEXT, 3DFACE, INSERT | As before (IOP-01, IOP-09). |
| ATTRIB | Visible text. An invisible attribute and an ATTDEF are reported. |
| MTEXT, TEXT | The codes `%%c`, `%%d`, `%%p`, `%%nnn`, `\U+XXXX`, the caret codes (`^J`) and the MTEXT codes for fonts, heights, colors, stacks and paragraphs. The text equals the plain text of the ezdxf reader. |
| SOLID, TRACE | A filled polygon (fill = pen color). The corners are in the order 1, 2, 4, 3. |
| HATCH | Each boundary path is a closed polyline. A solid hatch is filled. An island is a white filled polyline with `data.hatch_island`. A gradient has a flat fill of its first color. A pattern hatch is an outline, and a note counts the pattern names. |
| WIPEOUT | A white polygon with no line and `data.wipeout`, above what was drawn before it. |
| MESH | A `mesh3d` object (the control mesh). |
| LEADER | An open polyline. The arrowhead is not drawn. |
| DIMENSION, ARC_DIMENSION, ACAD_TABLE | The lines and text of the anonymous block. Without the block the entity is reported. The definition points on the layer `Defpoints` are reported. |
| SPLINE, ELLIPSE | Polylines within the tolerance. A note counts them. A spline without knots gets uniform knots, as in the ezdxf reader. |
| Entities in a tilted plane | CIRCLE, ARC, LWPOLYLINE, POLYLINE, SOLID, ELLIPSE and the paths of a HATCH become 3D polylines (the arbitrary axis algorithm of the DXF reference). TEXT, MTEXT and INSERT in a tilted plane are reported. |
| IMAGE, 3DSOLID, BODY, REGION, SURFACE, MLEADER, XLINE, RAY, MLINE, OLE2FRAME, XREF | Reported by name with the reason. An IMAGE names its file, an XREF its path. |
| XCLIP of a block reference, transparency | Not applied. A note counts them. |

### The report

- `report.entities_read` is the number of entities that the import visited. Each copy of an exploded block counts.
- `report.entities_imported` and `report.entities_skipped` add up to it. A field with 0 is left out of the JSON.
- Each skipped entity has a line in `report.skipped` with its kind, the reason and a count.
- `report.notes` says what was approximated or guessed.

### Paper space (IOP-16)

| DXF | Kadakas |
|---|---|
| A layout of paper space with content | A sheet layer (mm, 1:1) named like the layout. The paper size comes from the layout (`A3` and so on, else a custom size). A layout with only the viewport of the paper is not imported. |
| VIEWPORT | A viewport: frame, scale (view height over paper height), standard view (plan, front, isometric and so on), frozen layers, crop (a polyline or a circle that the viewport names), render mode. |
| Entities of the layout, blocks, title block, attributes | Objects on the sheet layer, in the order of the file, so what lies under a viewport stays under it. |
| 3D polyline in paper space | Flattened to 2D, and a note counts it. Other shapes that a sheet cannot hold are reported. |

A viewport that is off, has no size or is not valid is reported. A twist and a perspective view are not kept, and a note counts them. A view direction that is not a standard view uses the nearest standard view, and a note says which.

The export writes the reverse. A sheet layer is a layout with the paper, the viewport of the paper, the viewports, the annotations of the viewports, the title block (lines and text) and the objects of the sheet. The first layout is in the section ENTITIES with the flag `67 1`, the others in their blocks `*Paper_Space0` and so on. The option `sheets` is `false` to leave sheets out. Release 12 has no layouts. The update of a drawing (IOP-23) does not import paper space.

### Export releases

The option `version` of `export.file`:

| Value | Release | Notes |
|---|---|---|
| `AC1018` (default) | 2004 | The reference form. |
| `AC1015` | 2000 | No true colors (the nearest indexed color), the polyface mesh instead of the MESH entity. |
| `AC1027`, `AC1032` | 2013, 2018 | The same layout as 2004. Checked with the ezdxf reader and its audit. Not opened in AutoCAD. |
| `AC1009` | 12 | Only LINE, CIRCLE, ARC, POINT, TEXT and POLYLINE. No handles, unit, layouts or Unicode. The code page is `ANSI_1252`. Names are capital letters. Every object that it cannot hold is counted in `report.skipped`. |

The names `2000`, `2004`, `2013`, `2018` and `R12` work too. The export window has a list for it.

### Test files

- Public sample files of ezdxf (MIT licence) and GDAL (MIT style licence) are fetched with `curl` from a fixed commit into `tests/data/dxf-public/`. Git ignores that folder. The tests skip when there is no network, or when `KADAKAS_CORPUS=off` is set. No LibreCAD file (GPL) is used.
- Every corpus file goes through the import. It must not panic, it must end in time, and its report must account for every entity. The counts of entities, layouts, viewports and MTEXT texts are compared with the ezdxf reader (`crates/kadakas-io/tests/fixtures/dxf_public_counts.json`, made by `tests/data/make_public_counts.py`).
- Our own small files are in `tests/data/dxf-own/` (made by `tests/data/make_own_files.py`). They run offline.
- The tests for damaged and hostile files are in `crates/kadakas-io/tests/dxf_hostile.rs`.

### Not done

- Block clips (XCLIP) and transparency are not applied.
- Pattern hatches are outlines. The pattern is not drawn.
- A viewport with a twist keeps no twist.
- TEXT, MTEXT and INSERT in a tilted plane are reported and not placed.
- The releases 2013 and 2018 of the export are not opened in a CAD program. Release 12 is not opened in LibreCAD or a laser program by hand.
- An import of a very large drawing is slow in the editor, because the batch of a few hundred thousand commands runs in quadratic time. The import itself is linear.

## Code

| Part | Place |
|---|---|
| Readers for OBJ, STL and glTF, writer for STL | `crates/kadakas-meshio` (one module for each format, no other Kadakas crate) |
| Mesh into a surface model | `crates/kadakas-core/src/surface/frommesh.rs` |
| The command | `Command::Mesh3dToSurface` in `crates/kadakas-core/src/command/surface_ops.rs` |
| `import.file`, `export.file` for STL | `crates/kadakas-api/src/meshio_api.rs` |
| `mesh.to_surface` | `crates/kadakas-api/src/mesh_api/convert.rs` |
| IFC export of surface models | `crates/kadakas-ifc/src/export/faceted.rs` |
| DXF export of surface models | `crates/kadakas-io/src/dxf/export.rs` and `export/native.rs` |
| The form of the windows | `crates/kadakas-ui-core/src/mesh_import.rs` |
| The windows | `crates/kadakas-app/src/io_dialog.rs` and `crates/kadakas-modeler/src/interop.rs` |

The parsers are written for Kadakas. No new third-party crate was added, so the notices and the About list have nothing new.

## Tests

- `kadakas-meshio`: unit tests for every reader and the STL writer, damaged and hostile files, and a property test with random bytes.
- `kadakas-core`: tests of the conversion with cubes, spheres, grids with holes, welds, slivers, limits and soften counts, a property test on random terrain and spheres, and the command tests (`tests/mesh_to_surface.rs`).
- `kadakas-api/tests/meshio.rs`: a U shaped prism whose outline has corners on a line (watertight through every format), the place of the model after a round trip, hostile glTF files, round trips of a closed slab with a hole through OBJ, STL, ASCII STL, glTF and GLB (the same volume, area and face count), a property test on random prisms and terrain, options, guards, refusals and the path policy.
- `kadakas-ifc` and `kadakas-io`: the entities of the surface export.
- `tests/behavior/test_meshio.py`: the checks of IfcOpenShell (the IFC of surface models), ezdxf (the DXF) and glTF validation. They skip when the tool is missing.

## Not done

- Textures and materials of OBJ and glTF are not read. Only the base color is kept.
- Colors of single triangles of an STL file, vertex colors and normals are not read.
- glTF with Draco, Meshopt or quantized data, and sparse accessors, are refused.
- A DXF MESH entity with a subdivision level above 0 is read as its control mesh.
- Component instances are not exported to IFC. Explode them first. DXF writes them (MOD-13).
