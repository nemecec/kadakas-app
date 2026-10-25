# Interop of surface models (MOD-12)

Kadakas reads and writes triangle meshes and writes surface models to the common 3D formats. This page is the guide for the program, for agents and for the people who work on the code.

The SketchUp file format is not read and not written. No program of that kind was used as a source. The formats come from their public specifications.

## Formats

| Format | Import | Export |
|---|---|---|
| OBJ | `import.file`, colors from the MTL file | `export.file` format `obj`, with an MTL file |
| STL | binary and ASCII | binary (default) and ASCII |
| glTF and GLB | `.gltf` with embedded or loaded buffers, and `.glb` | `gltf` and `glb` |
| STEP | `.step` and `.stp` (IOP-11), AP203, AP214 and AP242 | not done |
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
| File size, checked before the file is read | 512 MiB (STEP: 128 MiB), and never above `import_text_bytes` of the session | `mesh_file_too_large` |
| Triangles in all parts | 8,000,000 (`max_triangles`) | `mesh_too_many_triangles` |
| Vertices in all parts | 4,000,000 (`max_vertices`) | `mesh_too_many_vertices` |
| Memory estimated from the counts | 2 GiB | `mesh_too_much_memory` |
| Parts | 5,000 | `mesh_too_many_parts` |

STEP files have limits of their own, each with its own reason: `mesh_too_many_entities` (the entities of the file), `mesh_too_many_instances` (the uses of products in an assembly), `mesh_too_many_copies` (the bodies that mapped items and repeated products would copy), `mesh_too_much_work` and `mesh_too_much_time` (see Guards of STEP). All are `limit` errors.

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

## STEP files (IOP-11)

A STEP file (ISO 10303-21 text) holds the exact geometry of a product from a manufacturer. `import.file` cuts it into triangles on load. Each result is a `mesh3d` object that also keeps the exact surfaces (see `docs/exact-solids.md`). Exports are not done.

```json
{"method": "import.file", "params": {"path": "/data/bench.step", "options": {"tolerance": 0.05}}}
```

The reader is the crate `kadakas-step`. It uses the public knowledge of the format and no CAD program. No third-party crate was added. `truck-stepio` and `ruststep` were not chosen. They add a large dependency tree. Our own reader is small and keeps to our limits.

### What is read

| Part of the file | Result |
|---|---|
| Schemas AUTOMOTIVE_DESIGN (AP214), CONFIG_CONTROL_DESIGN (AP203), AP242 | read. Another schema name gives a note and the file is read the same way |
| MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS, FACETED_BREP | solids. A closed shell with inward normals is turned |
| SHELL_BASED_SURFACE_MODEL, FACE_BASED_SURFACE_MODEL | open surfaces |
| TRIANGULATED_FACE, TRIANGULATED_FACE_SET, TRIANGULATED_SURFACE_SET, COMPLEX_TRIANGULATED_FACE and SURFACE_SET, TESSELLATED_SOLID, TESSELLATED_SHELL | the given triangles are taken. These meshes have no exact data |
| GEOMETRIC_CURVE_SET, GEOMETRIC_SET | 3D polylines (`polyline3d`) |
| PLANE, CYLINDRICAL_SURFACE, CONICAL_SURFACE, SPHERICAL_SURFACE, TOROIDAL_SURFACE | exact |
| SURFACE_OF_REVOLUTION, SURFACE_OF_LINEAR_EXTRUSION | the profile curve is a line, circle, ellipse, spline, polyline, trimmed or composite curve |
| B_SPLINE_SURFACE_WITH_KNOTS, BEZIER, QUASI_UNIFORM and UNIFORM surfaces, also rational (complex entity with RATIONAL_B_SPLINE_SURFACE) | degree up to 25 |
| RECTANGULAR_TRIMMED_SURFACE | the basis surface. The bounds of the faces cut it |
| LINE, CIRCLE, ELLIPSE, B_SPLINE_CURVE_WITH_KNOTS (also rational), BEZIER, QUASI_UNIFORM and UNIFORM curves, POLYLINE, TRIMMED_CURVE, COMPOSITE_CURVE, SURFACE_CURVE, SEAM_CURVE | edge curves |
| FACE_BOUND, FACE_OUTER_BOUND, ORIENTED_EDGE, ORIENTED_FACE, ORIENTED_CLOSED_SHELL, POLY_LOOP | holes and orientation flags are used |
| PRODUCT, PRODUCT_DEFINITION, SHAPE_DEFINITION_REPRESENTATION | the product name is the name of the mesh |
| NEXT_ASSEMBLY_USAGE_OCCURRENCE with CONTEXT_DEPENDENT_SHAPE_REPRESENTATION and ITEM_DEFINED_TRANSFORMATION | the placement of each instance, to depth 32 and 20,000 instances. A usage with no placement sits at the origin and a note counts it. A product that only mapped items use shows where they place it, not at the origin. A usage whose parent maps the product is that mapped item, unless the usage has a transformation that no mapped item has: then it is one more use |
| MAPPED_ITEM with REPRESENTATION_MAP | an instance of a representation |
| LENGTH_UNIT as SI_UNIT or CONVERSION_BASED_UNIT, PLANE_ANGLE_UNIT | millimeters, meters, inches and so on, for each representation |
| STYLED_ITEM with COLOUR_RGB or a predefined colour, SURFACE_STYLE_TRANSPARENT | colour of a solid, a shell or a face. A part gets one mesh for each colour |

### Not read

- Hyperbolas, parabolas, offset curves and surfaces, and degenerate tori. The edge becomes a straight line or the face is skipped. The report says so.
- Pcurves. Faces are cut from the 3D edges and the surface, not from the curves in the parameter space.
- Wireframe edges without faces, points, vertex loops, product structure of kinematics, PMI, GD&T, layers and visibility.
- Anchor and reference sections of AP242 edition 2. Only the data section is read.
- A sphere or cone face with a loop around the axis is closed through the pole on its side.
- A face with a loop around a torus in the v direction is skipped.

### How faces are cut

- The boundary of each face is cut first. An edge is cut once and both faces use the same points, so a closed solid has no cracks. Equal points are joined at the end (0.1 percent of the tolerance in millimeters, at least 1 micrometer).
- A plane is cut by ear clipping with the holes joined by bridges. Other surfaces are cut in the parameters (u, v) of the surface. The angle of a periodic surface is made continuous along each loop. A pole of a sphere or cone is a line in (u, v). It is cut into many points at one place.
- A triangle is split where the surface leaves it by more than the tolerance. It is also split where the normal turns by more than the angle.
- The points of the boundary never move.
- Edges are flipped to keep the triangles round (Delaunay in a chart scaled to the size of the surface).
- A B-spline face is cut the same way. The boundary is placed on the surface by a search with Newton steps. The limits are the degree (25), the control points (262,144 for a surface), and 200,000 triangles for a face. A face over its limit is cut as far as the limit allows, and a note says so.
- The default tolerance is 1/2000 of the diagonal of the points of the file, between 0.01 and 0.5 mm. The default angle is 15 degrees. A larger tolerance gives fewer triangles.

### Options

| Option | Meaning |
|---|---|
| `unit` | The unit of the numbers. It replaces the unit of the file. Default: the file |
| `tolerance` | mm, 0.001 to 1000. The largest distance of a triangle from its surface |
| `angle` | degrees, 1 to 90, default 15 |
| `per_product` | true (default): one mesh for each product and colour. false: one for each body and colour |
| `join` | one mesh for the whole file |
| `keep_exact` | true (default): the mesh keeps its exact surfaces |
| `to_surface`, `soften_below_angle`, `preview`, `center`, `layer`, `name`, `max_*` | as for the other mesh formats. A part that becomes a surface model loses its exact data, and a note says so |

### The report

- `report.step.entities_read` equals `entities_used` plus `entities_skipped`. Every entity is in one of them.
- `report.step.used` lists the entity types that were read.
- `skipped` lists the others, each with a reason and a count.
- `tessellated` lists the faces and triangles by kind of surface.
- `products`, `instances`, `length_unit_mm`, `unit_from_file`, `tolerance_mm`, `angle_deg` and `welded_vertices` say what was done.
- `length_unit_mm` has one value for each unit in the file.
- A color of a surface wins over the color of a curve. An overriding style wins over the style it overrides, in any order of the file. A style for one instance of an item is not applied to all instances, and the report counts it as skipped.
- `report.exact` says how many meshes keep their exact data and why some do not. `report.curves` counts the 3D polylines.
- A file with nothing to read is refused with `mesh_unsupported` or `mesh_empty` and the reasons. Damaged files give `mesh_truncated` or `mesh_malformed`.

### Guards

A STEP file is not trusted. These limits hold:

- values nest to 48 levels, and the file has at most 5 million entities
- the parser stops at the entity limit and at an estimate of its memory (3 GiB), before the file is parsed in full
- a STEP file has at most 128 MiB (64 MiB in the browser build). The reader takes 9 to 14 times the size of the file in memory, measured with `/usr/bin/time -l` on a release build: a file of 5.1 MB took 47 MB, one of 19.5 MB with 475,000 triangles took 233 MB, and one of 30.6 MB with 390,000 triangles took 427 MB. The other formats allow 512 MiB
- vertices, triangles and memory have the limits of the other mesh formats
- the degree and the control points are limited as above
- a curve has at most 200,000 points, and an assembly has at most 32 levels
- an assembly has at most 20,000 instances. Every use of a product counts, also in a product that has no shape
- the copies of mapped items are counted before they are made
- a counter of work stops files that ask for much time and make little output. A step is about 50 ns on a current computer. A surface evaluation takes a few steps and a triangle of a curved face about 50. A file gets 60 million steps, and 100 more for each triangle that the reader cut from surfaces (`max_work` and `work_per_triangle`). A copy of a triangle, or a triangle that the file gives, earns nothing. A file with 8 million triangles gets 860 million. Faces that give no triangles count too, and so does the scan for the size of the file (once). The error is `mesh_too_much_work`
- a clock is a guard next to the counter: 60 seconds (`max_millis`), `mesh_too_much_time`. It is for slow computers and for builds without optimisation. The browser build has no clock and uses the counter only.
- the measured numbers are in the table below

Measured on a release build (`tests/release_probes.rs` and the `probe` example, memory with `/usr/bin/time -l`):

| File | Time | Result |
|---|---|---|
| 5 MB with 20,000 mapped items of an empty representation and 100,000 points | 35 ms (was 39.8 s) | no triangles |
| 19.5 MB with 2,500 plates with a hole, 475,000 triangles | 1.4 s (was 8.8 s) | read, 26.6 million steps |
| the same at a tolerance of 0.005 mm, 1.2 million triangles | 4.7 s | read, 92 million steps (it was refused after 3.8 s) |
| the same at 0.002 mm, 1.9 million triangles | 8.8 s | read, 182 million steps |
| 360 KB with 200 faces of 4,800 points on one loop | 0.8 s | read, 33 million steps |
| a face with 100 x 100 square holes (2.8 MB) | 0.14 s (was 4.0 s) | read, 60,000 triangles |
| a face with 140 x 140 square holes (5.6 MB) | 0.27 s (was 15.7 s) | read, 117,600 triangles |
| a face with 30,000 triangular holes (7 MB) | 0.4 s (was 30 s) | read, 150,000 triangles |
| a face with 100,000 triangular holes (24 MB) | 1.9 s, 487 MB (was 530 s, 1.2 GB) | read, 500,000 triangles |
| small holes on the diagonal of a face, 1,000 / 10,000 / 90,000 holes (34 MB) | 0.02 s / 0.2 s / 2.0 s (was 0.13 s / 4.0 s / 157 s) | read, 540,000 triangles for 90,000 |
| 10,000 holes in one row of a plate of 1 m (the price was too low) | 0.2 s (was refused) | read, 60,002 triangles |
| 30,000 holes in one cell of the first grid, in a face of 1,000 km | 0.4 s | read |
| a comb of 192,000 points / a circle and a star of 390,000 points | 0.5 s (was 2.9 s) / 1.0 s | read |
| 32 MB with 6 mapped copies of a mesh of a million triangles, then 60,000 faces that give nothing | 4.6 s (was 54 s) | `mesh_too_much_work` |
| 1.4 MB with 30,000 faces on a revolution of a line, none cut | 0.5 s | `mesh_too_much_work` |
| 3,000 faces on a revolution of a spline with 2,000 control points | 4.4 s | `mesh_too_much_work` |
| the same, with no counter | 60 s | `mesh_too_much_time` |

The holes of a face are joined to its outline, and the polygon is cut by ear clipping, with quadtrees: one of all the segments (the outline, the holes and the bridges) and one of the corners that bend inward. A hole costs the boxes around its bridge, not the size of the polygon, also when the holes are packed in a band. The bridge goes to the nearest corner that the hole can see. A hole that touches the polygon at a corner, or with a corner on an edge, is joined at that point with no bridge. Holes that touch the outline or each other in several places are cut correctly or refused. The fuzz tests with 10,000 cases of every kind found no wrong result. Before the cut, repeated points and holes that are the same are removed, and the report counts them in a note. The result is checked against the outline less the holes. If the areas differ or a triangle faces the wrong way, the face is left out with a reason.

Every step of the cutting is counted. The price of a polygon is 1,200 steps for each hole and 260 for each point, as measured on holes in rows, on diagonals and in grids, and on circles, stars, combs and zigzags (2 to 8 ns for a step). The cutting stops at one million steps and twice the price. A face that needs more is left out, with a note and the reason, and the steps count against the work of the whole job, which stops when that is over its limit. Ten steps are one unit of work.

One step is about 50 ns on curved faces (10 to 19 thousand steps for each millisecond on a sphere, a torus, a revolved solid and a B-spline patch, at three tolerances) and 20 to 40 thousand steps for each millisecond on planes and on the plates. So 60 million steps are about 4 seconds of work with no output. The 100 steps for each triangle are about 5 microseconds, which is about what a triangle takes (3 to 7 microseconds on a curved face, under 1 on a plane).

References that point to themselves end. Numbers that are not finite or larger than 1e12 are not used. A number that the file defines twice uses the first definition, and a note counts these. The tests cut files in many places, damage bytes, make cycles and use huge counts.

### Tests

- `tests/data/step/make_step_files.py` writes the test files. They hold a box, a cylinder, a sphere, a torus and a cone. They also hold a revolved solid, an extruded solid, a plate with a hole and B-spline patches. More files hold a faceted solid, an AP242 tessellated solid, curves, colours, units and an assembly with two placements.
- The Rust tests compare volumes and areas with the analytic values and check that the solids are closed.
- A property test uses random boxes and cylinders in random places.
- A public file of CadQuery (Apache-2.0) is fetched at run time and is not in the repository. The test skips without a network.
- `tests/behavior/test_step.py` compares with OpenCascade when `OCP` is installed and skips when it is not.

## Export

### STL

`export.file` with `format: "stl"` writes the triangles of all 3D objects, or of the objects in `objects`. The options are `encoding` (`binary` by default, or `ascii`), `units` (`mm` by default, or `m`), `y_up` (false by default) and `name`. STL has no colors, materials or lines. The result has `open_models`, the number of exported surface models that are not closed solids, and a note, because a printer needs closed solids. It also has `leaky_models`: closed solids whose triangles are not watertight (an edge that two triangles do not share), with a note. The report holds the coordinates of the model, so reading the file again puts it where it was.

### OBJ, glTF and GLB

These formats are written by `kadakas-x3d` (3D-10 and 3D-11). They write surface models and meshes like every other 3D object. Y up and meters are the default. The options `units` and `y_up` change that. A surface model is written as the triangles of its faces. Soft edges do not show in these files.

### IFC

A surface model is written as `IfcFacetedBrep` when it is a closed solid. The faces keep their holes (`IfcFaceBound`), so a face is not cut into triangles. Every other face is written in an open shell of an `IfcShellBasedSurfaceModel`, and a note says how many faces. A model that has both solids and open faces is one surface model, with its solids as closed shells. Hidden faces are left out, so a solid with a hidden face is an open shell. IFC2x3, IFC4 and IFC4x3 all take the same entities. Component instances are skipped with the advice to explode them first.

### DXF

A surface model is written as a polyface mesh by default. The option `mesh_entity` is `polyface`, `face3d` (one 3DFACE per triangle) or `mesh` (one MESH entity, a subdivision mesh at level 0 with triangle faces). The heights include the elevation of the layer. A hidden face is left out. A component instance is written too (MOD-13), as one mesh for each part of its definition through the transform of the instance, and a part in a class that is switched off is left out. The diagonals that cut a face into triangles and the soft edges are marked invisible (the edge flags of a 3DFACE, the negative indices of a polyface face), so a CAD program draws the edges of the faces only. The file is release 2004 (`AC1018`) with its code page, unless the option `version` says another one (see the DXF files section). The audit of ezdxf accepts the MESH entity in it. A DXF file with a MESH entity is best read by a program that reads DXF 2010 or later. Kadakas reads the MESH entity too (IOP-15): it becomes a `mesh3d` object with the control mesh.

## IFC files (IOP-29)

What Kadakas reads and writes of the IFC files that other programs make. The sources are public (`docs/research/ifc-real-world.md`); the tests run against public files with a clear licence.

### Containers and text

- **STEP** (`.ifc`) and **ifcZIP** (`.ifczip`, a zip with one `.ifc`) are read and written. The kind of a file is found from its first bytes, not from its name. A zip is refused when it has more than 1,000 entries, more than one `.ifc`, an entry name with `..`, a drive letter or a leading slash, an unpacked IFC file over the size limit (`max_file_mb`), or, above 64 MB, more than 200 times the size of the zip. Nothing is written to disk when a zip is read. An export to a path that ends in `.ifczip` writes an ifcZIP (same bytes for the same document).
- **IFC XML** and **IFC JSON** are not read. The answer names the format: `IFC XML is not supported; export STEP (.ifc) or ifcZIP`. Backlog: a reader for ifcXML (the ISO samples have twins in XML) when users ask for it; the schema is large and a safe reader needs a size and depth budget like the STEP reader has.
- Strings: `\X\hh`, `\S\c`, `\P<A-I>\` (ISO 8859 parts 1 to 9), `\X2\...\X0\` and `\X4\...\X0\` are decoded; an escape that is not complete stays as written. Keywords are not case sensitive (`#2= ifcactorrole(.architect.,$,$);` is read), `DATA`, `HEADER` and `ENDSEC` too. A repeated step id keeps the first entity and the report counts the others. Products that repeat a GlobalId are all imported and counted in a note.
- The report has `entities_read` (products with a representation), `entities_imported` and `entities_skipped`; the first equals the sum of the others, and every skipped product has a line in `skipped`.

### Geometry

| IFC | Result |
|---|---|
| `IfcRelVoidsElement` openings | cut from the Body of the host with a boolean on the closed meshes (walls, slabs, any solid). A `Reference` representation is not subtracted. When a cut fails (an open mesh, a body that is no solid, too complex), the host keeps its gross shape, the opening becomes an object of its own, and a note counts them with the reason |
| `IfcBooleanResult` with a solid | difference, union and intersection of closed meshes. `IfcPolygonalBoundedHalfSpace` is a prism cut by the plane |
| `IfcRevolvedAreaSolid` | exact profile (arcs), the angle in the angle unit of the file |
| `IfcSweptDiskSolid` | a pipe along the directrix, mitered at the bends, hollow with `InnerRadius`. Start and end parameters are applied on polylines, and reported as not applied on curved directrices |
| `IfcCsgSolid`, `IfcBlock`, `IfcSphere`, `IfcRightCircularCylinder`, `IfcRightCircularCone`, `IfcRectangularPyramid` | meshes within the curve tolerance (2 mm); booleans of them work |
| `IfcAdvancedBrep`, `...WithVoids`, and shells with edge loops | the geometry entities are those of STEP, so the solid is written as a small STEP file in memory and read by `kadakas-step` (planes, cylinders, spheres, tori, B-spline surfaces, rational too). The exact surfaces stay with the mesh (MOD-14, `docs/exact-solids.md`). A face the STEP reader cannot cut is left out and counted. When no face can be cut the bounding box stands in |
| `IfcTriangulatedIrregularNetwork` | flags below zero (-1 hole, -2 void) leave the triangle out and the terrain becomes a 3D mesh so that the holes stay open; breakline flags are counted, not kept as lines. Works for IFC4X3 and for the release candidates (no `PnIndex`) |
| `IfcIShapeProfileDef` | with the fillet radius |

The boolean (`import/csg.rs`) is a binary space partition on polygons with a tolerance of 1e-7 file units. It needs closed operands (welded, T-junctions healed) and refuses more than 400,000 polygons or a tree deeper than 400. The result is healed, so it is closed and can be the operand of the next operation.

### Placement, sites and georeference

- A product far from the origin (projected coordinates used as model coordinates, 100 km and more) is moved to the origin when the document has no georeference; the shift (whole meters) goes into the georeference, together with `IfcMapConversion` if the file has one.
- A file with more than one `IfcSite` (a complex site with partial sites) puts the elements of each site on a layer with the name of the site, when they are in no storey. Parts of an `IfcElementAssembly` follow their assembly.
- `IfcGridPlacement` is read (crossing of two axes, offsets, optional direction). `IfcLinearPlacement` is counted in a note and the product keeps the place of its parent.
- `IfcMapConversionScaled`: the factors are read and reported as not applied. Property set names of the IFC2x3 georeference are matched without case.

### Export options

- `site_surface` writes the proposed surface of the first site model also as the shape of the `IfcSite` (Allplan reads an `IfcSite` with a face set as a terrain).

### Test corpus

`cargo test -p kadakas-ifc --test ifc_corpus` downloads CC BY 4.0 files of buildingSMART (`Certification-datasets`, `Community-Sample-Test-Files`) into `tests/data/ifc-public/` (ignored by git) from pinned commits, checks each against `crates/kadakas-ifc/tests/fixtures/ifc_public_sha256.json` (made by `tests/data/make_ifc_public_hashes.py`) and skips offline, with `KADAKAS_CORPUS=off`, and in CI unless `KADAKAS_CORPUS=fetch`. Files in Git LFS come from the media server; a pointer file is skipped with a message. The tests check, for every file, that nothing panics, the time, and the accounting; and for chosen files the counts of the scene, the georeference, the volumes (openings, revolved beam, CSG, advanced breps against IfcOpenShell and the tessellated twin), the TIN flags and the grid placement. `tests/behavior/test_ifc_solids.py` compares volumes with IfcOpenShell (known: its advanced brep volume of `basin-advanced-brep.ifc` is 2.7 times that of its own tessellated twin; Kadakas agrees with the twin).

### Left out, with the reason

- Surfaces of revolution and of extrusion inside an advanced brep (in IFC their curve is a profile, not a curve), `IfcIndexedPolyCurve` edges, and faces that the STEP reader cannot cut (a cylinder or cone whose loop goes around the axis without a seam edge, a torus). Reason: the STEP reader's face cutting has these limits (`docs/interop.md`, STEP section); the product is kept as its bounding box when nothing could be cut, and the report names the faces.
- `IfcSurfaceCurveSweptAreaSolid`, `IfcFixedReferenceSweptAreaSolid`, `IfcExtrudedAreaSolidTapered`: no test file and no user ask yet.
- Breaklines of a TIN as lines of the site model: a site model has no breakline type that matches the triangle flags.
- Grid factors of `IfcMapConversionScaled`, the double offset of a site placement on top of a map conversion, a CRS in feet: reported (factors) or untested for lack of a public file. IFC XML and JSON: see above.
- Memory of a file over 1 GiB: the cap stays (`max_file_mb`, at most 4096) and the message names the option; the index keeps 24 bytes per entity, the file stays on disk (`reader.rs`).

## DXF files (IOP-01, IOP-15, IOP-16, IOP-27)

This section is about real-world DXF files. The research with its sources is `docs/research/dxf-real-world.md`. The code is in `crates/kadakas-io/src/dxf`.

### What is read

- Text DXF and binary DXF, from release 12 (`AC1009`) to 2018 (`AC1032`). `report.release` names the release of the file.
- A binary file starts with `AutoCAD Binary DXF`. Release 12 writes group codes in one byte, newer releases in two. The reader finds the width from the first pair. A damaged binary file is refused with the byte where it is damaged and the advice to save a text DXF.
- Limits that protect the import: the file size (`max_bytes`), 40 million pairs, 20 million records, 1 MiB for one value, 12 levels of blocks, 5 million visits and `max_exploded` objects from blocks. Each limit gives a refusal or a line in the report. The memory is not bound by the file size alone: a pair takes about 70 bytes, so a text file needs up to 15 times its size, and a file at the pair limit needs about 3 GB.
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
| MTEXT, TEXT | The codes `%%c`, `%%d`, `%%p`, `%%nnn`, `\U+XXXX`, the caret codes (`^J`) and the MTEXT codes for fonts, heights, colors, stacks and paragraphs. The text equals the plain text of the ezdxf reader for the corpus files. |
| SOLID, TRACE | A filled polygon (fill = pen color). The corners are in the order 1, 2, 4, 3. |
| HATCH | Each boundary path is a closed polyline. A solid hatch is filled. An island is a white filled polyline with `data.hatch_island`. A gradient has a flat fill of its first color. A pattern hatch has a hatch fill of the document, made from its pattern lines (IOP-27, see Pattern hatches). |
| WIPEOUT | A white polygon with no line and `data.wipeout`, above what was drawn before it. |
| MESH | A `mesh3d` object (the control mesh). |
| LEADER | An open polyline. The arrowhead is not drawn. |
| DIMENSION, ARC_DIMENSION, ACAD_TABLE | The lines and text of the anonymous block. Without the block the entity is reported. The definition points on the layer `Defpoints` are reported. |
| SPLINE, ELLIPSE | Polylines within the tolerance. A note counts them. A spline without knots gets uniform knots, as in the ezdxf reader. |
| Entities in a tilted plane | CIRCLE, ARC, LWPOLYLINE, POLYLINE, SOLID, ELLIPSE and the paths of a HATCH become 3D polylines (the arbitrary axis algorithm of the DXF reference). A TEXT, ATTRIB or MTEXT in a tilted plane is placed flat by its projection, and a block reference in a tilted plane is placed in 3D (IOP-27, see Tilted planes). |
| IMAGE, 3DSOLID, BODY, REGION, SURFACE, MLEADER, XLINE, RAY, MLINE, OLE2FRAME, XREF | Reported by name with the reason. An IMAGE names its file, an XREF its path. |
| XCLIP of a block reference | The geometry of the block is cut by the polygon of the clip (IOP-27, see Block clips). |
| Transparency | Not applied. A note counts them. |

### Pattern hatches (IOP-27)

A pattern hatch has a pattern name, a scale, an angle and pattern lines. The pattern lines are stored in the file after the scale and the turn. The import uses them as they are.

- Each pattern line has an angle, a base point, an offset vector and a dash list. The offset is the vector from a line to the next line of the family.
- A hatch line of the document has an angle, a spacing across the lines, a stagger along them, an origin and a dash list. The import converts one into the other without loss. A family that runs to the other side gets a negative stagger.
- A dash is positive, a gap is negative and a dot is 0. A dot becomes a short dash. The gap after it is shorter by that much, so the period stays the same.
- The dot is 10 percent of the spacing or of the period, whichever is smaller. It is at least 0.1 mm and at most 40 percent of the smallest gap. At 1:100 a dot of a pattern at scale 100 is then 0.1 mm or more on paper.
- A dash list that starts with a gap keeps its gap. The export writes the same items again. A real short dash stays a dash.
- Some dash lists are not patterns: gaps only, dots only, or a period under 0.0001 mm. The line is left out and counted.
- The lines are in mm (`units: world`). The unit of the file, the scale of a block, a turn and a mirror (extrusion -Z) apply. A block with different scales in x and y works too, because a straight line stays straight.
- A hatch in a tilted block stays an outline, because a fill cannot lie in a tilted plane. It makes no resource.
- The origin of a family is reduced by whole steps. Rounding noise does not count. Hatches of one pattern then share one resource, wherever they lie.
- A resource that draws the same lines as one of the document is used again. Else a new hatch is made. It has the pattern name, with ` x2` or ` 30 deg` when the lines are the standard pattern at another scale or turn. A name that is taken gets ` (2)`.
- An entity with a pattern name and no pattern lines gets the lines of the standard pattern of that name from the table. The scale (code 41) and the angle (code 52) apply. The table has 106 patterns: `ANSI31` to `ANSI38`, `AR-*`, `EARTH`, `GRASS`, `GRAVEL`, `STEEL`, `NET` and more.
- A double hatch (code 77) adds the lines turned by 90 degrees. This is done for a table pattern and for one family of lines. A file that holds both halves is not doubled again.
- The boundary loops are polylines with the hatch fill. An island is a white filled polyline on top, as for a solid hatch. The outer loop is always drawn before the islands in it. The outer loop knows its islands (`data.hatch_islands`).
- A pattern keeps at most 128 lines. A note counts the lines that were left out. A line whose copies would lie on each other is left out and counted. The import reads at most 10,000 lines from one entity, and a note counts the rest.
- All the families of a pattern together may make 100,000 lines in the box of the hatch. A denser pattern stays an outline, and a note says so. The drawing engine would cut it without a word.
- The table of the standard patterns is `crates/kadakas-io/src/dxf/hatch/std_patterns.txt`. It is made by `tests/data/make_std_patterns.py` from the metric table of ezdxf (MIT licence).

The export writes a hatch fill as a HATCH entity.

- The object keeps its outline entity. The HATCH has the boundary path, the name and the pattern lines. A landscape area with holes writes the holes as more paths. The islands of an imported pattern hatch are holes of one HATCH too. Real files of AutoCAD flag the outer path as external (3 here) and an island as outermost (18).
- A fill that is a standard pattern gets its name (`ANSI31`). The scale goes in code 41 and the turn in code 52. The turn must be a multiple of 15 degrees. A pattern of one family of lines is named at the turn 0 only. A resource that starts with the standard name (`ANSI31 x2 90 deg`, made by the import) keeps that name at any turn.
- Any other fill gets its own name in capital letters, as a custom pattern.
- The numbers follow the unit, the origin and the scale of the layer (paper units).
- A fill with a color or a weight on one of its lines stays plain LINE entities. So does a tile. Release 12 has no HATCH. It writes the outline, and a note counts the objects.
- The report says how many HATCH entities were written, by name.

### Block clips (IOP-27)

A block reference with a clip (XCLIP) has an extension dictionary. It leads to a SPATIAL_FILTER object. Kadakas explodes blocks, so it has no symbol with a stencil. The import cuts the geometry of each entity of the block by the polygon, in the plane.

- The polygon is in the space of the reference. The inverse matrix of the filter (the last 24 numbers of code 40, the first 12) gives it in the coordinates of the block. Two points are a rectangle by its corners.
- The turn, the scale and the place of the reference apply, also in arrays (every copy is cut) and in nested blocks (all clips of the references around apply).
- A line or a polyline is cut at the edge of the polygon. The cut points are exact. A concave polygon can cut a line in many pieces.
- A line that lies on the edge of the polygon stays.
- A curve (a circle, an arc, a bulge) that lies entirely inside stays exact. A curve that is cut becomes chords within the tolerance. A note counts these.
- A filled shape (a solid, a hatch loop, a wipeout) becomes the polygon boolean of the shape and the clip. The pattern of a hatch stays. A hole becomes a white island, as in a hatch.
- A text, a point and a 3D point stay or go whole by their point. A mesh stays or goes whole by its vertices. A note counts these.
- An entity that is entirely outside is not imported. Its line in `report.skipped` says `outside the clip (XCLIP) of its block reference`, and the accounting stays exact.
- An inverted clip keeps the part outside. The filter holds a polygon, and an XRECORD in its dictionary holds the part that is cut away. The filter polygon is the outer one or the part that is cut away, as ezdxf says. The smaller box is the part that is cut away. The outer area is the box of the compare polygon in the XRECORD, else of the larger polygon.
- A clip that is switched off (code 71 is 0), or that cannot be read, shows the whole block. A note counts them.
- A front or back clip plane (3D) is not applied. Only the polygon is. A note counts them.
- The clip is in the plane. A 3D polyline is cut by its plan, and its height is interpolated.
- A circle or an arc is cut into chords within the tolerance, at most 20,000. A note says when the cap holds.
- A clip may have 10,000 points. A longer one is refused with a note, and the whole block shows. A clip of 10,000 points cuts 20,000 lines in a few seconds. The edges are kept in slabs.
- The clip of a block cuts the geometry in the plane of the block, before the tilt of a block inside it. A tilted block inside a clipped block is not cut by the plan of the clip.

### Twist and perspective of viewports (IOP-27)

A viewport of the sheet has a `twist` and a `perspective` flag.

- The twist (code 51) is the angle that turns a plan view about the center of the view. A positive angle turns the model counterclockwise on the sheet, as the transformation of the ezdxf reader does. The view center (code 12 and 22) is in the display axes. The model point at the middle of the frame is the target plus the turned back view center.
- The viewport object keeps the twist in degrees from -180 to 180. `viewport.create` and `viewport.update` take `twist`. The window of the app has a field for it.
- The sheet draws a twisted plan view: every point, arc, hatch and symbol follows the turn. Text stays upright. A raster picture cannot follow the turn, because it has straight sides on the sheet. A map underlay and the picture in a table cell are not drawn. A note on the page says so. The reply of `viewport.create`, `viewport.update` and `viewport.get` has the same text in `warnings`, and the window shows it.
- `fit` makes room for the turned content. It turns the box of each object, so the fit is as tight as the boxes allow.
- The reply has `warnings` for a twist on a 3D view or a section, and for the perspective flag on a plan. The values are kept.
- The notes of a pattern on the sheet stay inside the frame, also at a twist of 180 degrees.
- A viewport keeps the fields that a newer version added (`extra`), like a saved view.
- A 3D view and a section are not turned. The camera of the 3D view has no roll. A 3D viewport keeps the twist and writes it back, and a note counts it.
- The perspective flag (flag value 1 of code 90) gives a 3D viewport a perspective camera. It shows the same size at the target as the parallel view does. The lens length (code 42) is not read, and the field of view is the one of Kadakas. A note says so.
- A plan has no perspective. A plan viewport keeps the flag and writes it back, and a note counts it.
- The export writes the twist from 0 to 360 and the flag. The target is the middle of the view and the view center is 0.

### Tilted planes (IOP-27)

An entity lies in a plane when its extrusion direction (codes 210, 220 and 230) is not straight up or down. The axes of the plane come from the arbitrary axis algorithm of the DXF reference.

- A block reference in a tilted plane is placed in 3D. The turn, the scale and the insertion point apply in the plane. The point of the insertion is the pivot of the tilt.
- Lines, polylines, circles and arcs of the block become 3D polylines. Curves are cut within the tolerance. 3D polylines, points and meshes move in 3D.
- A fill cannot lie in a tilted plane. The shape keeps its outline, and a note counts the fills that were lost. The white islands of hatches are left out and counted.
- A flat shape keeps its elevation as its height above the plane of the block. That is code 30, or 38 for a LWPOLYLINE.
- Nested references work. A tilted reference inside a block that turns or scales is tilted inside the block first.
- An object of Kadakas has no work plane. A text has a point, a height and a turn on the drawing plane, and a symbol has the same. So a text in a tilted plane is placed flat by its projection onto the drawing plane.
- The point of the text is the projected point. The turn is the turn of the projected baseline. The height is the height times the length of the projected height vector. A plane tilted by 60 degrees gives half the height.
- A plane that stands nearly upright (a wall) would give a height of nothing. Then the text keeps its full height, and the turn follows the wall.
- A plane that is seen from the back would mirror the text. A text cannot be mirrored, so the text is placed as it is and a note counts it.
- A note counts the texts, the texts on upright planes and the texts seen from the back. The loss is the 3D position of the text. The export writes the text flat.
- A block reference whose container cannot be inverted (a scale of 0) is skipped with that reason.

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

A viewport that is off, has no size or is not valid is reported. A view direction that is not a standard view uses the nearest standard view, and a note says which. The twist and the perspective view are in the next part.

The export writes the reverse. A sheet layer is a layout with the paper, the viewport of the paper, the viewports, the annotations of the viewports, the title block (lines and text) and the objects of the sheet. The first layout is in the section ENTITIES with the flag `67 1`, the others in their blocks `*Paper_Space0` and so on. The option `sheets` is `false` to leave sheets out. Release 12 has no layouts. The update of a drawing (IOP-23) does not import paper space.

The default export writes sheet layers as layouts (IOP-16), and the report says which (`N sheet layers were written as layouts: names`). Set `sheets` to `false` for the old behavior.

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

- Public sample files of ezdxf (MIT licence) and GDAL (MIT style licence) are fetched with `curl` from a fixed commit into `tests/data/dxf-public/`. Git ignores that folder. Each file is checked against its SHA-256 (`crates/kadakas-io/tests/fixtures/dxf_public_sha256.json`). The tests skip when there is no network. An HTTP error while the network works fails the test. `KADAKAS_CORPUS=off` fetches nothing. In CI (the variable `CI` is set) nothing is fetched unless `KADAKAS_CORPUS=fetch` opts in. Files in the cache are used in every case. No LibreCAD file (GPL) is used.
- Every corpus file goes through the import. It must not panic, it must end in time, and its report must account for every entity. The counts of entities, layouts, viewports and MTEXT texts are compared with the ezdxf reader (`crates/kadakas-io/tests/fixtures/dxf_public_counts.json`, made by `tests/data/make_public_counts.py`).
- Our own small files are in `tests/data/dxf-own/` (made by `tests/data/make_own_files.py`). They run offline.
- The tests for damaged and hostile files are in `crates/kadakas-io/tests/dxf_hostile.rs`.
- The tests of IOP-27 are `crates/kadakas-io/tests/dxf_hatch.rs` (own entities), `dxf_leftovers.rs` (public files, with facts from the ezdxf reader in `fixtures/dxf_iop27.json`, made by `tests/data/make_iop27_facts.py`) and `tests/behavior/test_dxf_hatch.py`. The last one compares the length of the pattern lines with the lines that `ezdxf.render.hatching` draws for the same hatch, for nine patterns.

### Not done

- Transparency is not applied.
- The islands of a pattern hatch are white filled outlines on top of the hatch. A polyline has no holes, so the lines of the pattern still run under the island. Outputs that do not paint over it show them. The export writes the island as its own polyline. It also writes it as a hole of the HATCH when the outer loop still knows it.
- A hatch in a tilted plane has no fill, only its boundary as 3D polylines.
- A pattern hatch with an associative boundary (code 330 to the entities) loses the association. The boundary becomes polylines.
- A twist does not turn a 3D view or a section. A raster picture is not drawn in a twisted plan view. The lens length of a perspective view is not read.
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
| STEP reader (Part 21, products, faces, tessellation) | `crates/kadakas-step` (`part21`, `read_brep`, `tess`, `mesh2d`, `assembly`) |
| Exact surfaces and curves, the payload of a mesh | `crates/kadakas-core/src/exact` |
| `mesh3d.exact_info`, `mesh3d.retessellate` | `crates/kadakas-api/src/mesh_api/exact.rs` |
| The form of the windows | `crates/kadakas-ui-core/src/mesh_import.rs` |
| The windows | `crates/kadakas-app/src/io_dialog.rs` and `crates/kadakas-modeler/src/interop.rs` |

The parsers are written for Kadakas. No new third-party crate was added, so the About list has nothing new. The notices only list the new workspace crate.

## Tests

- `kadakas-meshio`: unit tests for every reader and the STL writer, damaged and hostile files, and a property test with random bytes.
- `kadakas-core`: tests of the conversion with cubes, spheres, grids with holes, welds, slivers, limits and soften counts, a property test on random terrain and spheres, and the command tests (`tests/mesh_to_surface.rs`).
- `kadakas-api/tests/meshio.rs`: a U shaped prism whose outline has corners on a line (watertight through every format), the place of the model after a round trip, hostile glTF files, round trips of a closed slab with a hole through OBJ, STL, ASCII STL, glTF and GLB (the same volume, area and face count), a property test on random prisms and terrain, options, guards, refusals and the path policy.
- `kadakas-step`: the Part 21 reader, the test files, the hostile files and a property test (see STEP files).
- `kadakas-api/tests/step.rs`: the import through the API, the report, the exact data, the two new methods, a move and a turn, stale data, the size caps and a save and load.
- `kadakas-ifc` and `kadakas-io`: the entities of the surface export.
- `tests/behavior/test_meshio.py`: the checks of IfcOpenShell (the IFC of surface models), ezdxf (the DXF) and glTF validation. They skip when the tool is missing.

## Not done

- STEP export, PMI, GD&T and kinematics. Textures of STEP files. See the list in the STEP section.
- Textures and materials of OBJ and glTF are not read. Only the base color is kept.
- Colors of single triangles of an STL file, vertex colors and normals are not read.
- glTF with Draco, Meshopt or quantized data, and sparse accessors, are refused.
- A DXF MESH entity with a subdivision level above 0 is read as its control mesh.
- Component instances are not exported to IFC. Explode them first. DXF writes them (MOD-13).
