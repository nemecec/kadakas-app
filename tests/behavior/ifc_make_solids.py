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


if __name__ == "__main__":
    folder = Path(sys.argv[1])
    folder.mkdir(parents=True, exist_ok=True)
    wall_door_window(folder)
    slab_round_hole_and_pocket(folder)
    print("ok")
