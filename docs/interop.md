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

A Y up file turns so that Z is up. The point (x, y, z) of the file becomes (x, -z, y). This is the inverse of the glTF and OBJ export of Kadakas, so a round trip returns the same place. The units are `mm`, `cm`, `m`, `km`, `inch`, `foot`, `yard` and `mile`. A glTF file with a `unit` or an `up_axis` in the call is refused, because the format fixes both.

The result lists the unit, the axis and the box around the model in millimeters (`bounds_mm`), so a wrong guess shows at once. The option `center` moves the middle of the box to x = 0 and y = 0 and the lowest point to z = 0.

### Parts, colors and names

A file becomes one object for each pair of an object name and a material. The option `join` makes one object. A file with more than 5,000 such pairs has its parts joined by color first. If that is not enough the import is refused with the count.

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

The faces keep the winding of the triangles, so the front is the outside of a closed mesh. The answer has `softened_edges` and a `report` with the counts: `source_triangles`, `welded_vertices`, `degenerate_triangles`, `duplicate_triangles`, `joined_slivers`, `vertices`, `edges`, `faces`, `faces_with_holes`, `merged_triangles`, `unmerged_regions`, `healed_vertices`, `border_edges`, `nonmanifold_edges`, `closed`, `area_mm2` and `volume_mm3` for a closed mesh. `dry_run: true` converts a copy and changes nothing.

## Soften edges at import

Soft edges are not drawn and shade smoothly. A mesh from a file has many edges that a person does not want to see, for example the sides of a cylinder. The option is off by default, so the edges of the source stay as they are.

For an agent:

- `soften_below_angle` in the options of `import.file` (degrees, 0 to 180). It asks for the surface model, because a mesh has no edges to soften. `to_surface: true` converts without softening.
- The result always has `softened_edges`. It is 0 when nothing was softened.
- A part that does not fit the limits stays a mesh with no edge softened. `report.to_surface.refused` lists it with the counts, and a note says so.
- `preview: true` reads the file, converts it and answers with the count, and changes nothing.
- The IFC import takes the same options (`to_surface`, `soften_below_angle` and `surface_tolerance`) and converts the meshes it made.

For a person, the import window of the app and of the modeler (File > Import) asks "Soften edges under an angle?" for a mesh file. It is off by default. The angle is 20 degrees. When it is on, the window shows how many edges would be softened. The answer is kept in the prefs of the program (`import_soften` in `prefs.json` of the app and in `modeler.json` of the modeler) and comes back at the next import.

## Export

### STL

`export.file` with `format: "stl"` writes the triangles of all 3D objects, or of the objects in `objects`. The options are `encoding` (`binary` by default, or `ascii`), `units` (`mm` by default, or `m`), `y_up` (false by default) and `name`. STL has no colors, materials or lines. The result has `open_models`, the number of surface models that are not closed solids, and a note, because a printer needs closed solids.

### OBJ, glTF and GLB

These formats are written by `kadakas-x3d` (3D-10 and 3D-11). They write surface models and meshes like every other 3D object. Y up and meters are the default. The options `units` and `y_up` change that. A surface model is written as the triangles of its faces. Soft edges do not show in these files.

### IFC

A surface model is written as `IfcFacetedBrep` when it is a closed solid. The faces keep their holes (`IfcFaceBound`), so a face is not cut into triangles. Every other face is written in an open shell of an `IfcShellBasedSurfaceModel`, and a note says how many faces. A model that has both solids and open faces is one surface model, with its solids as closed shells. Hidden faces are left out, so a solid with a hidden face is an open shell. IFC2x3, IFC4 and IFC4x3 all take the same entities. Component instances are skipped with the advice to explode them first.

### DXF

A surface model is written as a polyface mesh by default. The option `mesh_entity` is `polyface`, `face3d` (one 3DFACE per triangle) or `mesh` (one MESH entity, a subdivision mesh at level 0 with triangle faces). The heights include the elevation of the layer. A hidden face is left out. A DXF file with a MESH entity needs a program that reads DXF 2010 or later. Kadakas does not read the MESH entity.

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
- `kadakas-api/tests/meshio.rs`: round trips of a closed slab with a hole through OBJ, STL, ASCII STL, glTF and GLB (the same volume, area and face count), a property test on random prisms and terrain, options, guards, refusals and the path policy.
- `kadakas-ifc` and `kadakas-io`: the entities of the surface export.
- `tests/behavior/test_meshio.py`: the checks of IfcOpenShell (the IFC of surface models), ezdxf (the DXF) and glTF validation. They skip when the tool is missing.

## Not done

- Textures and materials of OBJ and glTF are not read. Only the base color is kept.
- Colors of single triangles of an STL file, vertex colors and normals are not read.
- glTF with Draco, Meshopt or quantized data, and sparse accessors, are refused.
- The DXF MESH entity is written and not read.
- Component instances are not exported to IFC and DXF. Explode them first.
- A face that is not convex and has a corner on the line between two others is triangulated with a triangle of no area. The 3D scene does not write such a triangle. So after OBJ, STL or glTF the face can come back as two. Volume and area stay the same.
