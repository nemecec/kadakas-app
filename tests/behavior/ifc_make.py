"""Writes IFC test files with IfcOpenShell (the independent reference implementation). The behavior tests run it with
`uv run --no-project --with ifcopenshell python -I ifc_make.py <folder>` and import the files with Kadakas."""

import sys
from pathlib import Path

import ifcopenshell
import ifcopenshell.guid


def new_model(schema="IFC4"):
    """A project in millimeters with a site and one storey, and the body context."""
    f = ifcopenshell.file(schema=schema)
    e = f.create_entity
    guid = ifcopenshell.guid.new
    origin = e("IfcCartesianPoint", Coordinates=(0.0, 0.0, 0.0))
    wcs = e("IfcAxis2Placement3D", Location=origin)
    ctx = e("IfcGeometricRepresentationContext", ContextType="Model", CoordinateSpaceDimension=3, Precision=1e-5, WorldCoordinateSystem=wcs)
    body = e("IfcGeometricRepresentationSubContext", ContextIdentifier="Body", ContextType="Model", ParentContext=ctx, TargetView="MODEL_VIEW")
    units = e("IfcUnitAssignment", Units=[e("IfcSIUnit", UnitType="LENGTHUNIT", Prefix="MILLI", Name="METRE"),
                                          e("IfcSIUnit", UnitType="AREAUNIT", Name="SQUARE_METRE"),
                                          e("IfcSIUnit", UnitType="VOLUMEUNIT", Name="CUBIC_METRE")])
    project = e("IfcProject", GlobalId=guid(), Name="ifcopenshell project", RepresentationContexts=[ctx], UnitsInContext=units)
    site_place = e("IfcLocalPlacement", RelativePlacement=wcs)
    site = e("IfcSite", GlobalId=guid(), Name="Site", ObjectPlacement=site_place, CompositionType="ELEMENT")
    storey_place = e("IfcLocalPlacement", PlacementRelTo=site_place, RelativePlacement=e("IfcAxis2Placement3D", Location=e("IfcCartesianPoint", Coordinates=(0.0, 0.0, 0.0))))
    storey = e("IfcBuildingStorey", GlobalId=guid(), Name="Level 0", ObjectPlacement=storey_place, CompositionType="ELEMENT", Elevation=0.0)
    e("IfcRelAggregates", GlobalId=guid(), RelatingObject=project, RelatedObjects=[site])
    e("IfcRelAggregates", GlobalId=guid(), RelatingObject=site, RelatedObjects=[storey])
    return f, body, storey, storey_place


def pt(f, *c):
    return f.create_entity("IfcCartesianPoint", Coordinates=tuple(float(x) for x in c))


def axis(f, x=0.0, y=0.0, z=0.0):
    return f.create_entity("IfcAxis2Placement3D", Location=pt(f, x, y, z))


def extrusion(f, profile, depth, x=0.0, y=0.0, z=0.0):
    return f.create_entity("IfcExtrudedAreaSolid", SweptArea=profile, Position=axis(f, x, y, z),
                           ExtrudedDirection=f.create_entity("IfcDirection", DirectionRatios=(0.0, 0.0, 1.0)), Depth=float(depth))


def product(f, cls, name, body, storey, storey_place, items, rep_type="SweptSolid", at=(0.0, 0.0, 0.0), **attrs):
    guid = ifcopenshell.guid.new
    rep = f.create_entity("IfcShapeRepresentation", ContextOfItems=body, RepresentationIdentifier="Body", RepresentationType=rep_type, Items=items)
    shape = f.create_entity("IfcProductDefinitionShape", Representations=[rep])
    place = f.create_entity("IfcLocalPlacement", PlacementRelTo=storey_place, RelativePlacement=axis(f, *at))
    el = f.create_entity(cls, GlobalId=guid(), Name=name, ObjectPlacement=place, Representation=shape, **attrs)
    f.create_entity("IfcRelContainedInSpatialStructure", GlobalId=guid(), RelatedElements=[el], RelatingStructure=storey)
    return el


def wall_with_opening(out):
    f, body, storey, sp = new_model()
    e = f.create_entity
    guid = ifcopenshell.guid.new
    rect = e("IfcRectangleProfileDef", ProfileType="AREA", XDim=5000.0, YDim=250.0)
    wall = product(f, "IfcWall", "Garden wall", body, storey, sp, [extrusion(f, rect, 2400, 0, 0, 0)], PredefinedType="STANDARD")
    hole = e("IfcRectangleProfileDef", ProfileType="AREA", XDim=1000.0, YDim=400.0)
    opening = product(f, "IfcOpeningElement", "Gate", body, storey, sp, [extrusion(f, hole, 2000, 0, 0, 0)], PredefinedType="OPENING")
    e("IfcRelVoidsElement", GlobalId=guid(), RelatingBuildingElement=wall, RelatedOpeningElement=opening)
    pset = e("IfcPropertySet", GlobalId=guid(), Name="Pset_WallCommon", HasProperties=[
        e("IfcPropertySingleValue", Name="IsExternal", NominalValue=e("IfcBoolean", True)),
        e("IfcPropertySingleValue", Name="FireRating", NominalValue=e("IfcLabel", "EI30")),
    ])
    e("IfcRelDefinesByProperties", GlobalId=guid(), RelatedObjects=[wall], RelatingPropertyDefinition=pset)
    f.write(str(out / "wall_opening.ifc"))


def slab_with_voids(out):
    f, body, storey, sp = new_model()
    e = f.create_entity

    def poly(points):
        pts = [pt(f, *p) for p in points]
        return e("IfcPolyline", Points=pts + [pts[0]])

    outer = poly([(0, 0), (8000, 0), (8000, 5000), (0, 5000)])
    inner = [poly([(1000, 1000), (2500, 1000), (2500, 2500), (1000, 2500)]), poly([(5000, 1500), (7000, 1500), (7000, 3500), (5000, 3500)])]
    profile = e("IfcArbitraryProfileDefWithVoids", ProfileType="AREA", OuterCurve=outer, InnerCurves=inner)
    product(f, "IfcSlab", "Perforated terrace", body, storey, sp, [extrusion(f, profile, 150, 0, 0, 300)], PredefinedType="BASESLAB")
    f.write(str(out / "slab_voids.ifc"))


def mapped_bollards(out):
    f, body, storey, sp = new_model()
    e = f.create_entity
    guid = ifcopenshell.guid.new
    rect = e("IfcRectangleProfileDef", ProfileType="AREA", XDim=200.0, YDim=200.0)
    source = e("IfcShapeRepresentation", ContextOfItems=body, RepresentationIdentifier="Body", RepresentationType="SweptSolid", Items=[extrusion(f, rect, 900)])
    rmap = e("IfcRepresentationMap", MappingOrigin=axis(f), MappedRepresentation=source)
    for k, (x, y) in enumerate([(1000, 1000), (3000, 1000), (5000, 1000), (7000, 1000)]):
        op = e("IfcCartesianTransformationOperator3D", LocalOrigin=pt(f, x, y, 0), Scale=1.0)
        item = e("IfcMappedItem", MappingSource=rmap, MappingTarget=op)
        product(f, "IfcFurnishingElement", f"Bollard {k + 1}", body, storey, sp, [item], rep_type="MappedRepresentation")
    f.write(str(out / "mapped_bollards.ifc"))


def terrain_and_tree(out):
    f, body, storey, sp = new_model("IFC4")
    e = f.create_entity
    coords = e("IfcCartesianPointList3D", CoordList=[(0.0, 0.0, 0.0), (10000.0, 0.0, 500.0), (10000.0, 10000.0, 1000.0), (0.0, 10000.0, 500.0)])
    tin = e("IfcTriangulatedFaceSet", Coordinates=coords, Closed=False, CoordIndex=[(1, 2, 3), (1, 3, 4)])
    product(f, "IfcGeographicElement", "Ground", body, storey, sp, [tin], rep_type="Tessellation", PredefinedType="TERRAIN")
    circle = e("IfcCircleProfileDef", ProfileType="AREA", Radius=2000.0)
    canopy = extrusion(f, circle, 3000, 0, 0, 2000)
    trunk = extrusion(f, e("IfcCircleProfileDef", ProfileType="AREA", Radius=150.0), 2000)
    product(f, "IfcGeographicElement", "Alder", body, storey, sp, [trunk, canopy], at=(4000.0, 3000.0, 0.0), PredefinedType="USERDEFINED", ObjectType="VEGETATION")
    f.write(str(out / "terrain_tree.ifc"))


def ifc2x3_wall(out):
    f, body, storey, sp = new_model("IFC2X3")
    e = f.create_entity
    rect = e("IfcRectangleProfileDef", ProfileType="AREA", XDim=3000.0, YDim=200.0)
    product(f, "IfcWallStandardCase", "Old wall", body, storey, sp, [extrusion(f, rect, 2500)])
    f.write(str(out / "wall_ifc2x3.ifc"))


def world_bounds_mm(f, element):
    """Where IfcOpenShell puts the element: the box around its vertices in world coordinates, in millimeters."""
    import ifcopenshell.geom

    settings = ifcopenshell.geom.settings()
    try:
        settings.set("use-world-coords", True)
    except Exception:
        settings.set(settings.USE_WORLD_COORDS, True)
    shape = ifcopenshell.geom.create_shape(settings, element)
    v = shape.geometry.verts
    pts = [(v[i] * 1000.0, v[i + 1] * 1000.0, v[i + 2] * 1000.0) for i in range(0, len(v), 3)]
    return [[min(p[k] for p in pts), max(p[k] for p in pts)] for k in range(3)]


def mapped_with_origin(out):
    """A box that is mapped with a map origin at x = 100 and a target at x = 1000, and a mirrored second use."""
    import json

    f, body, storey, sp = new_model()
    e = f.create_entity
    rect = e("IfcRectangleProfileDef", ProfileType="AREA", XDim=10.0, YDim=10.0)
    source = e("IfcShapeRepresentation", ContextOfItems=body, RepresentationIdentifier="Body", RepresentationType="SweptSolid", Items=[extrusion(f, rect, 10)])
    rmap = e("IfcRepresentationMap", MappingOrigin=axis(f, 100.0, 0.0, 0.0), MappedRepresentation=source)
    op = e("IfcCartesianTransformationOperator3D", LocalOrigin=pt(f, 1000, 0, 0), Scale=1.0)
    item = e("IfcMappedItem", MappingSource=rmap, MappingTarget=op)
    el = product(f, "IfcFurnishingElement", "Bench", body, storey, sp, [item], rep_type="MappedRepresentation")
    f.write(str(out / "mapped_origin.ifc"))
    (out / "mapped_origin.json").write_text(json.dumps(world_bounds_mm(f, el)))


if __name__ == "__main__":
    folder = Path(sys.argv[1])
    folder.mkdir(parents=True, exist_ok=True)
    wall_with_opening(folder)
    slab_with_voids(folder)
    mapped_bollards(folder)
    terrain_and_tree(folder)
    ifc2x3_wall(folder)
    mapped_with_origin(folder)
    print("ok")
