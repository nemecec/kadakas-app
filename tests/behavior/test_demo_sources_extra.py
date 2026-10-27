"""Tests of examples/demo-address/sources_extra.py.

No network and no real data: every response here is invented, in the shape the real services use.
The live check of the real services is a manual step (see docs/research/estonian-open-data.md).
"""

import json
import math
import struct
import sys
import urllib.error
import zlib
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "examples" / "demo-address"))
import sources_extra as se  # noqa: E402


# ---------------------------------------------------------------- synthetic GeoTIFF

def make_tiff(width, height, tile, ox, oy, func, predictor=3, nodata=-9999.0):
    """A tiled, deflate compressed, float32 GeoTIFF like the Land Board 1 m DTM sheets."""
    tw = th = tile
    across, down = -(-width // tw), -(-height // th)
    blobs = []
    for tr in range(down):
        for tc in range(across):
            raw = bytearray()
            for r in range(th):
                vals = [func(tc * tw + c, tr * th + r) for c in range(tw)]
                if predictor == 3:
                    be = b"".join(struct.pack(">f", v) for v in vals)
                    planes = be[0::4] + be[1::4] + be[2::4] + be[3::4]
                    diff = bytes((planes[i] - (planes[i - 1] if i else 0)) & 255 for i in range(len(planes)))
                    raw += diff
                else:
                    raw += struct.pack("<%df" % tw, *vals)
            blobs.append(zlib.compress(bytes(raw)))
    entries = []  # (tag, type, count, payload bytes)
    n_tiles = len(blobs)
    ifd_size = 2 + 12 * 15 + 4
    extra_at = 8 + ifd_size
    extra = bytearray()

    def put(data):
        nonlocal extra
        off = extra_at + len(extra)
        extra += data
        return off

    data_at = None  # patched below
    scale = struct.pack("<3d", 1.0, 1.0, 0.0)
    tie = struct.pack("<6d", 0, 0, 0, ox, oy, 0)
    geokeys = struct.pack("<8H", 1, 1, 0, 1, 1025, 0, 1, 1)
    nod = (b"%d\0" % int(nodata))
    off_scale, off_tie, off_nod = put(scale), put(tie), put(nod)
    off_keys = put(geokeys)
    off_counts_pos = put(b"\0" * (4 * n_tiles))
    off_offsets_pos = put(b"\0" * (4 * n_tiles))
    data_at = extra_at + len(extra)
    offsets, pos = [], data_at
    for b in blobs:
        offsets.append(pos)
        pos += len(b)
    extra[off_counts_pos - extra_at:off_counts_pos - extra_at + 4 * n_tiles] = struct.pack(
        "<%dI" % n_tiles, *[len(b) for b in blobs])
    extra[off_offsets_pos - extra_at:off_offsets_pos - extra_at + 4 * n_tiles] = struct.pack(
        "<%dI" % n_tiles, *offsets)

    def short(tag, v):
        return (tag, 3, 1, struct.pack("<H", v) + b"\0\0")

    def long_(tag, v):
        return (tag, 4, 1, struct.pack("<I", v))

    def ref(tag, typ, cnt, off):
        return (tag, typ, cnt, struct.pack("<I", off))

    entries = [
        short(256, width), short(257, height), short(258, 32), short(259, 8), short(277, 1),
        short(317, predictor), short(322, tw), short(323, th),
        ref(324, 4, n_tiles, off_offsets_pos), ref(325, 4, n_tiles, off_counts_pos),
        short(339, 3), ref(33550, 12, 3, off_scale), ref(33922, 12, 6, off_tie),
        ref(34735, 3, 8, off_keys), ref(42113, 2, len(nod), off_nod),
    ]
    entries.sort()
    ifd = struct.pack("<H", len(entries))
    for tag, typ, cnt, payload in entries:
        ifd += struct.pack("<HHI", tag, typ, cnt) + payload.ljust(4, b"\0")
    ifd += struct.pack("<I", 0)
    head = b"II*\x00" + struct.pack("<I", 8) + ifd
    assert len(head) == extra_at
    return head + bytes(extra) + b"".join(blobs)


class FakeNet:
    """Replaces se._http. Routes by a substring of the URL; honours Range headers."""

    def __init__(self):
        self.routes = []
        self.calls = []

    def add(self, needle, body):
        self.routes.append((needle, body))

    def __call__(self, url, *, cache_dir=None, offline=False, headers=None, data=None, **kw):
        self.calls.append((url, headers, data))
        for needle, body in self.routes:
            if needle in url:
                if callable(body):
                    body = body(url, data)
                if isinstance(body, Exception):
                    raise body
                if isinstance(body, (dict, list)):
                    body = json.dumps(body).encode()
                if headers and "Range" in headers:
                    a, b = headers["Range"].split("=")[1].split("-")
                    return body[int(a):int(b) + 1]
                return body
        raise se.SourceError("HTTP 404 from test (no route for %s)" % url[:60])


@pytest.fixture
def net(monkeypatch):
    fake = FakeNet()
    monkeypatch.setattr(se, "_http", fake)
    return fake


def fc(*features):
    return {"type": "FeatureCollection", "features": list(features)}


def feat(geometry, **props):
    return {"type": "Feature", "geometry": geometry, "properties": props}


def square(x, y, s):
    return {"type": "Polygon", "coordinates": [[[x, y], [x + s, y], [x + s, y + s], [x, y + s], [x, y]]]}


# ---------------------------------------------------------------- projection

def test_lest97_origin_matches_the_epsg_3301_constants():
    # EPSG:3301 false easting 500000, false northing 6375000 at latitude 57.517553930555556, lon 24.
    x, y = se.wgs84_to_lest97(57.517553930555556, 24.0)
    assert (x, y) == (pytest.approx(500000.0, abs=1e-6), pytest.approx(6375000.0, abs=1e-6))


def test_lest97_round_trip_is_exact_to_a_millimetre():
    for x, y in [(612345.5, 6498765.25), (480000.0, 6550000.0), (400000, 6500000)]:
        lat, lon = se.lest97_to_wgs84(x, y)
        x2, y2 = se.wgs84_to_lest97(lat, lon)
        assert math.hypot(x - x2, y - y2) < 0.001


def test_central_meridian_is_on_x_500000():
    assert se.wgs84_to_lest97(58.5, 24.0)[0] == pytest.approx(500000.0, abs=1e-6)


# ---------------------------------------------------------------- map sheets

@pytest.mark.parametrize("x,y,sheet", [
    (670000.5, 6460000.5, "54671"), (675000.5, 6460000.5, "54672"),
    (670000.5, 6465000.5, "54673"), (679999.0, 6469999.0, "54674"),
    (680000.5, 6460000.5, "54681"), (570000.5, 6460000.5, "53671"),
    (670000.5, 6560000.5, "64671"),
])
def test_dtm_sheet_numbers_follow_the_land_board_scheme(x, y, sheet):
    assert se.dtm_sheet(x, y) == sheet


# ---------------------------------------------------------------- terrain

def plane(c, r):
    return 100.0 + 0.5 * c - 0.25 * r


def test_remote_tiff_reads_the_header_and_float_predictor_tiles(net):
    net.add("dtm", make_tiff(48, 48, 16, 1000.0, 2048.0, plane))
    t = se._RemoteTiff("https://example.test/dtm")
    assert (t.w, t.h, t.tw, t.th) == (48, 48, 16, 16)
    assert (t.ox, t.oy, t.ps) == (1000.0, 2048.0, 1.0)
    assert t.pixel(20, 33) == pytest.approx(plane(20, 33))


def test_remote_tiff_fetches_only_the_needed_tile(net):
    net.add("dtm", make_tiff(64, 64, 16, 0.0, 64.0, plane))
    t = se._RemoteTiff("https://example.test/dtm")
    net.calls.clear()
    t.pixel(40, 40)
    assert len(net.calls) == 1, "one range request for one tile"
    t.pixel(41, 41)
    assert len(net.calls) == 1, "a tile is decoded once"


def test_remote_tiff_without_predictor(net):
    net.add("dtm", make_tiff(32, 32, 16, 0.0, 32.0, plane, predictor=1))
    t = se._RemoteTiff("https://example.test/dtm")
    assert t.pixel(5, 6) == pytest.approx(plane(5, 6))


def test_bilinear_sample_is_exact_on_a_plane(net):
    net.add("dtm", make_tiff(64, 64, 16, 0.0, 64.0, plane))
    t = se._RemoteTiff("https://example.test/dtm")
    # Pixel (c, r) has its centre at x = c + 0.5, y = 64 - r - 0.5.
    x, y = 20.3, 40.8
    c, r = x - 0.5, 64 - y - 0.5
    assert t.sample(x, y) == pytest.approx(plane(c, r), abs=1e-4)


def test_nodata_is_none(net):
    net.add("dtm", make_tiff(32, 32, 16, 0.0, 32.0, lambda c, r: -9999.0 if c < 10 else 5.0))
    t = se._RemoteTiff("https://example.test/dtm")
    assert t.pixel(3, 3) is None
    assert t.pixel(20, 3) == 5.0


def test_terrain_grid_shape_and_values(net):
    net.add("geoportaal", make_tiff(64, 64, 16, 1000.0, 1064.0, plane))
    g = se.terrain_grid(1032.0, 1032.0, half_size=10, step=2)
    assert g["nx"] == g["ny"] == 11
    assert g["origin"] == (1022.0, 1022.0)
    assert g["step"] == 2.0
    assert len(g["z"]) == 11 and all(len(row) == 11 for row in g["z"])
    # z[j][i] is the height at (x0 + i*step, y0 + j*step): east and north raise it.
    assert g["z"][0][10] > g["z"][0][0]
    assert g["z"][10][0] > g["z"][0][0]
    cx = (1032.0 - 1000.0) - 0.5
    cy = (1064.0 - 1032.0) - 0.5
    assert g["z"][5][5] == pytest.approx(plane(cx, cy), abs=1e-2)
    assert g["nodata_count"] == 0
    assert se.terrain_grid.last_error is None


def test_terrain_grid_all_nodata_returns_none(net):
    net.add("geoportaal", make_tiff(32, 32, 16, 0.0, 32.0, lambda c, r: -9999.0))
    assert se.terrain_grid(16.0, 16.0, half_size=4, step=2) is None
    assert "no data" in se.terrain_grid.last_error


def test_terrain_grid_survives_a_missing_sheet(net):
    assert se.terrain_grid(612345.5, 6498765.25) is None
    assert se.terrain_grid.last_error.startswith("DTM sheet 54913")


def test_terrain_url_has_the_sheet_number(net):
    net.add("geoportaal", make_tiff(32, 32, 16, 612000.0, 6499000.0, plane))
    se.terrain_grid(612345.5, 6498765.25, half_size=2, step=2)
    assert "kaardiruut=54913" in net.calls[0][0]
    assert "54913_dtm_1m.tif" in net.calls[0][0]


def test_contour_lines_of_a_slope_are_straight_and_evenly_spaced():
    n = 11
    grid = {"origin": (0.0, 0.0), "step": 2.0, "nx": n, "ny": n,
            "z": [[0.1 * i * 2 for i in range(n)] for _ in range(n)]}  # rises 0.1 m per metre east
    lines = se.contour_lines(grid, interval=0.5)
    for level in (0.5, 1.0, 1.5):
        (line,) = lines[level]
        assert all(p[0] == pytest.approx(level / 0.1, abs=1e-6) for p in line)
        assert line[0][1] != line[-1][1], "the line runs north to south"
        assert len(line) == 11, "one point per grid row, joined into one line"


def test_contour_lines_skip_cells_without_data():
    grid = {"origin": (0.0, 0.0), "step": 1.0, "nx": 3, "ny": 3,
            "z": [[0.0, 1.0, 2.0], [0.0, None, 2.0], [0.0, 1.0, 2.0]]}
    assert isinstance(se.contour_lines(grid, 1.0), dict)


# ---------------------------------------------------------------- parcel

PARCEL = feat(square(100, 200, 30), tunnus="00000:000:0001", pindala=900, siht1="ELAMUMAA",
              siht2=None, siht3=None, l_aadress="Testi tn 1", omvorm="Eraomand", ov_nimi="Testi vald")


def test_plot_boundary_by_point(net):
    net.add("kataster", fc(PARCEL))
    r = se.plot_boundary(115.0, 215.0)
    assert r["cadastral_number"] == "00000:000:0001"
    assert r["area_m2"] == 900
    assert r["purpose"] == "ELAMUMAA"
    assert r["polygon"][0] == (100.0, 200.0)
    assert all(isinstance(p, tuple) for p in r["polygon"])
    # In the CQL filter the northing comes before the easting.
    url = net.calls[0][0]
    assert "POINT%28215.00+115.00%29" in url or "POINT(215.00%20115.00)" in url or "POINT(215.00+115.00)" in url


def test_plot_boundary_falls_back_to_the_ehr_record(net):
    ehr = {"ehitis": {"ehitiseKatastriyksused": {"ky": [{"katastritunnus": "00000:000:0001"}]}}}
    net.add("livekluster", ehr)

    def kataster(url, data):
        return fc() if "INTERSECTS" in urllib_unquote(url) else fc(PARCEL)

    net.add("kataster", kataster)
    r = se.plot_boundary(0.0, 0.0, ehr_code="123")
    assert r["cadastral_number"] == "00000:000:0001"
    assert any("tunnus" in urllib_unquote(c[0]) for c in net.calls)


def test_plot_boundary_none_when_no_parcel(net):
    net.add("kataster", fc())
    assert se.plot_boundary(1.0, 2.0) is None
    assert "no cadastral parcel" in se.plot_boundary.last_error


def test_plot_boundary_none_when_the_service_is_down(net):
    net.add("kataster", se.SourceError("HTTP 503 from test"))
    assert se.plot_boundary(1.0, 2.0) is None
    assert se.plot_boundary.last_error == "HTTP 503 from test"


def test_plot_boundary_takes_the_biggest_part_of_a_multipolygon(net):
    mp = {"type": "MultiPolygon", "coordinates": [square(0, 0, 2)["coordinates"], square(10, 10, 20)["coordinates"]]}
    net.add("kataster", fc(feat(mp, tunnus="1", pindala=1)))
    assert se.plot_boundary(1.0, 1.0)["polygon"][0] == (10.0, 10.0)


def urllib_unquote(s):
    import urllib.parse
    return urllib.parse.unquote_plus(s)


# ---------------------------------------------------------------- nearby features

def etak_answer(url, data):
    u = urllib_unquote(url)
    if "e_401_hoone_ka" in u:
        return fc(feat(square(10, 10, 8), ehr_gid="111", korgus_m=7.5, ads_lahiaadress="Testi tn 2",
                       tyyp_tekst="Elamu"),
                  feat(square(500, 500, 8), ehr_gid="222"))  # far away: filtered out by the radius
    if "e_501_tee_j" in u:
        return fc(feat({"type": "LineString", "coordinates": [[0, 30], [50, 30]]},
                       laius=6, teekate_tekst="Püsikate", nimetus="Testi tänav"))
    if "puittaimestik_p" in u:
        return fc(feat({"type": "Point", "coordinates": [20, 20]}, tyyp_tekst="Puu"))
    if "e_405_piire_j" in u:
        return fc(feat({"type": "LineString", "coordinates": [[0, 0], [0, 20]]}))
    return fc()


def test_nearby_features_etak(net):
    net.add("geoserver", etak_answer)
    r = se.nearby_features(15.0, 15.0, radius=100)
    assert r["source"] == "ETAK"
    (b,) = r["buildings"]
    assert b["height_m"] == 7.5 and b["ehr_code"] == "111" and b["address"] == "Testi tn 2"
    assert b["polygon"][0] == (10.0, 10.0) and b["distance_m"] == 0.0
    (road,) = r["roads"]
    assert road["width_m"] == 6 and road["line"][0] == (0.0, 30.0)
    assert road["distance_m"] == pytest.approx(15.0)
    assert r["trees"][0]["point"] == (20.0, 20.0)
    assert len(r["fences"]) == 1
    assert r["water"] == []
    assert r["errors"] == []


def test_nearby_features_keeps_partial_results_when_one_layer_fails(net):
    def partial(url, data):
        if "e_501_tee_j" in urllib_unquote(url):
            raise se.SourceError("HTTP 500 from test")
        return etak_answer(url, data)

    net.add("geoserver", partial)
    r = se.nearby_features(15.0, 15.0, radius=100, source="etak")
    assert len(r["buildings"]) == 1 and r["roads"] == []
    assert len(r["errors"]) == 1 and "e_501_tee_j" in r["errors"][0]


OVERPASS = {"elements": [
    {"type": "way", "id": 1, "tags": {"building": "house", "height": "6"},
     "geometry": [{"lat": 58.7000, "lon": 25.5000}, {"lat": 58.7000, "lon": 25.5001},
                  {"lat": 58.7001, "lon": 25.5001}, {"lat": 58.7001, "lon": 25.5000},
                  {"lat": 58.7000, "lon": 25.5000}]},
    {"type": "way", "id": 2, "tags": {"highway": "residential", "name": "Test"},
     "geometry": [{"lat": 58.6998, "lon": 25.4995}, {"lat": 58.6998, "lon": 25.5005}]},
    {"type": "node", "id": 3, "lat": 58.6999, "lon": 25.4999, "tags": {"natural": "tree", "species": "Tilia cordata"}},
    {"type": "way", "id": 4, "tags": {"barrier": "fence"},
     "geometry": [{"lat": 58.6996, "lon": 25.4995}, {"lat": 58.6996, "lon": 25.4999}]},
    {"type": "way", "id": 5, "tags": {"landuse": "residential"},
     "geometry": [{"lat": 58.6990, "lon": 25.4990}, {"lat": 58.6990, "lon": 25.5010},
                  {"lat": 58.7005, "lon": 25.5010}, {"lat": 58.6990, "lon": 25.4990}]},
]}


def test_nearby_features_falls_back_to_openstreetmap(net):
    net.add("gsavalik", se.SourceError("HTTP 503 from test"))
    net.add("overpass", OVERPASS)
    x, y = se.wgs84_to_lest97(58.6999, 25.4999)
    r = se.nearby_features(x, y, radius=150)
    assert r["source"] == "OpenStreetMap"
    (b,) = r["buildings"]
    assert b["height_m"] == 6.0
    assert len(b["polygon"]) == 4, "the closing point is dropped"
    assert all(5_000_000 < py < 7_000_000 and 100_000 < px < 900_000 for px, py in b["polygon"])
    assert r["roads"][0]["kind"] == "residential"
    assert r["trees"][0]["species"] == "Tilia cordata"
    assert r["trees"][0]["point"][0] == pytest.approx(x, abs=0.5)
    assert len(r["fences"]) == 1 and len(r["landuse"]) == 1


def test_nearby_features_etak_only_does_not_fall_back(net):
    net.add("gsavalik", se.SourceError("HTTP 503 from test"))
    net.add("overpass", OVERPASS)
    assert se.nearby_features(0.0, 0.0, source="etak") is None
    assert "ETAK" in se.nearby_features.last_error or "HTTP 503" in se.nearby_features.last_error


# ---------------------------------------------------------------- flags and soil

def flags_answer(url, data):
    u = urllib_unquote(url)
    if "kpo_malestised" in u:
        return fc(feat({"type": "Point", "coordinates": [30, 40]}, nimi="Testi ohvrikivi"))
    if "kpo_kaitsevoondid" in u:
        return fc(feat(square(-100, -100, 500), voond_nimi="Testi muinsuskaitseala"))
    if "kr_kaitseala" in u:
        return fc(feat(square(500, 0, 10), nimi="Kaugel park"))
    if "kr_yleujutusohuga_ala" in u:
        return fc(feat(square(-5, -5, 50)))
    return fc()


def test_environment_flags(net):
    net.add("geoserver", flags_answer)
    r = se.environment_flags(0.0, 0.0, radius=200)
    kinds = [f["kind"] for f in r]
    assert kinds[:2] == ["heritage_protection_zone", "flood_risk_area"] or "heritage_monument" in kinds
    assert "protected_area" not in kinds, "500 m away is outside the radius"
    monument = next(f for f in r if f["kind"] == "heritage_monument")
    assert monument["name"] == "Testi ohvrikivi"
    assert monument["distance_m"] == 50.0
    assert monument["source"] == "muinsuskaitse:kpo_malestised"
    zone = next(f for f in r if f["kind"] == "heritage_protection_zone")
    assert zone["distance_m"] == 0.0 and zone["name"] == "Testi muinsuskaitseala"
    assert r == sorted(r, key=lambda f: f["distance_m"])


def test_environment_flags_empty_list_is_not_an_error(net):
    net.add("geoserver", fc())
    assert se.environment_flags(0.0, 0.0) == []
    assert se.environment_flags.last_error is None


def test_environment_flags_none_when_all_layers_fail(net):
    assert se.environment_flags(0.0, 0.0) is None
    assert se.environment_flags.last_error


def test_soil_type(net):
    net.add("geoserver", fc(feat(square(0, 0, 10), varv=17, loimis_1=4)))
    r = se.soil_type(5.0, 5.0)
    assert r["group_code"] == 17 and r["group"] == "Madalsoomullad"


def test_soil_type_none_in_a_town(net):
    net.add("geoserver", fc())
    assert se.soil_type(5.0, 5.0) is None
    assert "no soil map polygon" in se.soil_type.last_error


# ---------------------------------------------------------------- geometry helpers

def test_distance_to_polygon_and_line():
    sq = [(0, 0), (10, 0), (10, 10), (0, 10)]
    assert se.distance_to(5, 5, sq, closed=True) == 0.0
    assert se.distance_to(15, 5, sq, closed=True) == 5.0
    assert se.distance_to(5, -3, [(0, 0), (10, 0)]) == 3.0
    assert se.distance_to(0, 0, [(3, 4)]) == 5.0


# ---------------------------------------------------------------- cache and offline mode

class FakeResponse:
    def __init__(self, body):
        self.body = body

    def read(self):
        return self.body

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


def test_cache_stores_the_response_and_offline_mode_reads_only_the_cache(tmp_path, monkeypatch):
    calls = []

    def fake_urlopen(req, timeout=None):
        calls.append(req.full_url)
        return FakeResponse(json.dumps(fc(PARCEL)).encode())

    monkeypatch.setattr(se.urllib.request, "urlopen", fake_urlopen)
    first = se.plot_boundary(115.0, 215.0, cache_dir=str(tmp_path))
    assert first["cadastral_number"] == "00000:000:0001"
    assert len(calls) == 1 and len(list(tmp_path.glob("*.bin"))) == 1

    def no_network(*a, **k):
        raise AssertionError("the network must not be used")

    monkeypatch.setattr(se.urllib.request, "urlopen", no_network)
    again = se.plot_boundary(115.0, 215.0, cache_dir=str(tmp_path), offline=True)
    assert again == first
    # A different request is not in the cache: None with a reason, no exception.
    assert se.plot_boundary(999.0, 999.0, cache_dir=str(tmp_path), offline=True) is None
    assert "offline" in se.plot_boundary.last_error


def test_cache_key_depends_on_range_and_body():
    k1 = se._cache_key("GET", "u", {"Range": "bytes=0-9"}, None)
    k2 = se._cache_key("GET", "u", {"Range": "bytes=10-19"}, None)
    k3 = se._cache_key("POST", "u", None, b"a")
    assert len({k1, k2, k3}) == 3
    assert k1 == se._cache_key("GET", "u", {"Range": "bytes=0-9"}, None)


def test_a_network_error_gives_none_and_a_reason(monkeypatch):
    def boom(req, timeout=None):
        raise urllib.error.URLError("no route")

    monkeypatch.setattr(se.urllib.request, "urlopen", boom)
    monkeypatch.setattr(se.time, "sleep", lambda s: None)
    assert se.plot_boundary(1.0, 2.0) is None
    assert "network error" in se.plot_boundary.last_error


def test_a_client_error_is_not_retried(monkeypatch):
    calls = []

    def forbidden(req, timeout=None):
        calls.append(1)
        raise urllib.error.HTTPError(req.full_url, 403, "Forbidden", {}, None)

    monkeypatch.setattr(se.urllib.request, "urlopen", forbidden)
    monkeypatch.setattr(se.time, "sleep", lambda s: None)
    assert se.plot_boundary(1.0, 2.0) is None
    assert len(calls) == 1 and "HTTP 403" in se.plot_boundary.last_error


def test_an_unexpected_error_inside_a_function_is_caught(net):
    net.add("kataster", b"this is not json")
    assert se.plot_boundary(1.0, 2.0) is None
    assert "not JSON" in se.plot_boundary.last_error


def test_every_request_sends_a_user_agent(monkeypatch):
    seen = {}

    def spy(req, timeout=None):
        seen.update(req.header_items())
        return FakeResponse(b"{}")

    monkeypatch.setattr(se.urllib.request, "urlopen", spy)
    se._http("https://example.test/x")
    assert "kadakas" in seen["User-agent"]
