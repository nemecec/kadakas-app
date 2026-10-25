"""Writes IFC test files with IfcOpenShell for the cuts and the solids of IOP-29: openings, revolved
solids, swept disks, CSG solids. The behavior tests run it with
`uv run --no-project --with ifcopenshell python -I ifc_make_solids.py <folder>`, compute the volumes with
IfcOpenShell (`ifc_volumes.py`) and compare them with the volumes that Kadakas gets from the same files."""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import ifcopenshell
import ifcopenshell.guid
from ifc_make import axis, extrusion, new_model, product, pt


def direction(f, x, y, z):
    return f.create_entity("IfcDirection", DirectionRatios=(float(x), float(y), float(z)))


def placement(f, at, z=(0, 0, 1), x=(1, 0, 0)):
    return f.create_entity("IfcAxis2Placement3D", Location=pt(f, *at), Axis=direction(f, *z), RefDirection=direction(f, *x))


def void(f, host, name, body, storey, sp, items, at=(0.0, 0.0, 0.0)):
    guid = ifcopenshell.guid.new
    op = product(f, "IfcOpeningElement", name, body, storey, sp, items, at=at, PredefinedType="OPENING")
    f.create_entity("IfcRelVoidsElement", GlobalId=guid(), RelatingBuildingElement=host, RelatedOpeningElement=op)
    return op


def wall_door_window(out):
    """6000 x 250 x 2800 wall: a door (1000 x 2100, up from the floor) and a window (1200 x 1200 at 900), both through the wall."""
    f, body, storey, sp = new_model()
    e = f.create_entity
    wall = product(f, "IfcWall", "Wall", body, storey, sp,
                   [extrusion(f, e("IfcRectangleProfileDef", ProfileType="AREA", XDim=6000.0, YDim=250.0), 2800, 0, 0, 0)],
                   PredefinedType="STANDARD")
    door = e("IfcExtrudedAreaSolid", SweptArea=e("IfcRectangleProfileDef", ProfileType="AREA", XDim=1000.0, YDim=250.0),
             Position=axis(f, -1500.0, 0.0, 0.0), ExtrudedDirection=direction(f, 0, 0, 1), Depth=2100.0)
    void(f, wall, "Door", body, storey, sp, [door])
    window = e("IfcExtrudedAreaSolid", SweptArea=e("IfcRectangleProfileDef", ProfileType="AREA", XDim=1200.0, YDim=1200.0),
               Position=placement(f, (1500.0, -200.0, 1500.0), z=(0, 1, 0), x=(1, 0, 0)), ExtrudedDirection=direction(f, 0, 0, 1), Depth=650.0)
    void(f, wall, "Window", body, storey, sp, [window])
    f.write(str(out / "wall_door_window.ifc"))


def slab_round_hole_and_pocket(out):
    """8000 x 5000 x 200 slab with a round hole (radius 500) and a pocket (1000 x 800, 80 deep)."""
    f, body, storey, sp = new_model()
    e = f.create_entity
    slab = product(f, "IfcSlab", "Slab", body, storey, sp,
                   [extrusion(f, e("IfcRectangleProfileDef", ProfileType="AREA", XDim=8000.0, YDim=5000.0), 200, 0, 0, 0)],
                   PredefinedType="FLOOR")
    hole = e("IfcExtrudedAreaSolid", SweptArea=e("IfcCircleProfileDef", ProfileType="AREA", Radius=500.0),
             Position=axis(f, 1500.0, 500.0, -50.0), ExtrudedDirection=direction(f, 0, 0, 1), Depth=300.0)
    void(f, slab, "Hole", body, storey, sp, [hole])
    pocket = e("IfcExtrudedAreaSolid", SweptArea=e("IfcRectangleProfileDef", ProfileType="AREA", XDim=1000.0, YDim=800.0),
               Position=axis(f, -2000.0, -500.0, 120.0), ExtrudedDirection=direction(f, 0, 0, 1), Depth=100.0)
    void(f, slab, "Pocket", body, storey, sp, [pocket])
    f.write(str(out / "slab_hole_pocket.ifc"))


def revolved_ring(out):
    """A rectangle 100 x 50 at 200 from the axis, turned 120 degrees about the Y axis of its frame."""
    f, body, storey, sp = new_model()
    e = f.create_entity
    profile = e("IfcRectangleProfileDef", ProfileType="AREA", Position=e("IfcAxis2Placement2D", Location=e("IfcCartesianPoint", Coordinates=(250.0, 25.0))), XDim=100.0, YDim=50.0)
    solid = e("IfcRevolvedAreaSolid", SweptArea=profile, Position=axis(f),
              Axis=e("IfcAxis1Placement", Location=pt(f, 0.0, 0.0, 0.0), Axis=direction(f, 0, 1, 0)), Angle=math.radians(120.0))
    product(f, "IfcBuildingElementProxy", "Ring", body, storey, sp, [solid])
    f.write(str(out / "revolved_ring.ifc"))


def swept_pipe(out):
    """A pipe of radius 25 along a polyline with two bends, and a hollow one."""
    f, body, storey, sp = new_model()
    e = f.create_entity
    path = e("IfcPolyline", Points=[pt(f, 0, 0, 0), pt(f, 1000, 0, 0), pt(f, 1000, 800, 600), pt(f, 1000, 800, 1600)])
    solid = e("IfcSweptDiskSolid", Directrix=path, Radius=25.0)
    product(f, "IfcBuildingElementProxy", "Pipe", body, storey, sp, [solid], rep_type="AdvancedSweptSolid")
    path2 = e("IfcPolyline", Points=[pt(f, 0, 2000, 0), pt(f, 1500, 2000, 0)])
    hollow = e("IfcSweptDiskSolid", Directrix=path2, Radius=30.0, InnerRadius=20.0)
    product(f, "IfcBuildingElementProxy", "Tube", body, storey, sp, [hollow], rep_type="AdvancedSweptSolid")
    f.write(str(out / "swept_pipe.ifc"))


def csg_solids(out):
    """A block less a cylinder; a sphere; a cone."""
    f, body, storey, sp = new_model()
    e = f.create_entity
    block = e("IfcBlock", Position=axis(f), XLength=2000.0, YLength=800.0, ZLength=800.0)
    cyl = e("IfcRightCircularCylinder", Position=axis(f, 1000.0, 400.0, -100.0), Height=1000.0, Radius=200.0)
    tree = e("IfcBooleanResult", Operator="DIFFERENCE", FirstOperand=block, SecondOperand=cyl)
    product(f, "IfcBuildingElementProxy", "Block with hole", body, storey, sp, [e("IfcCsgSolid", TreeRootExpression=tree)], rep_type="CSG")
    sphere = e("IfcSphere", Position=axis(f, 0.0, 0.0, 0.0), Radius=300.0)
    product(f, "IfcBuildingElementProxy", "Ball", body, storey, sp, [e("IfcCsgSolid", TreeRootExpression=sphere)], at=(5000.0, 0.0, 0.0), rep_type="CSG")
    cone = e("IfcRightCircularCone", Position=axis(f), Height=600.0, BottomRadius=200.0)
    product(f, "IfcBuildingElementProxy", "Cone", body, storey, sp, [e("IfcCsgSolid", TreeRootExpression=cone)], at=(7000.0, 0.0, 0.0), rep_type="CSG")
    f.write(str(out / "csg_solids.ifc"))


if __name__ == "__main__":
    folder = Path(sys.argv[1])
    folder.mkdir(parents=True, exist_ok=True)
    wall_door_window(folder)
    slab_round_hole_and_pocket(folder)
    revolved_ring(folder)
    swept_pipe(folder)
    csg_solids(folder)
    print("ok")
