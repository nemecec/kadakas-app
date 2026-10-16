"""Writes the corpus of messy parking lot DXF files and `expected.json` (what each file must give).

Run: uv run --with ezdxf python tests/behavior/fixtures/messy/make_messy.py

Every file draws the same kind of lot as a real CAD export would, in a different untidy way. All
numbers below are in millimeters in the "world" of the lot (the lot is a 30 m by 20 m rectangle at
the origin); a `Frame` writes them in the units and the place of the file.

The standard lot:
- the outline, 30,000 by 20,000 (600 m2), here and there with a rounded or curved corner;
- three islands: a circle (center 6,000 / 10,000, radius 1,500: 7.0686 m2), a square 3,000 by 3,000
  (x 12,000 to 15,000, y 8,500 to 11,500: 9 m2) and a half disc (center 20,000 / 10,000, radius 2,000,
  the right half: 6.2832 m2);
- stall rows: lines y = 5,000 and y = 15,000 over the whole width, a drive line x = 25,000 over the
  whole height, and short stall lines (x = 2,500 * k, k = 1 to 9, y 0 to 5,000 and 15,000 to 20,000).
  Two drive aisles (the middle band and the drive on the right) and 20 stalls make 24 zones.

`expected.json` has for each file: the gross and the net area of the lot (the net area is the gross
area less the islands), the way to find the lot (`find`), the way to make the hardscape (`via`), the
tolerance, the layer of the stall lines (`stalls`) and the words that the import report must hold.
"""

import json
import math
import random
from pathlib import Path

import ezdxf
from ezdxf import const
from ezdxf.math import Vec3

HERE = Path(__file__).resolve().parent
W, H = 30_000.0, 20_000.0
CIRCLE = ((6_000.0, 10_000.0), 1_500.0)
SQUARE = (12_000.0, 8_500.0, 15_000.0, 11_500.0)
HALF = ((20_000.0, 10_000.0), 2_000.0)
ISLANDS_TOTAL = math.pi * 1.5**2 + 9.0 + math.pi * 2.0**2 / 2
STALL_X = [2_500.0 * k for k in range(1, 10)]
ZONES = 24
EXPECTED = {}


class Frame:
    """Writes world millimeters in the units and the place of a file."""

    def __init__(self, unit_mm=1.0, off=(0.0, 0.0)):
        self.u, self.off = unit_mm, off

    def p(self, x, y):
        return ((x + self.off[0]) / self.u, (y + self.off[1]) / self.u)

    def d(self, v):
        return v / self.u


MM = Frame()


def shoelace(points):
    return abs(sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(points, points[1:] + points[:1]))) / 2e6


def new(version="R2010", insunits=4, layers=()):
    doc = ezdxf.new(version, setup=False)
    if version != "R12" and insunits is not None:
        doc.header["$INSUNITS"] = insunits
    for name in layers:
        if name not in doc.layers:
            doc.layers.add(name)
    return doc


def save(doc, name, fr=MM, **entry):
    doc.saveas(HERE / name)
    entry["origin"] = list(fr.p(0, 0))  # the corner of the lot in the units of the file
    entry["unit_mm"] = fr.u
    entry.setdefault("find", {"how": "layer", "layers": ["LOT"]})
    entry.setdefault("via", "from_curves")
    entry.setdefault("tol", 1.0)
    entry.setdefault("stalls", None)
    entry.setdefault("import", {})
    entry.setdefault("notes", [])
    entry.setdefault("area_tol", 0.02)
    entry["net_m2"] = round(entry["gross_m2"] - entry.get("islands_m2", ISLANDS_TOTAL), 6)
    EXPECTED[name] = entry


def rect_pts(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def lot_pts():
    return rect_pts(0, 0, W, H)


# --- pieces: the same island or edge drawn in the different ways of CAD ------------------------


def isl_circle_entity(msp, fr, layer):
    msp.add_circle(fr.p(*CIRCLE[0]), fr.d(CIRCLE[1]), dxfattribs={"layer": layer})


def isl_circle_bulges(msp, fr, layer, closed=True):
    (cx, cy), r = CIRCLE
    pts = [(*fr.p(cx - r, cy), 1), (*fr.p(cx + r, cy), 1)]
    if not closed:
        pts.append((*fr.p(cx - r, cy), 0))
    msp.add_lwpolyline(pts, format="xyb", close=closed, dxfattribs={"layer": layer})


def isl_square_lines(msp, fr, layer, gap=0.0):
    pts = rect_pts(*SQUARE)
    for i in range(4):
        a, b = pts[i], pts[(i + 1) % 4]
        if gap:
            ux, uy = (b[0] - a[0]) / 3000, (b[1] - a[1]) / 3000
            a = (a[0] + ux * gap, a[1] + uy * gap)
        msp.add_line(fr.p(*a), fr.p(*b), dxfattribs={"layer": layer})


def isl_square_poly(msp, fr, layer, closed=True):
    pts = [fr.p(*p) for p in rect_pts(*SQUARE)]
    if not closed:
        pts.append(pts[0])
    msp.add_lwpolyline(pts, close=closed, dxfattribs={"layer": layer})


def isl_half_arc_line(msp, fr, layer, gap_deg=0.0):
    (cx, cy), r = HALF
    msp.add_arc(fr.p(cx, cy), fr.d(r), -90.0 + gap_deg, 90.0 - gap_deg, dxfattribs={"layer": layer})
    msp.add_line(fr.p(cx, cy - r), fr.p(cx, cy + r), dxfattribs={"layer": layer})


def isl_half_poly(msp, fr, layer):
    (cx, cy), r = HALF
    msp.add_lwpolyline(
        [(*fr.p(cx, cy - r), math.tan(math.pi / 4)), (*fr.p(cx, cy + r), 0)],
        format="xyb", close=True, dxfattribs={"layer": layer},
    )


def isl_ellipse(msp, fr, layer):
    """A tilted ellipse of 2,000 by 1,000 half axes (6.2832 m2) in the place of the circle."""
    c = (6_000.0, 10_000.0)
    tilt = math.radians(30)
    msp.add_ellipse(
        (*fr.p(*c), 0), major_axis=(fr.d(2_000.0 * math.cos(tilt)), fr.d(2_000.0 * math.sin(tilt)), 0), ratio=0.5,
        dxfattribs={"layer": layer},
    )


def standard_islands(msp, fr, layer):
    isl_circle_entity(msp, fr, layer)
    isl_square_poly(msp, fr, layer)
    isl_half_poly(msp, fr, layer)


def stall_lines(msp, fr, layer, over=0.0, gap=0.0):
    """The stall lines. `gap`: the stall dividers stop that far before the stall row line."""
    def ln(a, b):
        msp.add_line(fr.p(*a), fr.p(*b), dxfattribs={"layer": layer})

    ln((-over, 5_000), (W + over, 5_000))
    ln((-over, 15_000), (W + over, 15_000))
    ln((25_000, -over), (25_000, H + over))
    for x in STALL_X:
        ln((x, -over), (x, 5_000 - gap))
        ln((x, 15_000 + gap), (x, H + over))


def soup(msp, fr, layer, seed=7, islands=True, z=0.0):
    """The lot and the islands as loose LINEs: each side in 4 to 6 pieces with gaps of 0.1 to 5 mm,
    overshoots at the corners, duplicates and pieces that overlap on the same line."""
    rng = random.Random(seed)
    gaps = [0.1, 0.3, 0.5, 1.0, 2.0, 5.0]
    pts = lot_pts()
    for i in range(4):
        a, b = pts[i], pts[(i + 1) % 4]
        length = math.dist(a, b)
        u = ((b[0] - a[0]) / length, (b[1] - a[1]) / length)
        k = rng.randint(4, 6)
        marks = [0.0, *sorted(rng.uniform(0.1, 0.9) * length for _ in range(k - 1)), length]

        def at(t):
            return fr.p(a[0] + u[0] * t, a[1] + u[1] * t)

        for j in range(k):
            s, e = marks[j], marks[j + 1]
            if j > 0:
                s += rng.choice(gaps) / 2
            if j < k - 1:
                e -= rng.choice(gaps) / 2
            if j == 0 and rng.random() < 0.7:
                s -= rng.choice([30.0, 120.0, 400.0])  # overshoots the corner
            if j == k - 1 and rng.random() < 0.7:
                e += rng.choice([30.0, 120.0, 400.0])
            def put(p, q):
                if z:  # survey lines carry heights
                    zz = z + rng.uniform(-50, 50)
                    p, q = (*p, zz), (*q, zz + rng.uniform(-5, 5))
                msp.add_line(p, q, dxfattribs={"layer": layer})

            put(at(s), at(e))
            if rng.random() < 0.3:  # a duplicate, drawn the other way round
                put(at(e), at(s))
            if rng.random() < 0.3 and e - s > 1500:  # a piece on the same line
                put(at(s + 300), at(e - 200))
    if islands:
        isl_square_lines(msp, fr, layer, gap=2.0)
        isl_circle_entity(msp, fr, layer)
        isl_half_arc_line(msp, fr, layer, gap_deg=0.02)  # 0.02 degrees at r 2,000 is 0.7 mm


def gross_corner_arc(r):
    """Gross area of the lot with one corner rounded by a quarter circle of radius r (mm)."""
    return 600.0 - r * r * (1 - math.pi / 4) / 1e6


def lot_poly_arcs(msp, fr, layer, closed=True, repeat_first=False, r=3_000.0):
    """The lot as one LWPOLYLINE with a rounded top right corner (a bulge)."""
    b = math.tan(math.pi / 8)
    pts = [(0, 0, 0), (W, 0, 0), (W, H - r, b), (W - r, H, 0), (0, H, 0)]
    pts = [(*fr.p(x, y), bb) for x, y, bb in pts]
    if repeat_first:
        pts.append((*pts[0][:2], 0))
    msp.add_lwpolyline(pts, format="xyb", close=closed, dxfattribs={"layer": layer})


# --- the files ---------------------------------------------------------------------------------


def f01_soup():
    doc = new("R2010", 4, ["LOT", "STALLS"])
    msp = doc.modelspace()
    soup(msp, MM, "LOT")
    stall_lines(msp, MM, "STALLS", over=100, gap=2)
    save(doc, "01_line_soup_mm.dxf", desc="LINE soup with gaps of 0.1 to 5 mm, overshoots, duplicates, overlaps",
         gross_m2=600.0, tol=6.0, stalls="STALLS", find={"how": "layer", "layers": ["LOT"]})


def f02_lwpoly_bulge_open():
    """LWPOLYLINE with bulges; the closed flag is missing but the last point equals the first."""
    doc = new("R2010", 4, ["LOT", "ISLANDS", "STALLS"])
    msp = doc.modelspace()
    lot_poly_arcs(msp, MM, "LOT", closed=False, repeat_first=True)
    isl_circle_bulges(msp, MM, "ISLANDS", closed=False)
    isl_square_poly(msp, MM, "ISLANDS", closed=False)
    isl_half_poly(msp, MM, "ISLANDS")
    stall_lines(msp, MM, "STALLS")
    save(doc, "02_lwpolyline_bulge_unclosed.dxf", desc="LWPOLYLINE with bulges, closed flag missing, first point = last",
         gross_m2=gross_corner_arc(3000.0), stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes")


def f03_polyline_2d():
    """Old style POLYLINE 2D with VERTEX bulges; one outline has the closed flag, others do not."""
    doc = new("R2000", 4, ["LOT", "ISLANDS", "STALLS"])
    msp = doc.modelspace()
    b = math.tan(math.pi / 8)
    r = 3000.0
    msp.add_polyline2d([(0, 0, 0), (W, 0, 0), (W, H - r, b), (W - r, H, 0), (0, H, 0)], format="xyb",
                       close=True, dxfattribs={"layer": "LOT"})
    msp.add_polyline2d([(CIRCLE[0][0] - 1500, 10_000, 1), (CIRCLE[0][0] + 1500, 10_000, 1)], format="xyb",
                       close=True, dxfattribs={"layer": "ISLANDS"})
    msp.add_polyline2d([*rect_pts(*SQUARE), SQUARE[:2]], dxfattribs={"layer": "ISLANDS"})  # first = last
    msp.add_polyline2d([(20_000, 8_000, math.tan(math.pi / 4)), (20_000, 12_000, 0)], format="xyb", close=True,
                       dxfattribs={"layer": "ISLANDS"})
    stall_lines(msp, MM, "STALLS")
    save(doc, "03a_polyline_2d_old.dxf", desc="old POLYLINE 2D with bulges, mixed closed flags",
         gross_m2=gross_corner_arc(3000.0), stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes")


def f03_polyline_3d():
    """POLYLINE 3D (flag 8) with heights for the outline and the square island."""
    doc = new("R2000", 4, ["LOT", "ISLANDS", "STALLS"])
    msp = doc.modelspace()
    z = [0.0, 40.0, 95.0, 150.0]
    msp.add_polyline3d([(x, y, zz) for (x, y), zz in zip(lot_pts(), z)], close=True, dxfattribs={"layer": "LOT"})
    isl_circle_entity(msp, MM, "ISLANDS")
    msp.add_polyline3d([(x, y, 20.0) for x, y in rect_pts(*SQUARE)], close=True, dxfattribs={"layer": "ISLANDS"})
    isl_half_arc_line(msp, MM, "ISLANDS")
    stall_lines(msp, MM, "STALLS")
    save(doc, "03b_polyline_3d_heights.dxf", desc="POLYLINE 3D with heights as the outline",
         gross_m2=600.0, stalls="STALLS", find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_curves")


def f04_splines():
    r = 3_000.0
    k = 0.5522847498
    c = (W - r, H - r)
    fit = [(c[0] + r * math.cos(math.radians(a)), c[1] + r * math.sin(math.radians(a))) for a in (0, 22.5, 45, 67.5, 90)]
    bez = [(W, H - r), (W, H - r + k * r), (W - r + k * r, H), (W - r, H)]
    for name, how in (("04a_spline_fit_points.dxf", "fit"), ("04b_spline_control_points.dxf", "ctrl"),
                      ("04c_spline_control_frame.dxf", "frame")):
        doc = new("R2010", 4, ["LOT", "ISLANDS", "STALLS"])
        msp = doc.modelspace()

        def ln(a, b):
            msp.add_line(a, b, dxfattribs={"layer": "LOT"})

        ln((0, 0), (W, 0))
        ln((W, 0), (W, H - r))
        if how == "fit":
            sp = msp.add_spline(fit_points=fit, dxfattribs={"layer": "LOT"})
        elif how == "ctrl":
            sp = msp.add_open_spline(bez, degree=3, dxfattribs={"layer": "LOT"})
        else:
            sp = msp.add_spline_control_frame(fit, degree=3, dxfattribs={"layer": "LOT"})
        pts = [(p.x, p.y) for p in sp.construction_tool().flattening(0.02)]
        ln((W - r, H), (0, H))
        ln((0, H), (0, 0))
        standard_islands(msp, MM, "ISLANDS")
        stall_lines(msp, MM, "STALLS")
        gross = shoelace([(0, 0), (W, 0), *pts, (0, H)])
        save(doc, name, desc=f"SPLINE kerb corner ({how})", gross_m2=gross, stalls="STALLS",
             find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, area_tol=0.05 if how == "fit" else 0.02)


def f04_ellipse():
    """An ELLIPSE arc as the kerb curve (a quarter of an ellipse of 4,000 by 2,500) and an ELLIPSE island."""
    doc = new("R2010", 4, ["LOT", "ISLANDS", "STALLS"])
    msp = doc.modelspace()
    a, b = 4_000.0, 2_500.0
    c = (W - a, H - b)

    def ln(p, q):
        msp.add_line(p, q, dxfattribs={"layer": "LOT"})

    ln((0, 0), (W, 0))
    ln((W, 0), (W, c[1]))
    msp.add_ellipse((*c, 0), major_axis=(a, 0, 0), ratio=b / a, start_param=0, end_param=math.pi / 2,
                    dxfattribs={"layer": "LOT"})
    ln((c[0], H), (0, H))
    ln((0, H), (0, 0))
    isl_ellipse(msp, MM, "ISLANDS")
    isl_square_poly(msp, MM, "ISLANDS")
    isl_half_arc_line(msp, MM, "ISLANDS")
    stall_lines(msp, MM, "STALLS")
    gross = 600.0 - a * b * (1 - math.pi / 4) / 1e6
    save(doc, "04d_ellipse_kerb_and_island.dxf", desc="ELLIPSE arc kerb and an ELLIPSE island", gross_m2=gross,
         islands_m2=2 * math.pi * 2.0 + 9.0,
         stalls="STALLS", find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, area_tol=0.02)


def f05_blocks():
    """The lot in blocks: an INSERT with scale 2 and rotation 90, one mirrored, nested blocks and an XREF."""
    doc = new("R2010", 4, ["LOT", "ISLANDS", "STALLS", "XREFS"])
    msp = doc.modelspace()
    # The block frame is the world turned and scaled back, so that the INSERT gives the world lot.
    lotb = doc.blocks.new("LOTB")
    ins = msp.add_blockref("LOTB", (0, 0), dxfattribs={"layer": "LOT", "xscale": 2.0, "yscale": 2.0, "rotation": 90.0})
    inv = ins.matrix44()
    inv.inverse()

    def wl(x, y):
        return tuple(inv.transform(Vec3(x, y, 0)))[:2]

    pts = [wl(*p) for p in lot_pts()]
    for i in range(4):
        lotb.add_line(pts[i], pts[(i + 1) % 4], dxfattribs={"layer": "LOT"})
    # The islands: a mirrored INSERT (xscale -1) of a block that holds a nested block.
    inner = doc.blocks.new("ISL_INNER")
    outer = doc.blocks.new("ISL_OUTER")
    ins2 = msp.add_blockref("ISL_OUTER", (100_000, 50_000), dxfattribs={"layer": "ISLANDS", "xscale": -1.0})
    inv2 = ins2.matrix44()
    inv2.inverse()

    def w2(x, y):
        return tuple(inv2.transform(Vec3(x, y, 0)))[:2]

    outer.add_blockref("ISL_INNER", (0, 0))
    inner.add_circle(w2(*CIRCLE[0]), CIRCLE[1])
    inner.add_lwpolyline([w2(*p) for p in rect_pts(*SQUARE)], close=True)
    # The right half of the world disc lies at the left of the block frame (x mirrored): 90 to 270.
    outer.add_arc(w2(*HALF[0]), HALF[1], 90.0, 270.0)
    outer.add_line(w2(HALF[0][0], HALF[0][1] - HALF[1]), w2(HALF[0][0], HALF[0][1] + HALF[1]))
    # An XREF placeholder: a block with the xref flag, inserted on the layer XREFS.
    doc.blocks.new("SITE_XREF", dxfattribs={"flags": 4, "xref_path": "missing/site.dxf"})
    msp.add_blockref("SITE_XREF", (0, 0), dxfattribs={"layer": "XREFS"})
    stall_lines(msp, MM, "STALLS")
    save(doc, "05_blocks_nested_mirrored_xref.dxf", desc="geometry in blocks: scale, rotation, mirror, nested, XREF",
         gross_m2=600.0, stalls="STALLS", find={"how": "layer", "layers": ["LOT", "ISLANDS"]},
         notes=["SITE_XREF"])


def std_simple(doc, fr, layers=("LOT", "ISLANDS")):
    msp = doc.modelspace()
    msp.add_lwpolyline([fr.p(*p) for p in lot_pts()], close=True, dxfattribs={"layer": layers[0]})
    standard_islands(msp, fr, layers[1])
    return msp


def f06_units():
    for name, code, unit_mm in (("06a_units_mm.dxf", 4, 1.0), ("06b_units_cm.dxf", 5, 10.0), ("06c_units_m.dxf", 6, 1000.0),
                                ("06d_units_inch.dxf", 1, 25.4), ("06e_units_foot.dxf", 2, 304.8)):
        doc = new("R2010", code, ["LOT", "ISLANDS", "STALLS"])
        fr = Frame(unit_mm)
        std_simple(doc, fr)
        stall_lines(doc.modelspace(), fr, "STALLS")
        save(doc, name, fr=fr, desc=f"$INSUNITS {code}", gross_m2=600.0, stalls="STALLS",
             find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes")
    # Unitless: no $INSUNITS (0), coordinates in meters; the import must say it assumed mm, and the
    # option `unit` fixes it.
    doc = new("R2010", 0, ["LOT", "ISLANDS", "STALLS"])
    fr = Frame(1000.0)
    std_simple(doc, fr)
    stall_lines(doc.modelspace(), fr, "STALLS")
    save(doc, "06f_unitless_meters.dxf", fr=fr, desc="$INSUNITS 0 with coordinates in meters", gross_m2=600.0, stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes",
         import_fix={"unit": "m"}, notes=["unit"])
    doc = new("R2010", 0, ["LOT", "ISLANDS", "STALLS"])
    std_simple(doc, MM)
    stall_lines(doc.modelspace(), MM, "STALLS")
    save(doc, "06g_unitless_mm.dxf", desc="$INSUNITS 0 with coordinates in mm", gross_m2=600.0, stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes", notes=["unit"])


def f07_far():
    # Survey coordinates in meters: 650,000 east, 6,588,000 north, as loose lines with small gaps.
    fr = Frame(1000.0, (650_000_000.0, 6_588_000_000.0))
    doc = new("R2010", 6, ["LOT", "STALLS"])
    msp = doc.modelspace()
    soup(msp, fr, "LOT", seed=11)
    stall_lines(msp, fr, "STALLS", over=100)
    save(doc, "07a_survey_m_6588000_north_soup.dxf", fr=fr, desc="survey coordinates (6,588,000 m north) as LINE soup, meters",
         gross_m2=600.0, tol=6.0, stalls="STALLS")
    # Far from the origin in mm: 6.5e8 / 6.6e9 mm; closed polylines.
    fr = Frame(1.0, (650_000_000.0, 6_588_000_000.0))
    doc = new("R2010", 4, ["LOT", "ISLANDS", "STALLS"])
    std_simple(doc, fr)
    stall_lines(doc.modelspace(), fr, "STALLS")
    save(doc, "07b_far_from_origin_mm.dxf", fr=fr, desc="x = 650,000,000 mm, y = 6,588,000,000 mm", gross_m2=600.0, stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes")


LONG = "Pysikud-" + "ülipikk-kihi-nimi-" * 13  # 8 + 234 = 242 characters


def f08_layers():
    names = {
        "lot": "Parkla äärejoon õü",
        "isl": "SAARED šž Ülemiste",
        "stall": LONG,
        "frozen": "KÜLMUTATUD ääre vana",
        "off": "VÄLJAS vana joon",
        "paper": "Leht-ä",
    }
    doc = new("R2010", 4, list(names.values()))
    doc.layers.get(names["lot"]).color = 3  # color by layer
    doc.layers.get(names["isl"]).color = 5
    doc.layers.get(names["frozen"]).freeze()
    doc.layers.get(names["off"]).off()
    msp = doc.modelspace()
    msp.add_lwpolyline(lot_pts(), close=True, dxfattribs={"layer": names["lot"]})
    standard_islands(msp, MM, names["isl"])
    # Colors by entity on the islands: an ACI color and a true color.
    ents = list(msp)
    for e in ents[1:]:
        e.dxf.color = 1
    ents[-1].rgb = (12, 200, 90)
    stall_lines(msp, MM, names["stall"])
    # Old versions of the lot on a frozen and on an off layer: bigger, and not the lot.
    msp.add_lwpolyline(rect_pts(-5_000, -5_000, 40_000, 30_000), close=True, dxfattribs={"layer": names["frozen"]})
    msp.add_lwpolyline(rect_pts(-9_000, -9_000, 50_000, 40_000), close=True, dxfattribs={"layer": names["off"]})
    # Paper space: a sheet frame and a copy of the lot outline, which are not part of the model.
    ps = doc.layouts.new("Layout2")
    ps.add_lwpolyline(rect_pts(0, 0, 420, 297), close=True, dxfattribs={"layer": names["paper"]})
    ps.add_lwpolyline(rect_pts(0, 0, 30_000, 20_000), close=True, dxfattribs={"layer": names["lot"]})
    save(doc, "08_layers_unicode_frozen_paperspace.dxf",
         desc="Unicode, spaces and a 242 character layer name; colors by layer and entity; frozen and off layers; paper space",
         gross_m2=600.0, stalls=names["stall"], find={"how": "layer", "layers": [names["lot"], names["isl"]]},
         via="from_shapes", notes=["paper"], layer_names=names)


def f09_noise():
    doc = new("R2010", 4, ["LOT", "ISLANDS", "STALLS", "TEXT", "DIMS", "HATCH"])
    msp = doc.modelspace()
    msp.add_lwpolyline(lot_pts(), close=True, dxfattribs={"layer": "LOT"})
    standard_islands(msp, MM, "ISLANDS")
    stall_lines(msp, MM, "STALLS")
    msp.add_text("PARKING LOT %%c150 Ä Ö", dxfattribs={"layer": "TEXT", "height": 300, "insert": (1000, 1000)})
    msp.add_mtext("{\\fArial|b1;Parkla}\\Pkokku 600 m2", dxfattribs={"layer": "TEXT", "char_height": 250, "insert": (2000, 12000)})
    msp.add_linear_dim(base=(0, -2000), p1=(0, 0), p2=(W, 0), dxfattribs={"layer": "DIMS"}).render()
    msp.add_aligned_dim(p1=(W, 0), p2=(W, H), distance=2000, dxfattribs={"layer": "DIMS"}).render()
    # A fill hatch of the lot next to the real outline.
    h = msp.add_hatch(color=8, dxfattribs={"layer": "HATCH"})
    h.paths.add_polyline_path([(x, y, 0) for x, y in lot_pts()], is_closed=True,
                              flags=const.BOUNDARY_PATH_EXTERNAL | const.BOUNDARY_PATH_POLYLINE)
    save(doc, "09_text_mtext_dimensions_hatch_noise.dxf", desc="TEXT, MTEXT, dimensions and a hatch around the lot",
         gross_m2=600.0, stalls="STALLS", find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes")


def f10_hatch_only():
    """The lot is only a HATCH: one outer boundary and three islands, no other outline."""
    for name, mode in (("10a_hatch_polyline_paths_only.dxf", "poly"), ("10b_hatch_edge_paths_only.dxf", "edge")):
        doc = new("R2010", 4, ["HATCH", "STALLS"])
        msp = doc.modelspace()
        h = msp.add_hatch(color=252, dxfattribs={"layer": "HATCH"})
        ext = const.BOUNDARY_PATH_EXTERNAL
        if mode == "poly":
            h.paths.add_polyline_path([(x, y, 0) for x, y in lot_pts()], is_closed=True, flags=ext | const.BOUNDARY_PATH_POLYLINE)
            h.paths.add_polyline_path([(CIRCLE[0][0] - 1500, 10_000, 1), (CIRCLE[0][0] + 1500, 10_000, 1)], is_closed=True,
                                      flags=const.BOUNDARY_PATH_POLYLINE)
            h.paths.add_polyline_path([(x, y, 0) for x, y in rect_pts(*SQUARE)], is_closed=True, flags=const.BOUNDARY_PATH_POLYLINE)
            h.paths.add_polyline_path([(20_000, 8_000, math.tan(math.pi / 4)), (20_000, 12_000, 0)], is_closed=True,
                                      flags=const.BOUNDARY_PATH_POLYLINE)
        else:
            e = h.paths.add_edge_path(flags=ext)
            pts = lot_pts()
            for i in range(4):
                e.add_line(pts[i], pts[(i + 1) % 4])
            e = h.paths.add_edge_path()
            e.add_arc(CIRCLE[0], CIRCLE[1], 0, 360, ccw=True)
            e = h.paths.add_edge_path()
            pts = rect_pts(*SQUARE)
            for i in range(4):
                e.add_line(pts[i], pts[(i + 1) % 4])
            e = h.paths.add_edge_path()
            e.add_arc(HALF[0], HALF[1], -90, 90, ccw=True)
            e.add_line((HALF[0][0], HALF[0][1] + HALF[1]), (HALF[0][0], HALF[0][1] - HALF[1]))
        stall_lines(msp, MM, "STALLS")
        save(doc, name, desc=f"the lot is only a HATCH ({mode} paths)", gross_m2=600.0, stalls="STALLS",
             find={"how": "hatch"}, via="from_shapes")


def f10_hatch_ellipse_spline():
    """Hatch edge paths with an ELLIPSE edge (a kerb corner and an island) and a SPLINE edge."""
    a, b = 4_000.0, 2_500.0
    c = (W - a, H - b)
    doc = new("R2010", 4, ["HATCH", "STALLS"])
    msp = doc.modelspace()
    h = msp.add_hatch(color=252, dxfattribs={"layer": "HATCH"})
    e = h.paths.add_edge_path(flags=const.BOUNDARY_PATH_EXTERNAL)
    e.add_line((0, 0), (W, 0))
    e.add_line((W, 0), (W, c[1]))
    e.add_ellipse(c, major_axis=(a, 0), ratio=b / a, start_angle=0, end_angle=90, ccw=True)
    e.add_line((c[0], H), (0, H))
    e.add_line((0, H), (0, 0))
    e = h.paths.add_edge_path()
    tilt = math.radians(30)
    e.add_ellipse((6_000, 10_000), major_axis=(2000 * math.cos(tilt), 2000 * math.sin(tilt)), ratio=0.5, ccw=True)
    e = h.paths.add_edge_path()
    pts = rect_pts(*SQUARE)
    for i in range(4):
        e.add_line(pts[i], pts[(i + 1) % 4])
    e = h.paths.add_edge_path()
    e.add_arc(HALF[0], HALF[1], -90, 90, ccw=True)
    e.add_line((HALF[0][0], HALF[0][1] + HALF[1]), (HALF[0][0], HALF[0][1] - HALF[1]))
    stall_lines(msp, MM, "STALLS")
    save(doc, "10c_hatch_edge_ellipse_only.dxf", desc="the lot is only a HATCH with ELLIPSE edges",
         gross_m2=600.0 - a * b * (1 - math.pi / 4) / 1e6, islands_m2=2 * math.pi * 2.0 + 9.0, stalls="STALLS",
         find={"how": "hatch"}, via="from_shapes", area_tol=0.03)
    r, k = 3_000.0, 0.5522847498
    bez = [(W, H - r), (W, H - r + k * r), (W - r + k * r, H), (W - r, H)]
    doc = new("R2010", 4, ["HATCH", "STALLS"])
    msp = doc.modelspace()
    h = msp.add_hatch(color=252, dxfattribs={"layer": "HATCH"})
    e = h.paths.add_edge_path(flags=const.BOUNDARY_PATH_EXTERNAL)
    e.add_line((0, 0), (W, 0))
    e.add_line((W, 0), (W, H - r))
    e.add_spline(control_points=bez, knot_values=[0, 0, 0, 0, 1, 1, 1, 1], degree=3)
    e.add_line((W - r, H), (0, H))
    e.add_line((0, H), (0, 0))
    h.paths.add_polyline_path([(CIRCLE[0][0] - 1500, 10_000, 1), (CIRCLE[0][0] + 1500, 10_000, 1)], is_closed=True,
                              flags=const.BOUNDARY_PATH_POLYLINE)
    h.paths.add_polyline_path([(x, y, 0) for x, y in rect_pts(*SQUARE)], is_closed=True, flags=const.BOUNDARY_PATH_POLYLINE)
    h.paths.add_polyline_path([(20_000, 8_000, math.tan(math.pi / 4)), (20_000, 12_000, 0)], is_closed=True,
                              flags=const.BOUNDARY_PATH_POLYLINE)
    stall_lines(msp, MM, "STALLS")
    from ezdxf.math import BSpline
    flat = [(p.x, p.y) for p in BSpline(bez, order=4).flattening(0.02)]
    save(doc, "10d_hatch_edge_spline_only.dxf", desc="the lot is only a HATCH with a SPLINE edge",
         gross_m2=shoelace([(0, 0), (W, 0), *flat, (0, H)]), stalls="STALLS",
         find={"how": "hatch"}, via="from_shapes", area_tol=0.03)


def f11_r12_and_r2018():
    # R12: no LWPOLYLINE, old POLYLINE only, no $INSUNITS, code page ANSI_1257 (Estonian).
    doc = ezdxf.new("R12", setup=False)
    doc.header["$DWGCODEPAGE"] = "ANSI_1257"
    for n in ("LOT", "SAARED_õä", "STALLS"):
        doc.layers.add(n)
    msp = doc.modelspace()
    b = math.tan(math.pi / 8)
    r = 3000.0
    msp.add_polyline2d([(0, 0, 0), (W, 0, 0), (W, H - r, b), (W - r, H, 0), (0, H, 0)], format="xyb", close=True,
                       dxfattribs={"layer": "LOT"})
    isl_circle_entity(msp, MM, "SAARED_õä")
    msp.add_polyline2d(rect_pts(*SQUARE), close=True, dxfattribs={"layer": "SAARED_õä"})
    isl_half_arc_line(msp, MM, "SAARED_õä")
    stall_lines(msp, MM, "STALLS")
    msp.add_text("Parkla", dxfattribs={"layer": "LOT", "height": 300, "insert": (500, 500)})
    save(doc, "11a_r12_cp1257.dxf", desc="R12 file with old POLYLINE and the Estonian code page",
         gross_m2=gross_corner_arc(3000.0), stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "SAARED_õä"]}, via="from_curves", notes=["unit"])
    doc = new("R2018", 4, ["LOT", "ISLANDS", "STALLS"])
    std_simple(doc, MM)
    stall_lines(doc.modelspace(), MM, "STALLS")
    save(doc, "11b_r2018.dxf", desc="R2018 file", gross_m2=600.0, stalls="STALLS",
         find={"how": "layer", "layers": ["LOT", "ISLANDS"]}, via="from_shapes")


def f12_full_lot():
    """The standard lot with a rounded corner, islands of three kinds and all the stall lines."""
    doc = new("R2018", 4, ["LOT", "ISLANDS", "STALLS", "ANNO"])
    msp = doc.modelspace()
    lot_poly_arcs(msp, MM, "LOT", closed=True)
    isl_circle_entity(msp, MM, "ISLANDS")
    isl_square_lines(msp, MM, "ISLANDS")
    isl_half_arc_line(msp, MM, "ISLANDS")
    stall_lines(msp, MM, "STALLS")
    save(doc, "12_full_lot_3_islands_2_aisles.dxf", desc="3 islands, 2 drive aisles, 20 stalls, rounded corner",
         gross_m2=gross_corner_arc(3000.0), stalls="STALLS", find={"how": "layer", "layers": ["LOT", "ISLANDS"]})


def f13_largest_region():
    """No layer helps: everything is on layer 0 (a flattened export). The lot is the largest region."""
    doc = new("R2010", 4, [])
    soup(doc.modelspace(), MM, "0", seed=3)
    save(doc, "13_all_on_layer_0_largest_region.dxf", desc="everything on layer 0: find the lot as the largest closed region",
         gross_m2=600.0, tol=6.0, find={"how": "largest"})


def f14_soup_with_heights():
    """A survey: every LINE has a height (z = 100 m plus a few cm). The lot is still flat in plan."""
    doc = new("R2010", 4, ["LOT", "STALLS"])
    soup(doc.modelspace(), MM, "LOT", seed=5, islands=False, z=100_000.0)
    msp = doc.modelspace()
    isl_circle_entity(msp, MM, "LOT")
    isl_square_lines(msp, MM, "LOT", gap=1.0)
    isl_half_arc_line(msp, MM, "LOT", gap_deg=0.01)
    stall_lines(msp, MM, "STALLS")
    save(doc, "14_line_soup_with_heights.dxf", desc="LINE soup where every LINE has a z value (survey heights)",
         gross_m2=600.0, tol=6.0, stalls="STALLS")


def main():
    for f in (f01_soup, f02_lwpoly_bulge_open, f03_polyline_2d, f03_polyline_3d, f04_splines, f04_ellipse, f05_blocks,
              f06_units, f07_far, f08_layers, f09_noise, f10_hatch_only, f10_hatch_ellipse_spline, f11_r12_and_r2018, f12_full_lot, f13_largest_region, f14_soup_with_heights):
        f()
    for e in EXPECTED.values():
        e["zones"] = ZONES if e["stalls"] else None
    (HERE / "expected.json").write_text(json.dumps(EXPECTED, indent=1, ensure_ascii=False, sort_keys=True))
    print(len(EXPECTED), "files")


if __name__ == "__main__":
    main()
