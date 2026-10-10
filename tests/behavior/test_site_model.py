"""Scenario: a site model (terrain) from a grid of 3D loci, with a pad, contours and cut and fill."""

from pytest import approx

from helpers import parse_svg, svg_object_ids


def locus_grid(cad, n=9, step=1000, slope=0.1):
    """n by n loci, `step` mm apart. The ground rises toward +X by `slope`."""
    ids = []
    for i in range(n):
        for j in range(n):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * step, j * step, i * step * slope]}})
    return ids


def make_site(cad, **settings):
    ids = locus_grid(cad)
    site = cad.result("site.create", {"source_ids": ids, "settings": settings})["id"]
    return site, ids


def test_create_moves_sources_into_the_site_model(cad):
    site, ids = make_site(cad)
    assert cad.result("objects.list", {"kind": "locus3d"}) == []
    obj = cad.result("objects.get", {"id": site})
    assert obj["shape"]["kind"] == "site_model"
    assert len(obj["shape"]["points"]) == 81
    assert cad.result("document.summary")["objects_by_kind"] == {"site_model": 1}
    # One undo step gives the loci back.
    assert cad.result("undo")["done"]
    assert len(cad.result("objects.list", {"kind": "locus3d"})) == 81
    assert cad.result("objects.list", {"kind": "site_model"}) == []


def test_elevation_at_points(cad):
    site, _ = make_site(cad)
    # Source points keep their elevation, other points interpolate.
    assert cad.result("site.elevation", {"id": site, "x": 3000, "y": 5000})["z"] == approx(300)
    r = cad.result("site.elevation", {"id": site, "x": 2500, "y": 3100})
    assert r["z"] == approx(250)
    assert r["slope"] == approx(0.1)
    assert r["aspect"] == approx(3 * 3.141592653589793 / 2)  # faces west
    assert cad.result("site.elevation", {"id": site, "x": -500, "y": 0})["z"] is None


def test_contour_count_at_an_interval(cad):
    site, _ = make_site(cad, minor_interval=100, major_interval=400)
    c = cad.result("site.contours", {"id": site, "points": False})
    # The ground goes from 0 to 800 mm: lines at 100, 200, ..., 800.
    assert c["levels"] == [100, 200, 300, 400, 500, 600, 700, 800]
    assert c["count"] == 8
    assert c["major_count"] == 2
    c = cad.result("site.contours", {"id": site})
    first = c["contours"][0]
    assert not first["closed"]
    assert all(p[0] == approx(1000) for p in first["points"])
    # A finer interval gives more lines.
    cad.result("site.update", {"id": site, "settings": {"minor_interval": 50, "major_interval": 100}})
    assert cad.result("site.contours", {"id": site, "points": False})["count"] == 16


def test_pad_gives_expected_cut_and_fill(cad):
    # Flat ground at 0 over 8 m by 8 m. A 2 m by 2 m pad 0.5 m above it, with a grade limit 0.5 m wider on each side.
    ids = []
    for i in range(9):
        for j in range(9):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, 0]}})
    site = cad.result("site.create", {"source_ids": ids})["id"]
    square = lambda lo, hi: [[lo, lo], [hi, lo], [hi, hi], [lo, hi]]
    cad.execute({"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "grade_limit"}, "boundary": square(2500, 5500)}})
    cad.execute({"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 500}, "boundary": square(3000, 5000)}})
    cf = cad.result("site.cut_fill", {"id": site})
    assert cf["cut_m3"] == approx(0, abs=1e-9)
    # Pad: 2 x 2 x 0.5 = 2 m3. Slope ring from 2 m wide at 0.5 m to 3 m wide at 0: a frustum minus the box.
    frustum = 0.5 / 3 * (4 + 9 + 6)
    assert cf["fill_m3"] == approx(frustum, rel=1e-9)
    assert cf["net_m3"] == approx(frustum, rel=1e-9)
    assert cf["area_m2"] == approx(64)
    # The ground changes only inside the grade limit.
    assert cad.result("site.elevation", {"id": site, "x": 4000, "y": 4000})["z"] == approx(500)
    assert cad.result("site.elevation", {"id": site, "x": 4000, "y": 4000, "surface": "existing"})["z"] == approx(0)
    assert cad.result("site.elevation", {"id": site, "x": 7000, "y": 7000})["z"] == approx(0)
    # The pad lowers the ground below: cut and fill swap sides.
    pad = cad.result("objects.list", {"kind": "site_modifier"})[1]["id"]
    shape = cad.result("objects.get", {"id": pad})["shape"]
    shape["modifier"]["elevation"] = -500
    cad.execute({"op": "modify_object", "id": pad, "shape": shape})
    cf = cad.result("site.cut_fill", {"id": site})
    assert cf["fill_m3"] == approx(0, abs=1e-9)
    assert cf["cut_m3"] == approx(frustum, rel=1e-9)


def test_svg_shows_contours_by_display_choice(cad):
    site, _ = make_site(cad, minor_interval=200, major_interval=400)
    svg = cad.result("render.svg", {"width": 600})["svg"]
    root = parse_svg(svg)
    contours = [e for e in root.iter() if e.get("data-kind") == "contour"]
    assert len(contours) == 4
    assert {e.get("data-surface") for e in contours} == {"existing"}
    # Contours do not add a second data-id: every object is drawn once.
    ids = svg_object_ids(svg)
    assert len(ids) == len(set(ids))
    cad.result("site.update", {"id": site, "settings": {"label_contours": True}})
    assert "contour-label" in cad.result("render.svg", {"width": 600})["svg"]


def test_update_adds_sources_and_checks_input(cad):
    site, _ = make_site(cad)
    contour = cad.execute({"op": "add_object", "shape": {"kind": "polyline3d", "closed": False, "points": [[0, 0, 0], [1000, 0, 100]]}})[0]
    r = cad.result("site.update", {"id": site, "add_source_ids": [contour]})
    assert (r["points"], r["lines"]) == (81, 1)
    assert cad.result("objects.list", {"kind": "polyline3d"}) == []
    circle = cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}})[0]
    assert "only locus3d" in cad.error("site.create", {"source_ids": [circle]})["message"]
    assert "positive" in cad.error("site.update", {"id": site, "settings": {"minor_interval": 0}})["message"]
    assert cad.error("site.elevation", {"id": circle, "x": 0, "y": 0})["code"] == 1
    assert "not a site model" in cad.error("site.cut_fill", {"id": circle})["message"]


def test_site_methods_reach_mcp(mcp):
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert {"site.create", "site.update", "site.elevation", "site.cut_fill", "site.contours"} <= set(tools["site"]["inputSchema"]["properties"]["method"]["enum"])
    ids = []
    for p in [[0, 0, 0], [10000, 0, 1000], [0, 10000, 2000], [10000, 10000, 3000]]:
        r = mcp.method_json("document", "execute", {"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": p}}})
        ids += r["created"]
    site = mcp.method_json("site", "site.create", {"source_ids": ids})["id"]
    z = mcp.method_json("site", "site.elevation", {"id": site, "x": 5000, "y": 5000})["z"]
    assert z == approx(1500)
