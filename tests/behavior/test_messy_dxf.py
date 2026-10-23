"""Scenario: an agent tiles a parking lot from a DXF file as real CAD programs write it.

The corpus `fixtures/messy/` (made by `fixtures/messy/make_messy.py`, with the expected values in
`expected.json`) holds the same lot of 30 m by 20 m with three islands in many untidy ways: LINE
soup with gaps and overshoots, polylines with bulges and without the closed flag, old POLYLINEs,
SPLINE and ELLIPSE kerbs, blocks, all the units, survey coordinates, Unicode layer names, frozen and
off layers, paper space, noise, a lot that is only a HATCH, R12 and R2018 (feature spec IOP-01,
IOP-09, IOP-14, PAT-01 to PAT-12, HRD-12, HRD-13).

For each file the test runs the whole workflow through the API: import, find the lot, make the
hardscape, lay running bond and herringbone, zones for the stalls, a kerb course, the pattern
check, the report, the DXF export (units and blocks), and then reads the export again with ezdxf
(audit and recover) and compares the counts in the file with the report.
"""

import json
import math

import pytest

from helpers import REPO

MESSY = REPO / "tests" / "behavior" / "fixtures" / "messy"
CORPUS = json.loads((MESSY / "expected.json").read_text())
# Units are 400 x 200 with a 4 mm joint, so that a lot of 580 m2 has about 7,000 units.
UL, UW, GAP = 400, 200, 4
GEOMETRY = {"line", "arc", "circle", "polyline", "ellipse", "spline", "polyline3d", "rect", "bezier", "path"}


def classes(cad):
    return {c["name"]: c["id"] for c in cad.result("document.summary")["classes"]}


def objects_of(cad, class_names):
    """The objects of the classes on design layers. Paper space is imported as sheet layers (IOP-16),
    and its objects can share a class with the model, so they are left out here."""
    cls = classes(cad)
    sheets = {la["id"] for la in cad.result("document.summary")["layers"] if la["kind"] == "sheet"}
    out = []
    for n in class_names:
        assert n in cls, (n, sorted(cls))
        out += [o for o in cad.result("objects.list", {"class": cls[n]}) if o["layer"] not in sheets]
    return out


def import_file(cad, name, entry, fix=False):
    opts = dict(entry["import"])
    if fix:
        opts.update(entry.get("import_fix", {}))
    r = cad.result("import.file", {"path": str(MESSY / name), "options": opts})
    return r["report"]


def report_text(rep):
    return json.dumps([rep.get("notes"), rep.get("skipped")], ensure_ascii=False).lower()


def find_lot(cad, entry):
    """The way an agent finds the lot: the layers that the file uses, the hatch, or the largest region."""
    how = entry["find"]["how"]
    if how == "layer":
        objs = objects_of(cad, entry["find"]["layers"])
    elif how == "hatch":
        objs = objects_of(cad, ["HATCH"])
    else:
        objs = [o for o in cad.result("objects.list") if o["shape"]["kind"] in GEOMETRY]
    return [o["id"] for o in objs if o["shape"]["kind"] in GEOMETRY]


def make_style(cad, name, kind):
    return cad.result(
        "hardscape_style.create",
        {"name": name, "material": "Concrete pavers", "thickness": 80,
         "joints": {"kind": kind, "unit_length": UL, "unit_width": UW, "spacing": GAP}},
    )["id"]


def make_hardscape(cad, entry, ids, style):
    """The lot becomes one hardscape with the islands as holes."""
    if entry["find"]["how"] == "largest" or entry["via"] == "from_curves":
        dry = cad.result("region.from_curves", {"ids": ids, "tolerance": entry["tol"], "dry_run": True})
        # The lot is the region with the largest area at depth 0; the islands are its holes.
        areas = [(r["area_m2"], i) for i, r in enumerate(dry["regions"]) if r["depth"] == 0 and r["role"] == "area"]
        assert areas, dry
        best = max(areas)[1]
        made = cad.result(
            "region.from_curves",
            {"ids": ids, "tolerance": entry["tol"], "create": "hardscape", "style": style, "regions": [best]},
        )
        assert len(made["created"]) == 1, made
        assert made["dead_ends"]["open"] == 0, made["dead_ends"]
        return made["created"][0]["id"]
    made = cad.result("hardscape.from_shapes", {"ids": ids, "style": style})
    # The hardscape with the most holes is the lot (a frozen or off copy would be another one).
    assert len(made["created"]) == 1, made
    return made["created"][0]["id"]


def hardscape_info(cad, hid):
    return cad.result("objects.get", {"id": hid})["data"]["hardscape"]


def all_units(cad, hid):
    units, off = [], 0
    while True:
        r = cad.result("unit_pattern.layout", {"id": hid, "include": ["units"], "offset": off, "limit": 20_000})
        units += r["units"]
        if not r.get("next_offset") or len(units) >= r["units_total"]:
            return units
        off = r["next_offset"]


def read_dxf(path):
    """Audit and recover the export; the counts of whole and cut units by layer."""
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    from ezdxf import recover

    dxf = ezdxf.readfile(str(path))
    audit = dxf.audit()
    assert not audit.errors, [x.message for x in audit.errors]
    _, rec = recover.readfile(str(path))
    assert not rec.errors, [x.message for x in rec.errors]
    whole = cut = inserts = 0
    for x in dxf.modelspace():
        layer = x.dxf.layer
        if layer.endswith("-UNITS-WHOLE"):
            whole += 1
            inserts += x.dxftype() == "INSERT"
        elif layer.endswith("-UNITS-CUT"):
            cut += 1
    return whole, cut, inserts


def flatten(vertices):
    """Corners of a ring; a vertex of kind point_on_arc lies on the arc through its neighbours."""
    pts = [tuple(v["p"]) if isinstance(v, dict) else tuple(v) for v in vertices]
    kinds = [v.get("kind") if isinstance(v, dict) else None for v in vertices]
    out, n = [], len(pts)
    for i, p in enumerate(pts):
        out.append(p)
        if kinds[(i + 1) % n] == "point_on_arc" and kinds[i] != "point_on_arc":
            a, b, c = p, pts[(i + 1) % n], pts[(i + 2) % n]
            d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]))
            if abs(d) < 1e-9:
                continue
            ux = ((a[0] ** 2 + a[1] ** 2) * (b[1] - c[1]) + (b[0] ** 2 + b[1] ** 2) * (c[1] - a[1]) + (c[0] ** 2 + c[1] ** 2) * (a[1] - b[1])) / d
            uy = ((a[0] ** 2 + a[1] ** 2) * (c[0] - b[0]) + (b[0] ** 2 + b[1] ** 2) * (a[0] - c[0]) + (c[0] ** 2 + c[1] ** 2) * (b[0] - a[0])) / d
            ang = lambda q: math.atan2(q[1] - uy, q[0] - ux)
            r = math.hypot(a[0] - ux, a[1] - uy)
            a0, a1, a2 = ang(a), ang(b), ang(c)
            ccw = ((a1 - a0) % math.tau) < ((a2 - a0) % math.tau)
            sweep = (a2 - a0) % math.tau if ccw else -((a0 - a2) % math.tau)
            for k in range(1, 40):
                t = a0 + sweep * k / 40
                out.append((ux + r * math.cos(t), uy + r * math.sin(t)))
    return out


def hole_polygons(cad, hid):
    return [flatten(h) for h in hardscape_info(cad, hid)["holes"]]


def edge_distance(poly, p):
    best = float("inf")
    for (x1, y1), (x2, y2) in zip(poly, poly[1:] + poly[:1]):
        dx, dy = x2 - x1, y2 - y1
        t = 0.0 if dx == dy == 0 else max(0.0, min(1.0, ((p[0] - x1) * dx + (p[1] - y1) * dy) / (dx * dx + dy * dy)))
        best = min(best, math.hypot(p[0] - x1 - t * dx, p[1] - y1 - t * dy))
    return best


def inside(poly, p):
    c = False
    for (x1, y1), (x2, y2) in zip(poly, poly[1:] + poly[:1]):
        if (y1 > p[1]) != (y2 > p[1]) and p[0] < (x2 - x1) * (p[1] - y1) / (y2 - y1) + x1:
            c = not c
    return c


def assert_islands_unpaved(cad, hid):
    """No point on the edges of a unit lies more than 2 mm inside an island (an arc is cut into chords of about 1 mm)."""
    holes = hole_polygons(cad, hid)
    boxes = [(min(p[0] for p in h), min(p[1] for p in h), max(p[0] for p in h), max(p[1] for p in h)) for h in holes]
    units = all_units(cad, hid)
    assert units
    for u in units:
        ring = u["outline"]
        xs, ys = [p[0] for p in ring], [p[1] for p in ring]
        for h, (x0, y0, x1, y1) in zip(holes, boxes):
            if max(xs) < x0 or min(xs) > x1 or max(ys) < y0 or min(ys) > y1:
                continue
            for (ax, ay), (bx, by) in zip(ring, ring[1:] + ring[:1]):
                for t in (0.0, 0.25, 0.5, 0.75):
                    p = (ax + (bx - ax) * t, ay + (by - ay) * t)
                    assert not (inside(h, p) and edge_distance(h, p) > 2.0), ("unit on an island", ring)
    return len(units)


def dxf_extent(path):
    import ezdxf

    dxf = ezdxf.readfile(str(path))
    xs, ys = [], []
    for x in dxf.modelspace():
        if x.dxf.layer.endswith("-UNITS-CUT") and x.dxftype() == "LWPOLYLINE":
            for p in x.get_points():
                xs.append(p[0])
                ys.append(p[1])
    return min(xs), min(ys), max(xs), max(ys), int(dxf.header.get("$INSUNITS", 0))


def test_the_corpus_is_complete():
    names = sorted(p.name for p in MESSY.glob("*.dxf"))
    assert names == sorted(CORPUS), "run fixtures/messy/make_messy.py again"
    assert len(names) >= 25


# The files that run by default: one of each kind of trouble. The others are marked `slow`
# (about 20 to 55 seconds each in a debug build); run them with KADAKAS_SLOW=1 or `-m slow`.
FAST = {
    "01_line_soup_mm.dxf",
    "02_lwpolyline_bulge_unclosed.dxf",
    "06e_units_foot.dxf",
    "07a_survey_m_6588000_north_soup.dxf",
    "10b_hatch_edge_paths_only.dxf",
    "10e_hatch_clockwise_edges_only.dxf",
    "12_full_lot_3_islands_2_aisles.dxf",
}
assert FAST <= set(CORPUS), FAST - set(CORPUS)


@pytest.mark.parametrize(
    "name",
    [pytest.param(n, marks=[] if n in FAST else pytest.mark.slow) for n in sorted(CORPUS)],
)
def test_the_whole_workflow_on_a_messy_file(cad, make_serve, tmp_path, name):
    e = CORPUS[name]
    if "import_fix" in e:
        # Without the option the unit is a guess, and the report says so.
        rep = import_file(make_serve(), name, e)
        assert "unit" in report_text(rep) and "assum" in report_text(rep), rep
    else:
        rep = None
    imported = import_file(cad, name, e, fix=True)
    rep = rep or imported
    text = report_text(rep)
    for word in e["notes"]:
        assert word.lower() in text, (word, rep["notes"], rep["skipped"])

    ids = find_lot(cad, e)
    assert ids
    rb = make_style(cad, "Running bond", "running_bond")
    hb = make_style(cad, "Herringbone", "herringbone")
    hid = make_hardscape(cad, e, ids, rb)
    assert len(cad.result("report.hardscape")["rows"]) == 1
    row = cad.result("report.hardscape")["rows"][0]
    assert row["plan_area_m2"] == pytest.approx(e["net_m2"], abs=e["area_tol"]), row
    assert row["holes"] == 3

    # Running bond and herringbone on the lot: whole and cut units, and the area adds up.
    for style in (rb, hb):
        cad.result("hardscape.set", {"id": hid, "style": style})
        rep = cad.result("report.unit_pattern", {"ids": [hid]})
        r = rep["rows"][0]
        assert rep["totals"]["complete"] is True
        assert r["region_area_m2"] == pytest.approx(e["net_m2"], abs=e["area_tol"])
        assert r["area_laid_m2"] + r["gap_area_m2"] == pytest.approx(r["region_area_m2"], abs=1e-6)
        assert r["whole_units"] > 6000 and r["cut_units"] > 100

    # Zones for the stalls, where the file has stall lines: 20 stalls and the aisles.
    if e["stalls"]:
        stall_ids = [o["id"] for o in objects_of(cad, [e["stalls"]])]
        assert len(stall_ids) == 3 + 18
        z = cad.result("hardscape.split", {"id": hid, "lines": stall_ids, "tolerance": max(e["tol"], 3.0)})
        assert len(z["zones"]) == e["zones"], [zn["area_m2"] for zn in z["zones"]]
        assert z["zones_area_m2"] == pytest.approx(z["area_m2"], abs=1e-6)
        assert z["area_m2"] == pytest.approx(e["net_m2"], abs=e["area_tol"])
        assert all(zn["area_m2"] > 1.0 for zn in z["zones"])

    # A soldier course along the kerb (the outline of the lot).
    c = cad.result(
        "hardscape.course",
        {"id": hid, "courses": [{"name": "Kerb", "along": "outline", "corner": "corner_unit", "align_field": True}]},
    )
    assert c["courses"][0]["units"] > 100

    for style, bond in ((rb, "running_bond"), (hb, "herringbone")):
        cad.result("hardscape.set", {"id": hid, "style": style})
        r = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
        chk = cad.result("unit_pattern.check", {"ids": [hid]})["objects"][0]
        for kind in ("overlap", "outside_area", "wrong_size", "joint_too_narrow"):
            assert chk["counts"][kind] == 0, (bond, kind, chk["findings"][:3])
        # Straight units on a curved kerb leave small wedges (a documented taper, PAT-08): they
        # are findings of the course only, and a lot with straight edges has none.
        wedges = [f for f in chk["findings"] if f["kind"] == "coverage"]
        assert all(f["group"] == "Kerb" for f in wedges), wedges[:3]
        if abs(e["gross_m2"] - 600.0) < 1e-6:
            assert not wedges
        assert chk["units"] == r["whole_units"] + r["cut_units"]
        assert_islands_unpaved(cad, hid)
        for mode in ("units", "blocks"):
            out = tmp_path / f"{bond}-{mode}.dxf"
            cad.result("export.file", {"path": str(out), "format": "dxf", "options": {"patterns": mode}})
            whole, cut, inserts = read_dxf(out)
            assert (whole, cut) == (r["whole_units"], r["cut_units"]), (bond, mode)
            assert (inserts == whole) == (mode == "blocks")
        # The export is in real-world coordinates: the corner of the lot is where the file had it.
        x0, y0, x1, y1, units = dxf_extent(out)
        assert units == 4
        ox, oy = (v * e["unit_mm"] for v in e["origin"])
        assert (x0, y0) == pytest.approx((ox, oy), abs=max(5.0, abs(ox) * 1e-12)), (x0, y0, ox, oy)




def lay_the_stalls(cad, noisy):
    """The 30 m by 20 m lot with stall zones and a kerb course; `noisy`: every number went through
    feet (x / 304.8 * 304.8) and carries noise in the last bits."""
    W, H = 30_000.0, 20_000.0
    f = (lambda v: v / 304.8 * 304.8) if noisy else (lambda v: v)
    st = make_style(cad, "R", "running_bond")
    ring = [[f(0), f(0)], [f(W), f(0)], [f(W), f(H)], [f(0), f(H)]]
    pid = cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": ring}})[0]
    hid = cad.result("hardscape.from_shapes", {"ids": [pid], "style": st})["created"][0]["id"]
    lines = [[[f(0), f(5000)], [f(W), f(5000)]], [[f(0), f(15000)], [f(W), f(15000)]], [[f(25000), f(0)], [f(25000), f(H)]]]
    for k in range(1, 10):
        x = 2500.0 * k
        lines.append([[f(x), f(0)], [f(x), f(5000)]])
        lines.append([[f(x), f(15000)], [f(x), f(H)]])
    cad.result("hardscape.split", {"id": hid, "lines": lines, "tolerance": 3})
    cad.result("hardscape.course", {"id": hid, "courses": [{"name": "Kerb", "along": "outline", "corner": "corner_unit", "align_field": True}]})
    r = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    chk = cad.result("unit_pattern.check", {"ids": [hid]})["objects"][0]
    return r["whole_units"], r["cut_units"], chk["counts"]["coverage"]


def test_a_lot_in_feet_keeps_every_unit_with_zones_and_a_course(cad, make_serve):
    """Coordinates that went through a conversion (feet to mm) carry noise in the last bits. The
    zones, the course and the units must not lose a unit because of it: ten whole units next to
    the kerb were lost once (the same lot in exact numbers is the reference)."""
    exact = lay_the_stalls(cad, False)
    noisy = lay_the_stalls(make_serve(), True)
    assert exact[2] == 0
    assert noisy == exact


def test_hardscape_from_shapes_reads_a_3d_polyline_in_plan_and_says_so(cad):
    """The lot of file 03b is a POLYLINE 3D with four heights, its square island is another."""
    name = "03b_polyline_3d_heights.dxf"
    import_file(cad, name, CORPUS[name])
    ids = [o["id"] for o in objects_of(cad, ["LOT", "ISLANDS"]) if o["shape"]["kind"] == "polyline3d"]
    assert len(ids) == 2
    made = cad.result("hardscape.from_shapes", {"ids": ids, "style": make_style(cad, "R", "stack")})
    assert len(made["created"]) == 1 and made["created"][0]["holes"] == 1
    assert any("heights" in w for w in made["warnings"]), made
    row = cad.result("report.hardscape")["rows"][0]
    assert row["plan_area_m2"] == pytest.approx(600.0 - 9.0, abs=1e-6)
    # Flat 3D polylines need no warning.
    flat = cad.execute({"op": "add_object", "shape": {"kind": "polyline3d", "closed": True, "points": [[0, 0, 5], [1000, 0, 5], [1000, 1000, 5]]}})[0]
    made = cad.result("hardscape.from_shapes", {"ids": [flat], "dry_run": True})
    assert made["warnings"] == []
