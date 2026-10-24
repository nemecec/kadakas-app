# Exact solids (stage 1 of the hybrid exact and faceted model)

A STEP file holds the true surfaces of a product: a cylinder has a radius, a plane has a normal. Kadakas draws triangles, so the import cuts the faces into triangles. The import cuts the faces into triangles. Dropping the surfaces loses the dimensions. So each imported `mesh3d` object keeps its exact surfaces next to its triangles. This page describes that payload. The code is in `crates/kadakas-core/src/exact`.

## The rule

- The triangles stay the source for drawing and for every current tool.
- The payload is optional data on the object. A tool that does not know it ignores it.
- A payload is never wrong silently. If the triangles change, the payload is stale and every reader says so.

## Where it lives

`Mesh3d` has the optional field `exact`. It is left out of the JSON when there is no payload. A file from an older program has no such field and loads as before. `ExactSolid.extra` holds the fields of a later version. A program writes them back unchanged.

The payload is in millimeters, in the frame of the vertices of the mesh. A move or a turn of the object moves the payload too.

## The form

```json
{
  "version": 1,
  "product": "Bracket",
  "unit_mm": 25.4,
  "tolerance_mm": 0.05,
  "angle_deg": 15,
  "vertices": [[0, 0, 0], [10, 0, 0]],
  "edges": [{"start": 0, "end": 1, "same_sense": true, "curve": {"kind": "line", "o": [0, 0, 0], "d": [10, 0, 0]}}],
  "shells": [{"closed": true, "outward": null}],
  "faces": [{"shell": 0, "surface": {"kind": "plane", "f": {"matrix3": {}, "translation": []}}, "same_sense": true, "loops": [{"outer": true, "items": [{"kind": "edge", "edge": 0, "forward": true}]}], "color": [255, 0, 0, 255], "source": 35}],
  "mesh_hash": 123456789
}
```

- `vertices`, `edges`, `shells` and `faces` are lists. Numbers in other lists point into them.
- A face has a surface, loops and an optional colour.
- `same_sense` says if the normal of the face is the normal of the surface or the opposite.
- `flip` turns the face around (an oriented face or shell). It is left out when false.
- A loop is a list of edge uses (`edge`, `forward`) or of polygon corners (`vertex`). `outer` marks the outer bound.
- Surfaces: `plane`, `cylinder`, `cone`, `sphere` and `torus` have a frame `f` with unit axes and their sizes.
- `revolution` has a curve, a point and an axis. `extrusion` has a curve and a direction.
- `spline` has degrees, knots, control points and optional weights.
- Curves: `line`, `circle`, `ellipse`, `spline`, `poly`, `trimmed` and `composite`. An edge without a curve is a straight line.
- A face without a surface is the plane through its loop (a face of a faceted solid).
- `mesh_hash` is the fingerprint of the vertices and triangles that the payload belongs to.

## What is kept and what is not

Kept:

- the faces with their surface type and sizes
- the edges with their curves
- the loops with holes and the orientation flags
- the colours of faces
- the product name and the unit of the file (for the report)

Not kept:

- the assembly structure (each placed instance is its own object)
- pcurves
- tessellated items that came as triangles
- curve sets (they are polylines)
- PMI and everything that is not geometry
- faces that the importer failed to read (they are in the report)

## Caps

An import measures the payload and keeps it only when it is small enough. The sizes come from `ExactSolid::approx_bytes`, a count of numbers that is about 1.8 times the real JSON.

| Measured | Payload |
|---|---|
| a box of 6 faces | 3.6 KB of JSON, `approx_bytes` 6.7 KB |
| a cylinder with 2 caps | 1.3 KB |
| a plate with a hole (7 faces) | 4.3 KB |
| a B-spline patch with 8 by 8 control points | about 12 KB |
| a bicubic patch with 400 by 400 control points | about 9.6 MB by `approx_bytes` |

The STEP text of a part is about as long as its payload. One object keeps at most 8 MiB of `approx_bytes`. That is about 4.5 MB of JSON, or some 6,000 planar faces. One document keeps at most 64 MiB in all. The triangles of such a part take 10 to 30 MB. So the payload costs less than the mesh.

Over a cap, the mesh keeps its triangles and loses the payload. The report says so: `report.exact.without` lists each part with the reason and the size. The option `keep_exact` (default true) turns the payload off for a whole import.

## Moves, turns, scale, mirror

`Mesh3d::translate_xy`, `rotate_z` and `transform` move the payload and the vertices together and write the new `mesh_hash`. A uniform scale scales the radii and the tolerance. A mirror turns the faces around (`same_sense`). A stretch or a shear cannot be followed, so the payload is dropped and the call returns a note. A payload that was stale before stays stale.

## Stale data

The payload is stale when `mesh_hash` differs from the fingerprint of the current triangles. No tool needs to do anything for this. A tool that edits the triangles changes the fingerprint.

- `mesh3d.exact_info` still lists a stale payload, with `state: "stale"` and a note.
- `mesh3d.retessellate` refuses a stale payload with `error_id: exact_stale`.
- Object Info says that the triangles were edited and that the exact data no longer fits.
- A conversion to a surface model drops the payload (a surface model has its own faces). The import report counts this in `report.exact.lost_to_surface`.

## Uses now

1. Object Info shows "Exact data kept: 12 faces (cylinder x 4, plane x 8)".
2. `mesh3d.exact_info {id}` returns the true radii, axes and normals in millimeters for dimensioning.
3. `mesh3d.retessellate {id, tolerance, angle?}` cuts the triangles again from the surfaces, as one undo step. A finer tolerance gives a rounder cylinder. The tests compare the volume with the analytic value.

## How stages 2 and 3 build on it

- Stage 2 builds edit tools that keep exactness (offset a face, change a radius) and a STEP export. The payload has the topology for it: shared edges, loops and orientation.
- Stage 3 makes the payload the source for some objects. The triangles become a cache that `retessellate` rebuilds. The `mesh_hash` says when the cache is current.
- Booleans on exact solids need to move faces between objects. `ExactSolid::merge` and `retain_faces` already join and split the payload by face.
- Version 1 of the form stays readable. New fields go in `extra` first.
