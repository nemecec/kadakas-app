"""Scenario: a landscape architect puts a map background and building footprints into a drawing, turns the
footprints into massing models, reads the quantities, and changes the CRS of the document.

The map server is a Python `http.server` on 127.0.0.1 that serves a test PNG. No test uses the real network."""

import json
import math
import struct
import threading
import zlib
from http.server import BaseHTTPRequestHandler, HTTPServer

import pytest
from pytest import approx


def png_bytes(width, height, rgba):
    """A solid-color PNG made with the standard library."""
    row = b"\x00" + bytes(rgba) * width
    raw = row * height

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


BUILDINGS = {
    "type": "FeatureCollection",
    "features": [
        {"type": "Feature", "properties": {"name": "Tall", "height": 12.5},
         "geometry": {"type": "Polygon", "coordinates": [[[385010, 6672010], [385030, 6672010], [385030, 6672020], [385010, 6672020], [385010, 6672010]]]}},
        {"type": "Feature", "properties": {"name": "Low", "levels": 2},
         "geometry": {"type": "Polygon", "coordinates": [[[385060, 6672010], [385070, 6672010], [385070, 6672020], [385060, 6672020], [385060, 6672010]]]}},
    ],
}


@pytest.fixture
def map_server():
    """A WMS and WFS on a free local port. Yields (base_url, list of request paths)."""
    hits = []
    holder = {}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, status, ctype, body):
            self.send_response(status)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            hits.append(self.path)
            base = holder["base"]
            if "REQUEST=GetCapabilities" in self.path and self.path.startswith("/wms"):
                xml = f"""<WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink">
<Service><Title>Test WMS</Title></Service><Capability><Request><GetMap><Format>image/png</Format>
<DCPType><HTTP><Get><OnlineResource xlink:href="{base}/wms?"/></Get></HTTP></DCPType></GetMap></Request>
<Layer><Title>root</Title><CRS>EPSG:3857</CRS><CRS>EPSG:3067</CRS><Layer><Name>ortho</Name><Title>Ortho</Title></Layer></Layer></Capability></WMS_Capabilities>"""
                self.reply(200, "text/xml", xml.encode())
            elif "REQUEST=GetMap" in self.path:
                self.reply(200, "image/png", png_bytes(512, 512, (60, 140, 200, 255)))
            elif "REQUEST=GetFeature" in self.path:
                self.reply(200, "application/json", json.dumps(BUILDINGS).encode())
            else:
                self.reply(404, "text/plain", b"not found")

    server = HTTPServer(("127.0.0.1", 0), Handler)
    holder["base"] = f"http://127.0.0.1:{server.server_port}"
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield holder["base"], hits
    server.shutdown()
    server.server_close()


def set_crs(cad):
    return cad.result("gis.georef.set", {"crs": "EPSG:3067", "origin": [385000, 6672000]})


def test_agents_may_not_use_the_network_unless_the_owner_allows_it(cad, map_server):
    base, hits = map_server
    set_crs(cad)
    error = cad.error("gis.wms.capabilities", {"url": f"{base}/wms"})
    assert "--allow-url" in error["message"]
    assert hits == [], "no request left the process"
    info = cad.result("gis.sources")
    assert info["restricted"] is True and info["allowed_urls"] == []


def test_wms_map_becomes_an_underlay_below_the_vector_objects(make_serve, map_server, tmp_path):
    base, hits = map_server
    cad = make_serve("--allow-url", "127.0.0.1")
    set_crs(cad)
    caps = cad.result("gis.wms.capabilities", {"url": f"{base}/wms"})
    assert caps["title"] == "Test WMS"
    assert any(layer["name"] == "ortho" for layer in caps["layers"])
    cad.execute({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [20000, 10000]}})
    result = cad.result("gis.wms.add", {"url": f"{base}/wms", "layers": ["ortho"], "bbox": [-5000, -5000, 30000, 20000], "max_px": 300, "name": "Aerial"})
    assert (result["width"], result["height"]) == (300, 214)
    assert result["stats"]["crs"] == "EPSG:3067" or result["stats"]["crs"] == "EPSG:3857"
    assert any("GetMap" in h for h in hits)
    # The list does not carry the picture.
    listed = cad.result("objects.list", {"kind": "underlay"})
    assert len(listed) == 1 and "not shown" in listed[0]["shape"]["png"]
    assert listed[0]["data"]["gis_source"]["kind"] == "wms"
    # The SVG draws the picture first, so the rectangle is on top.
    svg = cad.result("render.svg", {"width": 600})
    svg = svg if isinstance(svg, str) else svg["svg"]
    assert svg.index("<image ") < svg.index("<rect ")
    assert "data:image/png;base64," in svg
    # The second request for the same view comes from the disk cache and asks only for the capabilities.
    before = len([h for h in hits if "GetMap" in h])
    again = cad.result("gis.wms.refresh", {"id": result["id"]})
    assert again["stats"]["from_cache"] == again["stats"]["tiles"]
    assert len([h for h in hits if "GetMap" in h]) == before
    # One undo step removes the map.
    assert cad.result("undo")["done"]
    assert cad.result("objects.list", {"kind": "underlay"}) == []


def test_wms_errors_are_clear(make_serve, map_server):
    base, _ = map_server
    cad = make_serve("--allow-url", "127.0.0.1")
    assert "gis.georef.set" in cad.error("gis.wms.add", {"url": f"{base}/wms", "layers": ["ortho"], "bbox": [0, 0, 1000, 1000]})["message"]
    set_crs(cad)
    assert "no layer named nope" in cad.error("gis.wms.add", {"url": f"{base}/wms", "layers": ["nope"], "bbox": [0, 0, 1000, 1000]})["message"]
    assert "bbox" in cad.error("gis.wms.add", {"url": f"{base}/wms", "layers": ["ortho"]})["message"]
    assert "404" in cad.error("gis.wms.capabilities", {"url": f"{base}/missing"})["message"]
    # An address that is not on the list is refused even when another one is.
    assert "not on the list" in cad.error("gis.wms.capabilities", {"url": "https://example.org/wms"})["message"]
    assert cad.error("gis.wms.capabilities", {"url": "file:///etc/passwd"})["code"] == -32602


def test_wfs_buildings_become_massing_models_with_quantities(make_serve, map_server):
    base, _ = map_server
    cad = make_serve("--allow-url", "127.0.0.1")
    set_crs(cad)
    imported = cad.result("gis.wfs.import", {"url": f"{base}/wfs", "type_name": "ns:buildings", "caps": False, "format": "geojson",
                                             "options": {"layer": "Buildings"}})
    assert imported["imported"] == 2
    footprints = cad.result("objects.list", {"kind": "polyline"})
    by_name = {o["data"]["name"]: o for o in footprints}
    assert by_name["Tall"]["data"]["height"] == 12.5
    # Convert both in one step. Heights come from the data: a height in meters, and a number of levels.
    r = cad.result("massing.from_shapes", {"ids": [o["id"] for o in footprints], "roof": "gable", "pitch": 35, "name_field": "name"})
    assert [c["how"] for c in r["created"]] == ["height from height", "floors from levels"]
    assert cad.result("objects.list", {"kind": "polyline"}) == []
    report = cad.result("massing.report")
    rows = {row["name"]: row for row in report["rows"]}
    tall = rows["Tall"]
    assert tall["footprint_m2"] == approx(200.0, abs=0.01)
    assert tall["eave_height_m"] == approx(12.5)
    rise = 5.0 * math.tan(math.radians(35))
    # Gable volume: the box plus half the area times the rise of the ridge (20 m by 10 m).
    assert tall["volume_m3"] == approx(200.0 * 12.5 + 200.0 * rise / 2, rel=1e-6)
    assert tall["ridge_height_m"] == approx(12.5 + rise)
    low = rows["Low"]
    assert low["floors"] == 2 and low["gross_floor_area_m2"] == approx(2 * 100.0, abs=0.01)
    assert report["totals"]["count"] == 2
    # A height of 12.5 m counts as 4 floors of 3 m (rounded), so the gross floor area is 4 * 200 + 2 * 100.
    assert tall["floors"] == 4
    assert report["totals"]["gross_floor_area_m2"] == approx(800.0 + 200.0, abs=0.05)
    assert cad.result("massing.report", {"format": "csv"})["csv"].startswith("id,name,layer,floors,roof,")
    # One undo brings the footprints back, with their data.
    assert cad.result("undo")["done"]
    assert len(cad.result("objects.list", {"kind": "polyline"})) == 2


def test_massing_roofs_and_volumes(cad):
    footprint = [[0, 0], [12000, 0], [12000, 8000], [0, 8000]]
    t = math.tan(math.radians(30))
    rise = 4.0 * t
    flat = cad.result("massing.add", {"footprint": footprint, "height": 6000})
    assert flat["quantities"]["volume_m3"] == approx(96 * 6.0)
    gable = cad.result("massing.add", {"footprint": footprint, "height": 6000, "roof": "gable"})
    assert gable["quantities"]["volume_m3"] == approx(96 * 6.0 + 96 * rise / 2, rel=1e-9)
    hip = cad.result("massing.add", {"footprint": footprint, "height": 6000, "roof": "hip"})
    assert hip["quantities"]["volume_m3"] == approx(96 * 6.0 + 8 * rise * (3 * 12 - 8) / 6, rel=1e-9)
    assert hip["quantities"]["ridge_height_m"] == approx(6.0 + rise)
    # Floors times a floor height; a curved corner is allowed.
    curved = cad.result("massing.add", {"footprint": [[0, 0], [10000, 0], {"p": [10000, 10000], "kind": "arc", "radius": 3000}, [0, 10000]],
                                         "floors": 3, "floor_height": 2800})
    got = cad.result("massing.get", {"id": curved["id"]})
    assert got["floors"] == 3 and got["quantities"]["eave_height_m"] == approx(8.4)
    assert got["quantities"]["footprint_m2"] < 100.0
    # Change the roof and the base.
    cad.result("massing.set", {"id": curved["id"], "roof": "hip", "pitch": 40, "base": 500, "floors": None, "height": 7000})
    got = cad.result("massing.get", {"id": curved["id"]})
    assert got["roof"] == "hip" and got.get("floors") is None and got["base_elevation_mm"] == 500
    # Wrong values are refused with a message.
    assert "pitch" in cad.error("massing.add", {"footprint": footprint, "pitch": 95})["message"]
    assert "flat, gable or hip" in cad.error("massing.add", {"footprint": footprint, "roof": "dome"})["message"]
    assert "footprint" in cad.error("massing.add", {"height": 3000})["message"]
    # It is in the document summary, in the 3D scene and in the render.
    assert cad.result("document.summary")["objects_by_kind"]["massing"] == 4
    assert "data-kind=\"massing\"" in (lambda s: s if isinstance(s, str) else s["svg"])(cad.result("render.svg"))


def test_massing_stands_on_the_site_model(cad):
    ids = []
    for i in range(9):
        for j in range(9):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, 200 + i * 100]}})
    cad.result("site.create", {"source_ids": ids})
    m = cad.result("massing.add", {"footprint": [[2000, 2000], [5000, 2000], [5000, 4000], [2000, 4000]], "height": 3000})
    got = cad.result("massing.get", {"id": m["id"]})
    # The lowest ground under the building: x = 2000 mm, 200 + 200 = 400 mm.
    assert got["base_elevation_mm"] == approx(400)
    rows = cad.result("massing.report")["rows"]
    assert rows[0]["base_elevation_m"] == approx(0.4)


def test_reproject_keeps_places_and_reports(cad):
    set_crs(cad)
    cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": [[0, 0], [20000, 0], [20000, 10000], [0, 10000]]}})
    cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [5000, 5000, 42.5]}})
    probe = cad.result("gis.coords", {"x": 20000, "y": 10000})
    assert probe["easting"] == approx(385020) and probe["northing"] == approx(6672010)
    dry = cad.result("gis.reproject", {"crs": "EPSG:32635", "dry_run": True})
    assert dry["applied"] is False
    report = dry["report"]
    assert report["objects_changed"] == 2 and report["from_crs"] == "EPSG:3067" and report["to_crs"] == "EPSG:32635"
    assert 0.999 < report["scale_factor"] < 1.001
    assert any("heights" in w for w in report["warnings"])
    assert cad.result("document.get")["georef"]["crs"] == "EPSG:3067"
    done = cad.result("gis.reproject", {"crs": "EPSG:32635"})
    assert done["applied"] is True
    assert cad.result("document.get")["georef"]["crs"] == "EPSG:32635"
    # The corner is at the same longitude and latitude, and the height did not change.
    after = cad.result("gis.coords", {"x": 20000 * report["scale_factor"], "y": 10000 * report["scale_factor"]})
    assert after["lat"] == approx(probe["lat"], abs=2e-5) and after["lon"] == approx(probe["lon"], abs=2e-5)
    locus = cad.result("objects.list", {"kind": "locus3d"})[0]
    assert locus["shape"]["p"][2] == 42.5
    # Converting back gives the first numbers again, within a few millimeters.
    back = cad.result("gis.reproject", {"crs": "EPSG:3067", "origin": [385000, 6672000]})
    assert back["applied"]
    poly = cad.result("objects.list", {"kind": "polyline"})[0]["shape"]["points"]
    assert poly[2][0] == approx(20000, abs=10) and poly[2][1] == approx(10000, abs=10)
    # One undo step per conversion.
    assert cad.result("undo")["done"] and cad.result("document.get")["georef"]["crs"] == "EPSG:32635"
    # Without a CRS, or to longitude and latitude, the call is refused.
    assert "projected" in cad.error("gis.reproject", {"crs": "EPSG:4326"})["message"]


def test_reproject_needs_a_crs(cad):
    assert "no CRS" in cad.error("gis.reproject", {"crs": "EPSG:32635"})["message"]
    assert "no CRS" in cad.error("gis.coords", {"x": 0, "y": 0})["message"]


def test_geoimage_with_a_world_file(cad, tmp_path):
    (tmp_path / "plan.png").write_bytes(png_bytes(10, 10, (200, 30, 30, 255)))
    (tmp_path / "plan.pgw").write_text("2\n0\n0\n-2\n385001\n6672009\n")
    r = cad.result("gis.geoimage.import", {"path": str(tmp_path / "plan.png"), "crs": "EPSG:3067", "name": "Plan"})
    assert r["georeference"]["crs"] == "EPSG:3067"
    under = cad.result("objects.get", {"id": r["id"]})["shape"]
    assert under["kind"] == "underlay" and under["name"] == "Plan"
    # The document had no georeference, so it gets one from the picture, with the origin in the middle.
    assert under["min"][0] == approx(-10000, abs=1e-3) and under["max"][0] == approx(10000, abs=1e-3)
    assert r["georeference"]["origin"] == [385010.0, 6672000.0]
    (tmp_path / "lonely.png").write_bytes(png_bytes(4, 4, (1, 2, 3, 255)))
    assert "no world file" in cad.error("gis.geoimage.import", {"path": str(tmp_path / "lonely.png")})["message"]


def test_describe_lists_the_new_methods(rpc):
    d = rpc("describe")["result"]
    names = {m["name"] for m in d["methods"]}
    assert {"gis.wms.add", "gis.wfs.import", "gis.reproject", "massing.from_shapes", "massing.report"} <= names
    assert "--allow-url" in d["gis"]["network_policy"]
    assert d["massing"]["examples"] and d["gis"]["examples"]
