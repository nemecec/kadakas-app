"""Writes parking_lot_loose.dxf: two parking lots drawn as loose LINEs and ARCs (nothing is a closed
polyline), with small gaps at the corners, for the region and zone tests (PAT-06, PAT-07, PAT-08).

Run: uv run --with ezdxf python tests/behavior/fixtures/make_parking_lot_loose.py

Units are millimeters.

- Lot A, layer LOT_A: a plain rectangle 10,000 by 6,000 at the origin. Four lines. Three corners
  are open by 0.3 mm, one by 0.4 mm, and the top line is drawn 30 mm too long past the left corner
  (a stub that overshoots, which is harmless).
- Lot B, layer LOT_B: 12,000 by 8,000 at x = 20,000, with the top right corner rounded by an ARC of
  radius 1,500. Lines and the arc are open by 0.2 to 0.5 mm at their ends.
- The island, layer ISLAND: a rectangle 2,000 by 1,000 inside lot B (four lines, each end open by
  0.2 mm).
- Layer STALLS: three vertical lines across lot B (x = 23,000, 26,000, 29,000), drawn 100 mm past
  the lot at both ends.
- Layer BROKEN: a rectangle at x = 40,000 whose left side is 30 mm short: it does not close.
- Layer NOTES: a text.
"""

from pathlib import Path

import ezdxf

doc = ezdxf.new("R2010")
doc.header["$INSUNITS"] = 4
for name in ("LOT_A", "LOT_B", "ISLAND", "STALLS", "BROKEN", "NOTES"):
    doc.layers.add(name)
msp = doc.modelspace()


def line(a, b, layer):
    msp.add_line(a, b, dxfattribs={"layer": layer})


# Lot A.
line((0.3, 0), (10_000, 0), "LOT_A")
line((10_000, 0.3), (10_000, 6_000), "LOT_A")
line((9_999.7, 6_000), (-30, 6_000), "LOT_A")
line((0, 5_999.6), (0, 0), "LOT_A")

# Lot B: x 20,000 to 32,000, y 0 to 8,000, the top right corner rounded (radius 1,500).
line((20_000.2, 0), (32_000, 0), "LOT_B")
line((32_000, 0.3), (32_000, 6_500), "LOT_B")
msp.add_arc(
    center=(30_500, 6_500),
    radius=1_500,
    start_angle=0.0,
    end_angle=90.0 - 0.0112,  # the end of the arc is 0.3 mm short of the top line
    dxfattribs={"layer": "LOT_B"},
)
line((30_500.4, 8_000), (20_000, 8_000), "LOT_B")
line((20_000, 7_999.5), (20_000, 0), "LOT_B")

# The island.
line((24_000.2, 3_000), (26_000, 3_000), "ISLAND")
line((26_000, 3_000.2), (26_000, 4_000), "ISLAND")
line((25_999.8, 4_000), (24_000, 4_000), "ISLAND")
line((24_000, 3_999.8), (24_000, 3_000), "ISLAND")

# Stall lines.
for x in (23_000, 26_000, 29_000):
    line((x, -100), (x, 8_100), "STALLS")

# A shape that does not close.
line((40_000, 0), (46_000, 0), "BROKEN")
line((46_000, 0), (46_000, 4_000), "BROKEN")
line((46_000, 4_000), (40_000, 4_000), "BROKEN")
line((40_000, 3_970), (40_000, 0), "BROKEN")

msp.add_text("PARKING LOTS", height=500, dxfattribs={"layer": "NOTES", "insert": (0, 7_000)})

out = Path(__file__).with_name("parking_lot_loose.dxf")
doc.saveas(out)
print("wrote", out)
