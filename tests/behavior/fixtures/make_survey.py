"""Make a test topographic survey DXF (rev1 and rev2) for a ~1 ha park site.

Coordinates are L-EST97 (EPSG:3301), meters, near Tallinn.
Run: uv run --with ezdxf python make_survey.py
"""
import math
import sys
import ezdxf

E0, N0 = 542000.0, 6588000.0  # SW corner of the site
W, H = 120.0, 90.0           # ~1.08 ha


def ground(x, y):
    # gentle slope to the north-east with a low swale
    return 32.0 + 0.035 * x + 0.02 * y - 0.8 * math.exp(-((x - 60) ** 2 + (y - 30) ** 2) / 600.0)


def make(rev: int, path: str):
    doc = ezdxf.new("R2018", setup=True)
    doc.header["$INSUNITS"] = 6  # meters
    msp = doc.modelspace()
    for name, color in [("SITE_BOUNDARY", 1), ("CONTOURS_MAJOR", 3), ("CONTOURS_MINOR", 8),
                        ("SPOT_LEVELS", 2), ("TREES", 82), ("TREE_TEXT", 7), ("BUILDING", 5),
                        ("ROAD_EDGE", 9), ("BREAKLINES", 6)]:
        doc.layers.add(name, color=color)

    # site boundary (closed, slightly irregular)
    bnd = [(0, 0), (W, 0), (W, 70), (100, H), (0, H)]
    msp.add_lwpolyline([(E0 + x, N0 + y) for x, y in bnd], close=True, dxfattribs={"layer": "SITE_BOUNDARY"})

    # contours: 3D polylines, sampled along iso-lines by marching a grid (simple: polyline per row crossing)
    # We approximate contour lines by tracing z=c on a fine grid with a simple scan per x column.
    levels = [32.5 + 0.5 * i for i in range(14)]
    for c in levels:
        pts = []
        for xi in range(0, int(W) + 1, 2):
            x = float(xi)
            # solve ground(x, y) = c for y in [0, H] by bisection if bracketed
            lo, hi = 0.0, H
            flo, fhi = ground(x, lo) - c, ground(x, hi) - c
            if flo * fhi > 0:
                if pts:
                    break
                continue
            for _ in range(40):
                mid = (lo + hi) / 2
                fm = ground(x, mid) - c
                if flo * fm <= 0:
                    hi, fhi = mid, fm
                else:
                    lo, flo = mid, fm
            y = (lo + hi) / 2
            if rev == 2 and abs(c - 35.0) < 1e-6 and 40 <= x <= 70:
                y += 1.5  # re-surveyed contour moved
            pts.append((E0 + x, N0 + y, c))
        if len(pts) >= 2:
            layer = "CONTOURS_MAJOR" if abs(c - round(c)) < 1e-6 else "CONTOURS_MINOR"
            msp.add_polyline3d(pts, dxfattribs={"layer": layer})

    # breakline along the road edge (3D)
    road = [(0, 5), (60, 4), (W, 6)]
    msp.add_polyline3d([(E0 + x, N0 + y, ground(x, y)) for x, y in road], dxfattribs={"layer": "BREAKLINES"})
    msp.add_lwpolyline([(E0 + x, N0 + y) for x, y in road], dxfattribs={"layer": "ROAD_EDGE"})

    # spot levels: POINT with z + TEXT
    spots = [(10, 10), (30, 60), (55, 30), (80, 15), (100, 50), (20, 85), (70, 80), (110, 30), (60, 50), (45, 15)]
    if rev == 2:
        spots += [(90, 70), (5, 45)]
    for x, y in spots:
        z = ground(x, y)
        msp.add_point((E0 + x, N0 + y, z), dxfattribs={"layer": "SPOT_LEVELS"})
        msp.add_text(f"{z:.2f}", height=0.4, dxfattribs={"layer": "SPOT_LEVELS"}).set_placement((E0 + x + 0.3, N0 + y + 0.3))

    # trees: block with attributes (common survey practice)
    blk = doc.blocks.new("TREE_SYM")
    blk.add_circle((0, 0), 0.5)
    blk.add_line((-0.5, 0), (0.5, 0))
    blk.add_line((0, -0.5), (0, 0.5))
    blk.add_attdef("SPECIES", (0.6, 0.2), dxfattribs={"height": 0.3})
    blk.add_attdef("DBH", (0.6, -0.2), dxfattribs={"height": 0.3})
    blk.add_attdef("CROWN", (0.6, -0.6), dxfattribs={"height": 0.3})
    trees = [
        ("T1", 15, 70, "Quercus robur", 60, 12),
        ("T2", 25, 75, "Tilia cordata", 45, 9),
        ("T3", 90, 40, "Acer platanoides", 35, 8),
        ("T4", 105, 20, "Betula pendula", 30, 6),
        ("T5", 50, 40, "Picea abies", 40, 6),   # in the car park area -> remove
        ("T6", 65, 42, "Betula pendula", 25, 5),  # in the car park area -> remove
        ("T7", 8, 30, "Fraxinus excelsior", 55, 11),
    ]
    for tid, x, y, sp, dbh, crown in trees:
        if rev == 2 and tid == "T6":
            continue  # felled by the time of rev2
        if rev == 2 and tid == "T3":
            x += 0.8  # re-surveyed position
        ins = msp.add_blockref("TREE_SYM", (E0 + x, N0 + y, ground(x, y)), dxfattribs={"layer": "TREES"})
        ins.add_auto_attribs({"SPECIES": sp, "DBH": str(dbh), "CROWN": str(crown)})
        msp.add_circle((E0 + x, N0 + y), crown / 2, dxfattribs={"layer": "TREES"})
        msp.add_text(tid, height=0.5, dxfattribs={"layer": "TREE_TEXT"}).set_placement((E0 + x - 1, N0 + y - 1.2))

    # existing building (pavilion) to keep
    b = [(10, 40), (24, 40), (24, 52), (10, 52)]
    if rev == 2:
        b = [(10, 40), (24.5, 40), (24.5, 52), (10, 52)]  # corner re-measured
    msp.add_lwpolyline([(E0 + x, N0 + y) for x, y in b], close=True, dxfattribs={"layer": "BUILDING"})

    doc.saveas(path)
    print("wrote", path, len(msp))


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    make(1, f"{out}/survey_rev1.dxf")
    make(2, f"{out}/survey_rev2.dxf")
