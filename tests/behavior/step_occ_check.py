"""Reads a STEP file with OpenCASCADE (cadquery or OCP) and prints its volume, area and box as
JSON. Run: python -I step_occ_check.py FILE. Exit code 3 when OpenCASCADE is not installed.
The test that calls it skips then."""

import json
import sys

try:
    from OCP.Bnd import Bnd_Box
    from OCP.BRepBndLib import BRepBndLib
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.STEPControl import STEPControl_Reader
except ImportError:
    sys.exit(3)

reader = STEPControl_Reader()
if reader.ReadFile(sys.argv[1]) != IFSelect_RetDone:
    sys.exit(2)
reader.TransferRoots()
shape = reader.OneShape()
vol, area, box = GProp_GProps(), GProp_GProps(), Bnd_Box()
BRepGProp.VolumeProperties_s(shape, vol)
BRepGProp.SurfaceProperties_s(shape, area)
BRepBndLib.Add_s(shape, box)
lo = box.CornerMin()
hi = box.CornerMax()
print(json.dumps({
    "volume": vol.Mass(),
    "area": area.Mass(),
    "min": [lo.X(), lo.Y(), lo.Z()],
    "max": [hi.X(), hi.Y(), hi.Z()],
}))
