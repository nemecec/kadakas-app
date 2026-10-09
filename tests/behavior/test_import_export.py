"""Scenario: import DXF, GeoJSON and Shapefile files through the API, and export again."""

import json
import math
import struct

from pytest import approx

from helpers import REPO

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


def shapefile_bytes():
    ring = [(0, 0), (0, 10), (10, 10), (10, 0), (0, 0)]  # clockwise: outer ring
    body = struct.pack("<i4d2i", 5, 0, 0, 10, 10, 1, len(ring)) + struct.pack("<i", 0)
    body += b"".join(struct.pack("<2d", x, y) for x, y in ring)
    header = bytearray(100)
    header[0:4] = struct.pack(">i", 9994)
    header[28:32] = struct.pack("<i", 1000)
    header[32:36] = struct.pack("<i", 5)
    return bytes(header) + struct.pack(">2i", 1, len(body) // 2) + body


def dbf_bytes():
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
    out += b" " + b"Lawn".ljust(8) + b"42".rjust(6)
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
