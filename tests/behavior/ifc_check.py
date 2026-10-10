"""Reads an IFC file with IfcOpenShell and prints facts as JSON. The behavior test runs it with
`uv run --no-project --with ifcopenshell python -I ifc_check.py <file>`."""

import json
import sys

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.util.element as element
import ifcopenshell.util.unit as unit
import ifcopenshell.validate


def spatial_tree(entity):
    node = {"type": entity.is_a(), "name": entity.Name, "children": []}
    for rel in getattr(entity, "IsDecomposedBy", []) or []:
        node["children"] += [spatial_tree(o) for o in rel.RelatedObjects]
    for rel in getattr(entity, "ContainsElements", []) or []:
        node["children"] += [{"type": e.is_a(), "name": e.Name} for e in rel.RelatedElements]
    return node


def main(path):
    model = ifcopenshell.open(path)
    logger = ifcopenshell.validate.json_logger()
    ifcopenshell.validate.validate(model, logger)

    counts = {}
    for e in model:
        counts[e.is_a()] = counts.get(e.is_a(), 0) + 1

    products = []
    settings = ifcopenshell.geom.settings()
    for p in model.by_type("IfcProduct"):
        if p.is_a("IfcSite") or p.is_a("IfcBuilding") or p.is_a("IfcBuildingStorey"):
            continue
        try:
            triangles = len(ifcopenshell.geom.create_shape(settings, p).geometry.faces) // 3
        except RuntimeError:
            # Text and points have no faces. Solids must always work.
            if not p.is_a("IfcAnnotation"):
                raise
            triangles = 0
        material = element.get_material(p)
        products.append({
            "type": p.is_a(),
            "name": p.Name,
            "guid": p.GlobalId,
            "predefined": getattr(p, "PredefinedType", None),
            "object_type": p.ObjectType,
            "triangles": triangles,
            "psets": element.get_psets(p),
            "material": material.is_a() if material else None,
            "container": element.get_container(p).is_a(),
            "layers": [a.Name for r in p.Representation.Representations for a in (r.LayerAssignments or [])],
        })

    guids = [e.GlobalId for e in model.by_type("IfcRoot")]
    has_map = model.schema != "IFC2X3"
    conversion = model.by_type("IfcMapConversion") if has_map else []
    crs = model.by_type("IfcProjectedCRS") if has_map else []
    site = model.by_type("IfcSite")[0]
    project = model.by_type("IfcProject")[0]
    out = {
        "schema": model.schema,
        "validation": [s["message"] for s in logger.statements],
        "counts": counts,
        "duplicate_guids": len(guids) - len(set(guids)),
        "psets": sorted({s.Name for s in model.by_type("IfcPropertySet")}),
        "products": products,
        "tree": spatial_tree(project),
        "unit_scale": unit.calculate_unit_scale(model),
        "project": project.Name,
        "map_conversion": conversion[0].get_info(include_identifier=False, recursive=False) if conversion else None,
        "crs": crs[0].Name if crs else None,
        "ref_latitude": list(site.RefLatitude) if site.RefLatitude else None,
        "ref_longitude": list(site.RefLongitude) if site.RefLongitude else None,
    }
    if out["map_conversion"]:
        out["map_conversion"].pop("SourceCRS", None)
        out["map_conversion"].pop("TargetCRS", None)
    print(json.dumps(out, default=str))


main(sys.argv[1])
