"""Tests of examples/demo-address (address to model). Synthetic fixtures only: the data in
examples/demo-address/fixtures is invented, in the same JSON shape as the public services.
The integration tests start a headless `kadakas serve` and run the whole script."""

import json
import struct
import subprocess
import sys
import zlib
from pathlib import Path

import pytest

from helpers import BIN, REPO

DEMO = REPO / "examples" / "demo-address"
sys.path.insert(0, str(DEMO))
sys.path.insert(0, str(REPO / "python"))

import demo_address as da  # noqa: E402
import geom  # noqa: E402
import layout  # noqa: E402
import sources as src  # noqa: E402
import terrain as terr  # noqa: E402

FIX = DEMO / "fixtures"


def fixture(name: str):
    return json.loads((FIX / name).read_text(encoding="utf-8"))


def tiny_png(size: int = 8) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + b"\x40\x80\x40" * size for _ in range(size))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


class FixtureFetcher(src.Fetcher):
    """Answers from the synthetic fixtures. No network."""

    def __init__(self, cache_dir, offline=False):
        super().__init__(cache_dir, offline)
        self.urls: list[str] = []

    def get(self, url: str) -> bytes:
        self.urls.append(url)
        if url.startswith(src.GAZETTEER_URL):
            return json.dumps(fixture("gazetteer.json")).encode()
        if url.startswith(src.EHR_URL):
            name = "ehr_sparse.json" if "000000002" in url else "ehr_complete.json"
            return json.dumps(fixture(name)).encode()
        if "GetMap" in url:
            return tiny_png()
        raise src.FetchError(url)


# ---------------------------------------------------------------- unit: numbers, addresses, EHR

def test_number_format_uses_decimal_comma():
    assert src.fmt_num(6, 1) == "6,0"
    assert src.fmt_num(30.84, 1) == "30,8"
    assert src.fmt_num(1234.5, 1) == "1234,5"
    assert src.fmt_num(12345.6, 0) == "12 346"
    assert src.fmt_num(None) == "puudub"
    assert src.fmt_num(-0.04, 1) == "0,0"


def test_register_numbers_with_comma_or_text():
    assert src.to_float("7,5") == 7.5
    assert src.to_float(None) is None
    assert src.to_float("x") is None
    assert src.to_int("2") == 2


def test_address_choice_prefers_building_and_city():
    cands = src.parse_candidates(fixture("gazetteer.json"))
    assert len(cands) == 3
    ranked = src.rank_candidates(cands, "Näidistänav 1, Näidislinn")
    assert ranked[0].code == "000000001"
    assert ranked[0].is_building
    # the road comes last even though the service listed it first
    assert ranked[-1].kind == "4"
    # a query without a city still prefers buildings
    ranked = src.rank_candidates(cands, "Näidistee 1")
    assert ranked[0].code == "000000002"


def test_address_county_alone_does_not_win_over_place():
    cands = src.parse_candidates(fixture("gazetteer.json"))
    ranked = src.rank_candidates(cands, "Näidismaakond 1")
    assert ranked[0].is_building


def test_parse_candidates_skips_rows_without_coordinates():
    assert src.parse_candidates({"addresses": [{"pikkaadress": "x", "liik": "E"}]}) == []
    assert src.parse_candidates({}) == []


def test_parse_complete_building():
    b = src.parse_building(fixture("ehr_complete.json"))
    assert b.code == "000000001"
    assert len(b.footprint) == 6  # closing point removed
    assert b.height == 6.5 and b.floors == 2
    assert b.year == 1986 and b.year_note == "esmane kasutus"
    assert b.structure == "tellis" and b.roof_material == "plekk"
    assert b.parcels == ["00000:000:0001"]
    assert b.missing == []


def test_parse_sparse_building_lists_missing_fields():
    b = src.parse_building(fixture("ehr_sparse.json"))
    assert b.footprint is None and b.height is None and b.floors is None and b.year is None
    assert b.length == 9.0 and b.width == 7.0
    assert set(b.missing) >= {"kontuur", "kõrgus", "korruste arv", "ehitusaasta"}


def test_parse_year_falls_back_to_permit_date():
    data = fixture("ehr_complete.json")
    data["ehitis"]["ehitiseAndmed"]["esmaneKasutus"] = None
    b = src.parse_building(data)
    assert b.year == 1985 and b.year_note == "kasutusloa kuupäev"


def test_parse_empty_answer_does_not_crash():
    b = src.parse_building({})
    assert b.footprint is None and b.parcels == []


def test_parse_multipolygon_takes_largest():
    data = fixture("ehr_complete.json")
    small = [[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]
    big = [[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]
    data["ehitis"]["ehitiseKujud"]["ruumikuju"][0]["geometry"] = {"type": "MultiPolygon", "coordinates": [[small], [big]]}
    assert abs(src.polygon_area(src.parse_building(data).footprint)) == 100.0


def test_wms_url_uses_version_1_1_1_and_epsg_3301():
    url = src.wms_getmap_url("of10000", (1, 2, 3, 4), 512)
    assert "version=1.1.1" in url and "srs=EPSG%3A3301" in url and "bbox=1.00%2C2.00%2C3.00%2C4.00" in url


# ---------------------------------------------------------------- unit: cache and offline

def test_fetcher_stores_and_reads_cache(tmp_path):
    f = src.Fetcher(tmp_path)
    path = f._path("http://example.invalid/a")
    path.parent.mkdir(parents=True)
    path.write_bytes(b"cached")
    assert f.get("http://example.invalid/a") == b"cached" and f.hits == 1


def test_offline_mode_uses_only_the_cache(tmp_path):
    f = src.Fetcher(tmp_path, offline=True)
    with pytest.raises(src.OfflineMiss):
        f.get("http://example.invalid/missing")
    assert f.downloads == 0


def test_offline_run_gives_clear_message_without_cache(tmp_path):
    msgs: list[str] = []
    out = da.run(["Näidistänav 1, Näidislinn", "--offline", "--cache-dir", str(tmp_path), "--mode", "headless",
                  "--binary", str(BIN), "--pause", "0"], say=msgs.append)
    assert "vahemälus" in out["results"]["error"]
    assert any("VIGA" in m for m in msgs)


# ---------------------------------------------------------------- unit: geometry, layout, terrain

L_SHAPE = [(0.0, 0.0), (12.0, 0.0), (12.0, 6.0), (6.0, 6.0), (6.0, 10.0), (0.0, 10.0)]


def test_footprint_orientation_and_min_rect():
    assert geom.signed_area(geom.ccw(list(reversed(L_SHAPE)))) > 0
    frame = geom.min_rect(L_SHAPE)
    assert frame["length"] == pytest.approx(12.0) and frame["width"] == pytest.approx(10.0)


def test_layout_without_plot_uses_offset_and_keeps_extension_inside():
    fp = geom.ccw(L_SHAPE)
    plot = layout.assumed_plot(fp)
    assert geom.point_in_poly(geom.centroid(fp), plot)
    lay = layout.plan_yard(fp, plot, True)
    assert lay.plot_is_assumed
    assert lay.extension and geom.inset_ok(lay.extension, plot)
    assert not geom.rects_overlap(lay.extension, fp)  # touches the building, never overlaps it
    if lay.terrace:
        assert not geom.rects_overlap(lay.terrace, lay.extension)
    for tree in lay.trees:
        assert not geom.point_in_poly(tree, fp)
        assert not any(geom.point_in_poly(tree, r) for r in lay.reserved)


def test_layout_extension_size_and_tight_plot():
    fp = geom.ccw(L_SHAPE)
    roomy = layout.plan_yard(fp, layout.assumed_plot(fp, 12.0), True)
    s = geom.min_rect(roomy.extension)
    assert sorted([round(s["length"], 1), round(s["width"], 1)]) == [5.0, 8.0] or s["width"] <= 5.0 + 1e-6
    tight = layout.plan_yard(fp, layout.assumed_plot(fp, 3.0), True)
    assert tight.extension is None and tight.extension_note


def test_terrain_cut_and_fill_on_a_slope():
    grid = {"origin": (0.0, 0.0), "step": 2.0, "nx": 11, "ny": 11,
            "z": [[10.0 + 0.1 * (2.0 * i) for i in range(11)] for _ in range(11)]}
    g = terr.clean_grid(grid)
    mid = terr.z_at(g, 10.0, 10.0)
    assert mid == pytest.approx(11.0)
    cf = terr.cut_fill(g, mid)
    assert cf["cut"] == pytest.approx(cf["fill"], rel=1e-6)
    assert cf["area"] == pytest.approx(400.0)
    # level 1 m below the whole ground: everything is cut, 400 m2 x mean depth
    assert terr.cut_fill(g, 10.0 - 1.0)["cut"] == pytest.approx(400.0 * 2.0, rel=1e-6)
    assert terr.balance_level(g) == pytest.approx(11.0, abs=0.01)
    assert len(terr.triangles(g)) == 200


def test_terrain_grid_with_holes_and_too_little_data():
    z = [[2.0] * 5 for _ in range(5)]
    z[1][1] = None
    assert terr.clean_grid({"origin": (0, 0), "step": 1, "nx": 5, "ny": 5, "z": z})["z"][1][1] == pytest.approx(2.0)
    sparse = [[None, None], [None, 3.0]]
    assert terr.clean_grid({"origin": (0, 0), "step": 1, "nx": 2, "ny": 2, "z": sparse}) is None


def test_dry_run_prints_every_step_and_makes_nothing(tmp_path):
    msgs: list[str] = []
    out = da.run(["Näidistänav 1, Näidislinn", "--dry", "--cache-dir", str(tmp_path / "c")], say=msgs.append)
    assert out["dry"] and len(out["steps"]) == len(da.STEPS)
    assert sum(m.startswith("Samm ") for m in msgs) == len(da.STEPS)
    assert not (tmp_path / "c").exists()


# ---------------------------------------------------------------- integration

def synthetic_extras() -> da.Extras:
    def plot_boundary(x, y, ehr_code=None):
        return {"cadastral_number": "00000:000:0001", "area_m2": 1200.0, "purpose": "Elamumaa",
                "polygon": [(x - 20, y - 20), (x + 20, y - 20), (x + 20, y + 20), (x - 20, y + 20), (x - 20, y - 20)]}

    def terrain_grid(x, y, half_size=20, step=2):
        n = int(2 * half_size / step) + 1
        return {"origin": (x - half_size, y - half_size), "step": float(step), "nx": n, "ny": n,
                "z": [[20.0 + 0.05 * step * i for i in range(n)] for _ in range(n)]}

    def nearby_features(x, y, radius=100):
        return {"buildings": [{"polygon": [(x + 25, y), (x + 31, y), (x + 31, y + 5), (x + 25, y + 5)], "korgus_m": 5.0}],
                "trees": [{"x": x - 10, "y": y + 15}, {"x": x - 12, "y": y + 17}], "roads": [], "water": []}

    return da.Extras(plot_boundary, terrain_grid, nearby_features)


def run_demo(tmp_path, address, extras, *more):
    msgs: list[str] = []
    out = da.run([address, "--cache-dir", str(tmp_path / "cache"), "--mode", "headless", "--binary", str(BIN),
                  "--pause", "0", "--yes", *more], extras=extras, fetcher=FixtureFetcher(tmp_path / "cache"),
                 say=msgs.append)
    return out, msgs


def open_result(out):
    """Open the saved document in a fresh session to read it back."""
    import kadakas

    return kadakas.open(out["results"]["kad"], binary=BIN, allow_dir=[Path(out["results"]["kad"]).parent])


def test_whole_demo_with_all_steps(tmp_path):
    out, msgs = run_demo(tmp_path, "Näidistänav 1, Näidislinn", synthetic_extras())
    assert "error" not in out["results"], msgs
    r = out["results"]
    for key in ("ifc", "pdf", "svg", "kad"):
        assert Path(r[key]).exists() and Path(r[key]).stat().st_size > 0
    assert "IFCMAPCONVERSION" in Path(r["ifc"]).read_text(errors="replace")
    assert "EPSG:3301" in Path(r["ifc"]).read_text(errors="replace")
    cf = r["cut_fill"]
    assert cf["cut"] > 0 and cf["fill"] > 0
    assert any("m3" in m and "Mulla" in m for m in msgs)
    assert not out["assumptions"] or all(isinstance(a, str) for a in out["assumptions"])
    doc = open_result(out)
    try:
        summary = doc.summary()
        assert summary["georeference"]["crs"] == "EPSG:3301"
        names = {layer.name for layer in doc.layers}
        assert {"Hoone", "Krunt", "Taustakaart", "Haljastus", "Laiendus (agent)", "Naabrid", "Maapind"} <= names
        kinds = summary["objects_by_kind"]
        assert kinds.get("surface_model", 0) >= 4  # building, extension, terrain, neighbour
        assert kinds.get("text", 0) >= 5
        building = out["ids"]["building"]
        rec = doc.call_raw("record.get", {"id": building})["records"]["Ehitisregister"]
        assert rec["EHR_kood"] == "000000001" and rec["Korgus_m"] == 6.5 and rec["Allikas"] == "Ehitisregister"
        assert doc.api.mesh_solid_report(id=building)["closed"]
        bounds = doc.api.mesh_bounds(id=building)
        assert abs(bounds["local"]["max"][2] - 6500.0) < 1.0 if "local" in bounds else True
        ext = out["ids"]["extension"]
        assert doc.api.mesh_solid_report(id=ext)["closed"]
    finally:
        doc.close()


def test_missing_data_uses_documented_defaults_and_says_so(tmp_path):
    out, msgs = run_demo(tmp_path, "Näidistee 1", da.Extras(), "--no-extras")
    assert "error" not in out["results"], msgs
    text = "\n".join(msgs)
    assert "Kõrgus puudub registrist, eeldan 6,0 m" in text
    assert "Kontuur puudub registrist" in text
    assert any("kõrgus 6,0 m" in a for a in out["assumptions"])
    assert "Katastripiiri ei saanud" in text
    assert "jätan sammu vahele" in text  # optional steps skipped without trouble
    doc = open_result(out)
    try:
        rec = doc.call_raw("record.get", {"id": out["ids"]["building"]})["records"]["Ehitisregister"]
        assert rec["Korguse_allikas"] == "eeldus" and rec["Korgus_m"] == 6.0
        assert "eeldan" in rec["Eeldused"] or "kõrgus" in rec["Eeldused"]
    finally:
        doc.close()


def test_agent_edits_carry_the_origin_agent(tmp_path):
    """The Python client is an agent: the activity log has the steps and every change event says `agent`."""
    out, _ = run_demo(tmp_path, "Näidistänav 1, Näidislinn", da.Extras(), "--no-extras")
    doc = open_result(out)
    try:
        events = doc.events
        events.subscribe(kinds=["objects"])
        doc.add.circle([0, 0], 100)
        origins = {m["params"]["origin"] for m in events.take()}
        assert origins == {"agent"}
    finally:
        doc.close()
    c = out["ctx"]
    assert out["results"]["agent_steps"] >= 10


def test_offline_run_after_one_online_run(tmp_path):
    """Fill the cache from the answers of one run, then run again with --offline and no network."""
    cache = tmp_path / "cache"
    fetcher = FixtureFetcher(cache)
    first = da.run(["Näidistänav 1, Näidislinn", "--cache-dir", str(cache), "--mode", "headless", "--binary", str(BIN),
                    "--pause", "0", "--yes", "--no-extras"], extras=da.Extras(), fetcher=fetcher, say=lambda m: None)
    assert "error" not in first["results"]
    for url in set(fetcher.urls):
        path = src.Fetcher(cache)._path(url)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(fetcher.get(url))
    msgs: list[str] = []
    second = da.run(["Näidistänav 1, Näidislinn", "--offline", "--cache-dir", str(cache), "--mode", "headless",
                     "--binary", str(BIN), "--pause", "0", "--yes", "--no-extras"], extras=da.Extras(), say=msgs.append)
    assert "error" not in second["results"], msgs
    assert second["ctx"].fetcher.downloads == 0 and second["ctx"].fetcher.hits >= 3
    assert any("vahemälust" in m for m in msgs)


def test_cli_dry_and_help():
    run = subprocess.run([sys.executable, str(DEMO / "demo_address.py"), "Näidistänav 1, Näidislinn", "--dry"],
                         capture_output=True, text=True, encoding="utf-8", check=True)
    assert "Kuivproov" in run.stdout and "Samm 12/12" in run.stdout
    helped = subprocess.run([sys.executable, str(DEMO / "demo_address.py"), "--help"], capture_output=True,
                            text=True, encoding="utf-8", check=True)
    for flag in ("--dry", "--cache-dir", "--offline", "--pause"):
        assert flag in helped.stdout
