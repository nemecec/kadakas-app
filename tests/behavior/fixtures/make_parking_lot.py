"""Writes parking_lot.dxf: a survey of a parking lot for the paving workflow test.

Run: uv run --with ezdxf python tests/behavior/fixtures/make_parking_lot.py

Layer LOT has the closed outline of the lot (60 m by 40 m, one corner cut off) and a closed
planting island inside it. Layer STALLS has the lines of the parking stalls, and layer NOTES a
text. Units are millimeters. The numbers are large map coordinates, so the importer has to shift
the drawing to the origin.
"""

from pathlib import Path

import ezdxf

doc = ezdxf.new("R2010")
doc.header["$INSUNITS"] = 4
for name in ("LOT", "STALLS", "NOTES"):
    doc.layers.add(name)
msp = doc.modelspace()

ox, oy = 650_000.0 * 1000, 6_470_000.0 * 1000
outline = [
    (ox + 0, oy + 0),
    (ox + 60_000, oy + 0),
    (ox + 60_000, oy + 28_000),
    (ox + 48_000, oy + 40_000),
    (ox + 0, oy + 40_000),
]
msp.add_lwpolyline(outline, close=True, dxfattribs={"layer": "LOT"})
island = [
    (ox + 22_000, oy + 14_000),
    (ox + 30_000, oy + 14_000),
    (ox + 31_000, oy + 20_000),
    (ox + 26_000, oy + 24_000),
    (ox + 21_000, oy + 20_000),
]
msp.add_lwpolyline(island, close=True, dxfattribs={"layer": "LOT"})
for i in range(10):
    x = ox + 2_000 + i * 2_500
    msp.add_line((x, oy + 1_000), (x, oy + 6_000), dxfattribs={"layer": "STALLS"})
msp.add_text(
    "PARKING LOT", height=500, dxfattribs={"layer": "NOTES", "insert": (ox + 1_000, oy + 38_000)}
)

out = Path(__file__).with_name("parking_lot.dxf")
doc.saveas(out)
print("wrote", out)
