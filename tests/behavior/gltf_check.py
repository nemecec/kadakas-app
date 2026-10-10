"""Reads a glTF, GLB or OBJ file with pygltflib or trimesh and prints facts as JSON on the last line.

Run it with: uv run --no-project --with pygltflib --with trimesh --with numpy python -I gltf_check.py <file>
"""

import json
import sys
from pathlib import Path


def gltf_facts(path):
    import pygltflib

    g = pygltflib.GLTF2().load(str(path))
    nodes = g.nodes
    meshes_used = [n.mesh for n in nodes if n.mesh is not None]
    # Triangles per mesh, from the index accessors.
    tris = 0
    for n in nodes:
        if n.mesh is None:
            continue
        for p in g.meshes[n.mesh].primitives:
            if p.mode in (None, 4):
                tris += g.accessors[p.indices].count // 3
    return {
        "version": g.asset.version,
        "nodes": len(nodes),
        "meshes": len(g.meshes),
        "materials": len(g.materials),
        "node_names": [n.name for n in nodes],
        "extras": [n.extras for n in nodes],
        "shared_meshes": len(meshes_used) - len(set(meshes_used)),
        "triangles_by_nodes": tris,
        "extensions_used": g.extensionsUsed,
        "scene_extensions": g.scenes[0].extensions if g.scenes else None,
    }


def trimesh_facts(path):
    import trimesh

    scene = trimesh.load(str(path), force="scene")
    geoms = list(scene.geometry.values())
    # Every instance counts: the scene graph applies the transform of each node.
    placed = scene.dump(concatenate=False)
    bounds = scene.bounds.tolist() if len(geoms) else None
    return {
        "geometries": len(geoms),
        "triangles": int(sum(len(getattr(g, "faces", [])) for g in placed)),
        "bounds": bounds,
        "extents": scene.extents.tolist() if len(geoms) else None,
        "watertight": [bool(getattr(g, "is_watertight", False)) for g in geoms],
    }


def main():
    path = Path(sys.argv[1])
    out = {}
    if path.suffix.lower() in (".gltf", ".glb"):
        out["gltf"] = gltf_facts(path)
    out["trimesh"] = trimesh_facts(path)
    print(json.dumps(out))


main()
