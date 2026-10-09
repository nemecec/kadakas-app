"""Scenario: import DXF, GeoJSON and Shapefile files through the API, and export again."""

import json
import math
import struct

from pytest import approx

import pytest

from helpers import REPO, JsonRpc

FIXTURES = REPO / "crates" / "cadektor-io" / "tests" / "fixtures"


def objects(cad, **filt):
    return cad.result("objects.list", filt)


def class_names(cad):
    return {c["name"]: c for c in cad.result("document.summary")["classes"]}


def test_describe_lists_the_file_methods_and_options(cad):
    d = cad.result("describe")
    names = [m["name"] for m in d["methods"]]
    assert "import.file" in names and "export.file" in names
    assert "layer_mode" in d["io"]["import_options"]


def test_import_dxf_as_classes_with_report_and_undo(cad):
    r = cad.result("import.file", {"path": str(FIXTURES / "site.dxf")})
    assert r["imported"] == 7
    assert r["skipped"] == 1
    assert r["report"]["skipped"][0]["entity"] == "DIMENSION"
    assert r["georeference"] is None
    summary = cad.result("document.summary")
    assert summary["object_count"] == 7
    assert len(summary["layers"]) == 1
    classes = class_names(cad)
    assert {"Walls", "Planting", "Survey"} <= set(classes)
    assert classes["Walls"]["attrs"]["pen"] == [255, 0, 0, 255]
    # Units are meters in the file: a circle of 0.75 m has a radius of 750 mm.
    (circle,) = objects(cad, kind="circle")
    assert circle["shape"]["radius"] == approx(750)
    assert circle["shape"]["center"] == approx([3000, 3000])
    # The survey point keeps its Z, in mm.
    (locus,) = objects(cad, kind="locus3d")
    assert locus["shape"]["p"] == approx([1000, 1000, 12500])
    # The room outline keeps its arc as an exact vertex.
    (room,) = [o for o in objects(cad, kind="polyline") if o["shape"]["closed"]]
    assert any(isinstance(v, dict) and v["kind"] == "point_on_arc" for v in room["shape"]["points"])
    # One undo takes the whole import back.
    assert cad.result("undo")["done"] is True
    assert cad.result("document.summary")["object_count"] == 0
    assert cad.result("undo")["done"] is False


def test_import_dxf_layers_as_design_layers(cad):
    cad.result("import.file", {"path": str(FIXTURES / "site.dxf"), "options": {"layer_mode": "layers"}})
    names = [layer["name"] for layer in cad.result("document.summary")["layers"]]
    assert names == ["Layer 1", "Walls", "Planting", "Survey"]
    assert len(cad.result("document.summary")["classes"]) == 1


def test_import_dxf_unit_option(cad, tmp_path):
    path = tmp_path / "feet.dxf"
    path.write_text("0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n10\n21\n0\n0\nENDSEC\n0\nEOF\n")
    cad.result("import.file", {"path": str(path), "options": {"unit": "foot", "layer": "Survey"}})
    (line,) = objects(cad, kind="line")
    assert line["shape"]["b"] == approx([3048, 0])
    assert "Survey" in [layer["name"] for layer in cad.result("document.summary")["layers"]]


def test_import_geojson_data_and_georeference(cad):
    r = cad.result("import.file", {"path": str(FIXTURES / "trees.geojson")})
    assert r["imported"] == 4
    geo = r["georeference"]
    assert geo["crs"] == "EPSG:32635"
    # Coordinates stay small although UTM values are in the millions.
    for o in objects(cad, kind="locus3d"):
        assert max(abs(o["shape"]["p"][0]), abs(o["shape"]["p"][1])) < 1_000_000
    trees = {o["data"]["species"]: o for o in objects(cad, kind="locus3d")}
    birch, oak = trees["Betula pendula"], trees["Quercus robur"]
    assert birch["data"]["trunk_cm"] == 32
    assert birch["data"]["protected"] is True
    assert oak["data"]["protected"] is False
    assert birch["shape"]["p"][2] == approx(12500)
    # 0.0006 degrees of longitude and 0.0002 of latitude at 60.17 N. UTM turns the grid by
    # about 1.8 degrees here, so compare the distance and not the single axes.
    dx = oak["shape"]["p"][0] - birch["shape"]["p"][0]
    dy = oak["shape"]["p"][1] - birch["shape"]["p"][1]
    east = 0.0006 * 111_412 * math.cos(math.radians(60.17))
    north = 0.0002 * 111_200
    assert math.hypot(dx, dy) == approx(math.hypot(east, north) * 1000, rel=0.01)
    # The polygon and its hole are two closed polylines with the same attributes.
    polys = [o for o in objects(cad, kind="polyline")]
    assert len(polys) == 2
    assert all(o["data"]["name"] == "Plot 1" for o in polys)
    assert sorted(o["data"].get("_ring", "outer") for o in polys) == ["hole", "outer"]
    assert "trees" in class_names(cad)
    assert cad.result("document.summary")["georeference"]["crs"] == "EPSG:32635"


def test_import_projected_geojson_keeps_metric_distances(cad, tmp_path):
    path = tmp_path / "points.geojson"
    path.write_text(json.dumps({
        "type": "FeatureCollection",
        "crs": {"type": "name", "properties": {"name": "urn:ogc:def:crs:EPSG::3067"}},
        "features": [
            {"type": "Feature", "geometry": {"type": "Point", "coordinates": [385000, 6672000]}, "properties": {"id": 1}},
            {"type": "Feature", "geometry": {"type": "Point", "coordinates": [385010, 6672020, 5]}, "properties": {"id": 2}},
        ],
    }))
    r = cad.result("import.file", {"path": str(path), "options": {"class_field": "id"}})
    assert r["georeference"]["crs"] == "EPSG:3067"
    assert r["georeference"]["origin"] == [385005.0, 6672010.0]
    pts = sorted((o["shape"]["p"] for o in objects(cad, kind="locus3d")), key=lambda p: p[0])
    for got, want in zip(pts, [[-5000, -10000, 0], [5000, 10000, 5000]], strict=True):
        assert got == approx(want, abs=1e-3)
    assert {"1", "2"} <= set(class_names(cad))


def test_second_import_uses_the_document_georeference(cad, tmp_path):
    cad.result("execute", {"command": {"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000, 6672000]}}})
    path = tmp_path / "p.geojson"
    path.write_text(json.dumps({"type": "Point", "coordinates": [385010, 6672020]}))
    cad.result("import.file", {"path": str(path), "options": {"source_crs": "EPSG:3067"}})
    (p,) = objects(cad, kind="locus3d")
    assert p["shape"]["p"] == approx([10000, 20000, 0])
    assert cad.result("document.summary")["georeference"]["origin"] == [385000, 6672000]


def shapefile_bytes(dx=0.0, dy=0.0):
    ring = [(dx + x, dy + y) for x, y in [(0, 0), (0, 10), (10, 10), (10, 0), (0, 0)]]  # clockwise: outer ring
    body = struct.pack("<i4d2i", 5, dx, dy, dx + 10, dy + 10, 1, len(ring)) + struct.pack("<i", 0)
    body += b"".join(struct.pack("<2d", x, y) for x, y in ring)
    header = bytearray(100)
    header[0:4] = struct.pack(">i", 9994)
    header[28:32] = struct.pack("<i", 1000)
    header[32:36] = struct.pack("<i", 5)
    return bytes(header) + struct.pack(">2i", 1, len(body) // 2) + body


def dbf_bytes(value=b"Lawn"):
    fields = [(b"NAME", b"C", 8), (b"AREA", b"N", 6)]
    rec_len = 1 + sum(f[2] for f in fields)
    head_len = 32 + 32 * len(fields) + 1
    out = bytearray(32)
    out[0] = 3
    out[4:8] = struct.pack("<i", 1)
    out[8:10] = struct.pack("<H", head_len)
    out[10:12] = struct.pack("<H", rec_len)
    for name, kind, size in fields:
        d = bytearray(32)
        d[: len(name)] = name
        d[11] = kind[0]
        d[16] = size
        out += d
    out += b"\r"
    out += b" " + value.ljust(8) + b"42".rjust(6)
    return bytes(out)


def test_import_shapefile_with_attributes_and_crs(cad, tmp_path):
    (tmp_path / "plots.shp").write_bytes(shapefile_bytes())
    (tmp_path / "plots.dbf").write_bytes(dbf_bytes())
    prj = ('PROJCS["ETRS89 / TM35FIN",GEOGCS["ETRS89",DATUM["European_Terrestrial_Reference_System_1989",'
           'SPHEROID["GRS 1980",6378137,298.257222101]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],'
           'PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",27],'
           'PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],'
           'UNIT["metre",1],AUTHORITY["EPSG","3067"]]')
    (tmp_path / "plots.prj").write_text(prj)
    r = cad.result("import.file", {"path": str(tmp_path / "plots.shp")})
    assert r["imported"] == 1
    assert r["georeference"]["crs"] == "EPSG:3067"
    assert r["georeference"]["origin"] == [5.0, 5.0]
    (poly,) = objects(cad, kind="polyline")
    assert poly["data"] == {"NAME": "Lawn", "AREA": 42, "_feature": 0}
    xs = sorted(v[0] if isinstance(v, list) else v["p"][0] for v in poly["shape"]["points"])
    assert xs == approx([-5000, -5000, 5000, 5000])
    assert "plots" in class_names(cad)


def test_export_dxf_and_import_again(cad, make_serve, tmp_path):
    cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": [
        [0, 0], {"p": [1500, -800], "kind": "point_on_arc"}, [3000, 0], [3000, 2000], [0, 2000]]}})
    cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [500, 500], "radius": 250}})
    cad.execute({"op": "add_object", "shape": {"kind": "plant", "at": [4000, 1000], "species": "Acer", "spread": 3000, "height": 6000}})
    cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [10, 20, 30]}})
    out = tmp_path / "out.dxf"
    r = cad.result("export.file", {"path": str(out)})
    assert r["exported"] == 4
    text = out.read_text()
    assert "LWPOLYLINE" in text and "\n42\n" in text  # a bulge
    assert text.rstrip().endswith("EOF")
    other = make_serve()
    r = other.result("import.file", {"path": str(out)})
    assert r["skipped"] == 0
    (poly,) = objects(other, kind="polyline")
    assert any(isinstance(v, dict) and v["kind"] == "point_on_arc" for v in poly["shape"]["points"])
    circles = sorted(o["shape"]["radius"] for o in objects(other, kind="circle"))
    assert circles == approx([250, 1500])  # the circle and the plant canopy
    (locus,) = objects(other, kind="locus3d")
    assert locus["shape"]["p"] == approx([10, 20, 30])


def test_export_geojson_with_georeference(cad, tmp_path):
    cad.result("execute", {"command": {"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000, 6672000]}}})
    cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [0, 0, 2000]}})
    (oid,) = [o["id"] for o in objects(cad)]
    cad.result("execute", {"command": {"op": "set_data", "id": oid, "set": {"species": "Oak"}}})
    out = tmp_path / "out.geojson"
    cad.result("export.file", {"path": str(out)})
    fc = json.loads(out.read_text())
    (feature,) = fc["features"]
    lon, lat, z = feature["geometry"]["coordinates"]
    assert (lon, lat) == approx((24.92, 60.2), abs=0.1)
    assert z == approx(2.0)
    assert feature["properties"]["species"] == "Oak"


def test_errors(cad, tmp_path):
    assert cad.error("import.file", {"path": str(tmp_path / "missing.dxf")})["code"] == 1
    assert cad.error("import.file", {"path": "x.dxf", "format": "png"})["code"] == -32602
    assert cad.error("import.file", {"path": "x.dxf", "options": {"unit": "parsec"}})["code"] == -32602
    bad = tmp_path / "bad.dxf"
    bad.write_bytes(b"\x00\x01 not a dxf")
    err = cad.error("import.file", {"path": str(bad)})
    assert "not a DXF" in err["message"]
    garbage = tmp_path / "garbage.geojson"
    garbage.write_text("{" * 10)
    assert cad.error("import.file", {"path": str(garbage)})["code"] == 1
    assert cad.error("export.file", {"path": str(tmp_path / "x.shp")})["code"] == 1
    assert cad.result("document.summary")["object_count"] == 0


def dxf_text(entities, header="", blocks=""):
    return f"0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"


def test_real_world_dxf_in_meters_is_centered_with_the_shift_recorded(cad, tmp_path):
    # L-EST97 survey coordinates: 6.5 million meters is beyond the 1e9 mm range of the model.
    path = tmp_path / "survey.dxf"
    path.write_text(dxf_text(
        "0\nLINE\n8\nA\n10\n6589000\n20\n540000\n11\n6589100\n21\n540050\n"
        "0\nCIRCLE\n8\nA\n10\n6589050\n20\n540025\n40\n2\n", header="9\n$INSUNITS\n70\n6\n"))
    r = cad.result("import.file", {"path": str(path)})
    assert r["imported"] == 2 and r["skipped"] == 0
    assert r["georeference"]["crs"] == ""
    assert r["georeference"]["origin"] == [6589050.0, 540025.0]
    (circle,) = objects(cad, kind="circle")
    assert circle["shape"]["center"] == approx([0, 0]) and circle["shape"]["radius"] == approx(2000)
    # `file` keeps the numbers of the file: everything is out of range.
    other = JsonRpc(["serve", "--allow-dir", str(tmp_path)])
    try:
        r = other.result("import.file", {"path": str(path), "options": {"origin": "file"}})
        assert r["imported"] == 0 and r["skipped"] == 2 and r["georeference"] is None
    finally:
        other.close()


def test_shapefile_without_prj_far_from_the_origin_is_centered(cad, tmp_path):
    (tmp_path / "plots.shp").write_bytes(shapefile_bytes(6589000.0, 540000.0))
    r = cad.result("import.file", {"path": str(tmp_path / "plots.shp")})
    assert r["imported"] == 1
    assert r["georeference"]["crs"] == "" and r["georeference"]["origin"] == [6589005.0, 540005.0]
    (poly,) = objects(cad, kind="polyline")
    xs = sorted(v[0] if isinstance(v, list) else v["p"][0] for v in poly["shape"]["points"])
    assert xs == approx([-5000, -5000, 5000, 5000])


FEET_GEOJSON = {
    "type": "FeatureCollection",
    "crs": {"type": "name", "properties": {"name": "urn:ogc:def:crs:EPSG::2263"}},  # NY Long Island, US survey feet
    "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [986000, 212000]}, "properties": {}}],
}


def test_feet_crs_lands_in_the_right_place(cad, tmp_path):
    path = tmp_path / "ny.geojson"
    path.write_text(json.dumps(FEET_GEOJSON))
    r = cad.result("import.file", {"path": str(path)})
    # 986000 ft is about 300 km. The origin is stored in meters.
    assert r["georeference"]["crs"] == "EPSG:2263"
    assert r["georeference"]["origin"] == approx([300533, 64617], abs=2)
    out = tmp_path / "back.geojson"
    cad.result("export.file", {"path": str(out)})
    lon, lat, _ = json.loads(out.read_text())["features"][0]["geometry"]["coordinates"]
    assert (lon, lat) == approx((-73.9972, 40.7421), abs=0.02)


def test_projected_prj_does_not_take_the_authority_of_its_base_geographic_crs(cad, tmp_path):
    prj = ('PROJCS["custom TM",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],'
           'PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433],AUTHORITY["EPSG","4326"]],'
           'PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],'
           'PARAMETER["Central_Meridian",25.0],PARAMETER["Scale_Factor",1.0],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]')
    (tmp_path / "plots.shp").write_bytes(shapefile_bytes(500000.0, 6670000.0))
    (tmp_path / "plots.prj").write_text(prj)
    r = cad.result("import.file", {"path": str(tmp_path / "plots.shp")})
    assert r["imported"] == 1
    assert r["georeference"]["crs"].startswith("PROJCS")
    assert r["georeference"]["origin"] == [500005.0, 6670005.0]


def test_geojson_holes_stay_holes_on_export(cad, tmp_path):
    path = tmp_path / "plot.geojson"
    path.write_text(json.dumps({"type": "Feature", "properties": {"name": "Plot"}, "geometry": {"type": "Polygon", "coordinates": [
        [[24.93, 60.16], [24.94, 60.16], [24.94, 60.17], [24.93, 60.17], [24.93, 60.16]],
        [[24.932, 60.162], [24.932, 60.166], [24.936, 60.166], [24.936, 60.162], [24.932, 60.162]]]}}))
    assert cad.result("import.file", {"path": str(path)})["imported"] == 2
    holes = [o for o in objects(cad, kind="polyline") if o["data"].get("_ring") == "hole"]
    assert len(holes) == 1 and holes[0]["data"]["_hole_of"] in [o["id"] for o in objects(cad)]
    out = tmp_path / "out.geojson"
    r = cad.result("export.file", {"path": str(out)})
    (feature,) = json.loads(out.read_text())["features"]
    assert feature["geometry"]["type"] == "Polygon" and len(feature["geometry"]["coordinates"]) == 2
    assert feature["properties"]["name"] == "Plot" and "_hole_of" not in feature["properties"]


def test_line_over_the_antimeridian_picks_the_zone_of_its_middle(cad, tmp_path):
    path = tmp_path / "line.geojson"
    path.write_text(json.dumps({"type": "LineString", "coordinates": [[179.9, 10.0], [-179.9, 10.0]]}))
    r = cad.result("import.file", {"path": str(path)})
    assert r["georeference"]["crs"] in ("EPSG:32660", "EPSG:32601")
    assert r["imported"] == 1


def test_code_pages_are_decoded(cad, tmp_path):
    # "\u0160\u00f5\u00f5" in CP1257.
    (tmp_path / "plots.shp").write_bytes(shapefile_bytes())
    (tmp_path / "plots.dbf").write_bytes(dbf_bytes("\u0160\u00f5\u00f5".encode("cp1257")))
    (tmp_path / "plots.cpg").write_text("1257")
    cad.result("import.file", {"path": str(tmp_path / "plots.shp")})
    (poly,) = objects(cad, kind="polyline")
    assert poly["data"]["NAME"] == "\u0160\u00f5\u00f5"
    path = tmp_path / "old.dxf"
    path.write_bytes(dxf_text("0\nTEXT\n8\nA\n10\n0\n20\n0\n40\n100\n1\n@@@\n", header="9\n$DWGCODEPAGE\n3\nANSI_1257\n")
                     .replace("@@@", "\u0160\u00f5\u00f5").encode("cp1257"))
    other = JsonRpc(["serve", "--allow-dir", str(tmp_path)])
    try:
        other.result("import.file", {"path": str(path)})
        (text,) = objects(other, kind="text")
        assert text["shape"]["text"] == "\u0160\u00f5\u00f5"
    finally:
        other.close()


def test_nested_array_inserts_fail_with_a_clear_error_and_no_memory_blowup(cad, tmp_path):
    blocks = ("0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n"
              "0\nBLOCK\n2\nA\n10\n0\n20\n0\n0\nINSERT\n2\nB\n70\n100\n71\n100\n44\n2\n45\n2\n0\nENDBLK\n")
    path = tmp_path / "bomb.dxf"
    path.write_text(dxf_text("0\nINSERT\n2\nA\n70\n100\n71\n100\n44\n300\n45\n300\n", blocks=blocks))
    assert path.stat().st_size < 600
    err = cad.error("import.file", {"path": str(path)})
    assert "max_exploded" in err["message"]
    assert cad.result("document.summary")["object_count"] == 0
    # The limit can be raised, and blocks can stay unexploded.
    r = cad.result("import.file", {"path": str(path), "options": {"explode_blocks": False}})
    assert r["imported"] == 0 and r["skipped"] == 1


def test_text_rotation_survives_a_dxf_round_trip(cad, make_serve, tmp_path):
    path = tmp_path / "t.dxf"
    path.write_text(dxf_text("0\nTEXT\n8\nA\n10\n0\n20\n0\n40\n100\n50\n30\n1\nHello\n"))
    cad.result("import.file", {"path": str(path)})
    (text,) = objects(cad, kind="text")
    assert text["data"]["rotation_deg"] == approx(30)
    out = tmp_path / "out.dxf"
    cad.result("export.file", {"path": str(out)})
    other = make_serve()
    other.result("import.file", {"path": str(out)})
    (back,) = objects(other, kind="text")
    assert back["data"]["rotation_deg"] == approx(30)


def test_agents_can_only_use_allowed_folders(tmp_path):
    inside, outside = tmp_path / "inside", tmp_path / "outside"
    inside.mkdir()
    outside.mkdir()
    (outside / "secret.dxf").write_text(dxf_text("0\nLINE\n8\nA\n10\n0\n20\n0\n11\n1\n21\n1\n"))
    (inside / "ok.dxf").write_text(dxf_text("0\nLINE\n8\nA\n10\n0\n20\n0\n11\n1\n21\n1\n"))
    srv = JsonRpc(["serve", "--allow-dir", str(inside)])
    try:
        assert srv.result("import.file", {"path": str(inside / "ok.dxf")})["imported"] == 1
        assert "--allow-dir" in srv.error("import.file", {"path": str(outside / "secret.dxf")})["message"]
        assert srv.error("import.file", {"path": str(inside / ".." / "outside" / "secret.dxf")})
        assert srv.error("export.file", {"path": str(outside / "x.dxf")})
        assert not (outside / "x.dxf").exists()
        # Extensions must match the format, and an existing file needs overwrite.
        assert "extension" in srv.error("export.file", {"path": str(inside / "x.txt"), "format": "dxf"})["message"]
        srv.result("export.file", {"path": str(inside / "out.dxf")})
        assert "overwrite" in srv.error("export.file", {"path": str(inside / "out.dxf")})["message"]
        srv.result("export.file", {"path": str(inside / "out.dxf"), "overwrite": True})
        assert (inside / "out.dxf.bak").exists()
        # A link that leaves the folder is outside too.
        (inside / "link.dxf").symlink_to(outside / "secret.dxf")
        assert srv.error("import.file", {"path": str(inside / "link.dxf")})
        # file.save and file.open follow the same folders.
        srv.result("file.save", {"path": str(inside / "doc.json")})
        assert srv.error("file.save", {"path": str(outside / "doc.json")})
        assert srv.error("file.open", {"path": str(outside / "doc.json")})
        assert "extension" in srv.error("file.save", {"path": str(inside / "doc.txt")})["message"]
        srv.result("file.open", {"path": str(inside / "doc.json")})
    finally:
        srv.close()
    # The folder of the document is allowed too.
    (outside / "doc.json").write_text(json.dumps(REPO and json.loads(open(inside / "doc.json").read())))
    other = JsonRpc(["serve", str(outside / "doc.json"), "--allow-dir", str(inside)])
    try:
        assert other.result("import.file", {"path": str(outside / "secret.dxf")})["imported"] == 1
    finally:
        other.close()


def test_exported_dxf_passes_the_ezdxf_audit(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    cad.execute({"op": "add_class", "name": "Walls / 1"})
    cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": [
        [0, 0], {"p": [1500, -800], "kind": "point_on_arc"}, [3000, 0], [3000, 2000], [0, 2000]]}, "attrs": {"line_weight": 0.37}})
    cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [500, 500], "radius": 250}})
    cad.execute({"op": "add_object", "shape": {"kind": "plant", "at": [4000, 1000], "species": "Acer", "spread": 3000, "height": 6000}})
    cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [10, 20, 30]}})
    cad.execute({"op": "add_object", "shape": {"kind": "text", "at": [0, 0], "text": "Hello \u00e4", "height": 200}})
    cad.execute({"op": "add_object", "shape": {"kind": "polyline3d", "closed": False, "points": [[0, 0, 0], [1, 1, 1]]}})
    out = tmp_path / "out.dxf"
    cad.result("export.file", {"path": str(out)})
    doc = ezdxf.readfile(str(out))
    auditor = doc.audit()
    assert not auditor.errors, [e.message for e in auditor.errors]
    assert not auditor.fixes, [e.message for e in auditor.fixes]
    assert sorted(e.dxftype() for e in doc.modelspace()) == sorted(["LWPOLYLINE", "CIRCLE", "CIRCLE", "INSERT", "POINT", "TEXT", "POLYLINE"])
    from ezdxf import recover
    _, rec = recover.readfile(str(out))
    assert not rec.errors and not rec.fixes
    assert {"Walls _ 1", "0"} <= {layer.dxf.name for layer in doc.layers}


def build_site_and_plants(cad):
    ids = []
    for i in range(9):
        for j in range(9):
            ids.append(cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}})[0])
    site = cad.result("site.create", {"source_ids": ids, "settings": {"minor_interval": 200, "major_interval": 400}})["id"]
    style = cad.result("plant_style.create", {"name": "Acer platanoides", "botanical_name": "Acer platanoides", "id_code": "AP", "spread": 4000, "height": 8000, "price": 55})
    style_id = style.get("id") or style.get("created", [None])[0]
    cad.result("plants.place", {"mode": "single", "style": style_id, "at": [2000, 2000]})
    cad.execute({"op": "add_object", "shape": {"kind": "existing_tree", "at": [6000, 6000], "species": "Quercus robur", "dbh": 500, "spread": 9000, "height": 15000}})
    return site


def test_site_contours_plants_and_trees_are_exported(cad, tmp_path):
    build_site_and_plants(cad)
    dxf = tmp_path / "site.dxf"
    r = cad.result("export.file", {"path": str(dxf)})
    assert r["report"]["skipped"] == []
    text = dxf.read_text()
    # Contours at 200, 400 and 600 mm are 3D polylines at their height.
    assert text.count("AcDb3dPolyline\n") >= 3
    assert "PLANT_Acer_platanoides" in text and "PLANT_Quercus_robur" in text
    geo = tmp_path / "site.geojson"
    cad.result("export.file", {"path": str(geo), "options": {"origin": "document"}})
    feats = json.loads(geo.read_text())["features"]
    contours = [f for f in feats if f["properties"].get("kind") == "contour"]
    assert len(contours) >= 3 and all(f["geometry"]["type"] == "LineString" for f in contours)
    assert {round(f["properties"]["level_m"], 3) for f in contours} >= {0.2, 0.4, 0.6}
    assert {c[2] for f in contours for c in f["geometry"]["coordinates"]} <= {0.2, 0.4, 0.6, 0.8}
    plant = next(f for f in feats if f["properties"].get("kind") == "plant")
    assert plant["properties"]["id_code"] == "AP" and plant["properties"]["plant_style"] == "Acer platanoides"
    tree = next(f for f in feats if f["properties"].get("kind") == "existing_tree")
    assert tree["properties"]["dbh_mm"] == 500 and tree["geometry"]["type"] == "Point"
    ezdxf = pytest.importorskip("ezdxf", reason="run with --with ezdxf")
    assert not ezdxf.readfile(str(dxf)).audit().errors
