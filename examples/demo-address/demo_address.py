#!/usr/bin/env python3
"""Address to model: an agent-style demo for Kadakas.

You give an Estonian address. The script looks it up in public registers, builds a georeferenced
model in Kadakas step by step, adds a yard concept and an extension as agent edits for review, and
exports IFC and a sheet. Messages are in Estonian. See README.md.

    python3 demo_address.py "<address>" --pause enter
"""

from __future__ import annotations

import argparse
import datetime as dt
import inspect
import os
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
try:
    import kadakas  # noqa: E402
except ImportError:  # run from a checkout without installing the package
    sys.path.insert(0, str(HERE.parents[1] / "python"))
    import kadakas  # noqa: E402

import geom  # noqa: E402
import layout as lay  # noqa: E402
import sources as src  # noqa: E402
import terrain as terr  # noqa: E402
from build import LAYERS, PLANTS, RECORD_BUILDING, Builder  # noqa: E402
from sources import fmt_num as n  # noqa: E402

DEFAULT_STOREY_M = 3.0
DEFAULT_ROOF_M = 1.5
DEFAULT_HEIGHT_NO_FLOORS_M = 6.0
DEFAULT_FOOTPRINT = (10.0, 8.0)
SOURCE_LAND_BOARD = "Maa- ja Ruumiamet (Republic of Estonia Land and Spatial Development Board), avaandmete litsents 01.01.2025"
SOURCE_EHR = "Ehitisregister (EHR), avalik API"


def korrust(count: int) -> str:
    return "1 korrus" if count == 1 else f"{count} korrust"


class DemoError(Exception):
    """A step failed in a way that stops the demo. The message is Estonian and ready to print."""


# ------------------------------------------------------------------ optional extra sources

@dataclass
class Extras:
    """Optional sources from sources_extra.py. Each is None when missing, and each call may return None."""
    plot_boundary: Callable | None = None
    terrain_grid: Callable | None = None
    nearby_features: Callable | None = None


EXTRAS_BUDGET_S = 20.0


def _wrap(fn: Callable, cache_dir: Path, offline: bool, budget: list[float]) -> Callable:
    """Call an optional source. It never raises and never blocks the demo for long: all optional
    calls together get `EXTRAS_BUDGET_S` seconds. The cache is the same folder as the other answers."""
    params = inspect.signature(fn).parameters

    def call(*a, **kw):
        if "cache_dir" in params:
            kw.setdefault("cache_dir", cache_dir / "http")
        if "offline" in params:
            kw.setdefault("offline", offline)
        limit = budget[0]
        if limit < 1.0:
            return None
        box: dict = {}

        def work():
            try:
                box["value"] = fn(*a, **kw)
            except Exception:  # noqa: BLE001  # an optional source never stops the demo
                box["value"] = None

        started = time.monotonic()
        worker = threading.Thread(target=work, daemon=True)
        worker.start()
        worker.join(limit)
        budget[0] = max(0.0, budget[0] - (time.monotonic() - started))
        if worker.is_alive():
            budget[0] = 0.0
            return None
        return box.get("value")

    return call


def load_extras(cache_dir: Path, offline: bool, budget_s: float = EXTRAS_BUDGET_S) -> Extras:
    try:
        import sources_extra as se  # type: ignore
    except ImportError:
        return Extras()
    got = {name: getattr(se, name, None) for name in ("plot_boundary", "terrain_grid", "nearby_features")}
    budget = [budget_s]
    return Extras(**{k: (_wrap(v, cache_dir, offline, budget) if v else None) for k, v in got.items()})


# ------------------------------------------------------------------ context

@dataclass
class Ctx:
    args: argparse.Namespace
    say: Callable[[str], None]
    fetcher: src.Fetcher
    extras: Extras
    doc: Any = None
    b: Builder | None = None
    cand: src.Candidate | None = None
    building: src.Building | None = None
    footprint: list = field(default_factory=list)          # metres, EPSG:3301, counterclockwise
    height: float = 0.0
    floors: int = 0
    assumptions: list[str] = field(default_factory=list)
    sources: list[str] = field(default_factory=list)
    plot: list | None = None
    plot_info: dict | None = None
    layout: lay.Layout | None = None
    grid: dict | None = None
    ids: dict = field(default_factory=dict)
    results: dict = field(default_factory=dict)
    out_dir: Path = Path(".")
    today: str = dt.date.today().isoformat()

    def assume(self, text: str) -> None:
        self.assumptions.append(text)
        self.say("  Eeldus: " + text)


# ------------------------------------------------------------------ steps

def step_search(c: Ctx) -> None:
    c.say(f"Otsin aadressi: {c.args.address} ...")
    try:
        cands = src.search_address(c.fetcher, c.args.address)
    except src.OfflineMiss:
        raise DemoError("Võrguta režiim: seda aadressi pole vahemälus. Käivita üks kord võrguga (vt README, eelsoojendus).")
    except src.FetchError as err:
        raise DemoError(f"Aadressiotsing ei vastanud ({err}). Kontrolli internetti või kasuta --offline koos vahemäluga.")
    if not cands:
        raise DemoError("Aadressi ei leitud. Proovi kirjutada tänav, maja number ja linn, näiteks „Näidistänav 1, Näidislinn“.")
    ranked = src.rank_candidates(cands, c.args.address)
    c.say(f"Leidsin {len(ranked)} vastet:")
    for i, cand in enumerate(ranked, 1):
        kind = "hoone" if cand.is_building else f"liik {cand.kind}"
        c.say(f"  {i}. {cand.address} ({kind})")
    choice = 1
    if c.args.pick:
        choice = c.args.pick
    elif len(ranked) > 1 and not c.args.yes and sys.stdin.isatty():
        raw = input(f"Vali number (Enter = 1, parim vaste): ").strip()
        choice = int(raw) if raw.isdigit() else 1
    if not 1 <= choice <= len(ranked):
        raise DemoError(f"Valik {choice} on väljaspool vahemikku 1 kuni {len(ranked)}.")
    cand = ranked[choice - 1]
    if not cand.is_building:
        buildings = [x for x in ranked if x.is_building]
        if not buildings:
            raise DemoError("Sellel aadressil pole hoonet (ainult krunt või tee). Anna hoone aadress.")
        near = min(buildings, key=lambda x: (x.x - cand.x) ** 2 + (x.y - cand.y) ** 2)
        dist = ((near.x - cand.x) ** 2 + (near.y - cand.y) ** 2) ** 0.5
        c.say(f"  See vaste ei ole hoone. Lähim hoone on {near.address} ({n(dist, 0)} m kaugusel).")
        if sys.stdin.isatty() and not c.args.yes:
            if input("  Kasutan seda hoonet? (Enter = jah, n = ei): ").strip().lower().startswith("n"):
                raise DemoError("Katkestatud: anna täpsem aadress.")
        cand = near
    c.cand = cand
    c.say(f"Valisin: {cand.address}, EHR-kood {cand.code}")
    c.sources.append(f"Aadress: {SOURCE_LAND_BOARD}, päritud {c.today}")


def step_register(c: Ctx) -> None:
    assert c.cand
    c.say(f"Küsin ehitisregistrist hoone {c.cand.code} andmeid ...")
    try:
        b = src.fetch_building(c.fetcher, c.cand.code)
    except src.OfflineMiss:
        raise DemoError("Võrguta režiim: selle hoone andmeid pole vahemälus.")
    except src.FetchError as err:
        raise DemoError(f"Ehitisregister ei vastanud ({err}).")
    c.building = b
    c.say(f"Leidsin hoone EHR-koodiga {b.code}: {b.name or 'nimetus puudub'}"
          + (f" ({b.use})" if b.use else ""))
    c.sources.append(f"Hoone: {SOURCE_EHR}, ehr.ee, päritud {c.today}")

    # footprint
    if b.footprint:
        c.footprint = geom.ccw(b.footprint)
        c.say(f"  Kontuur: {len(b.footprint)} nurka, pindala {n(b.footprint_area)} m2")
    else:
        length, width = b.length or DEFAULT_FOOTPRINT[0], b.width or DEFAULT_FOOTPRINT[1]
        cx, cy = c.cand.x, c.cand.y
        c.footprint = [(cx - length / 2, cy - width / 2), (cx + length / 2, cy - width / 2),
                       (cx + length / 2, cy + width / 2), (cx - length / 2, cy + width / 2)]
        c.say("  Kontuur puudub registrist.")
        c.assume(f"hoone kontuur on ristkülik {n(length)} x {n(width)} m aadressipunkti ümber")
    # floors and height
    c.floors = b.floors or 0
    if b.height is not None:
        c.height = b.height
        c.say(f"  Kõrgus {n(b.height)} m, " + (korrust(c.floors) if c.floors else "korruste arv puudub"))
    elif c.floors:
        c.height = c.floors * DEFAULT_STOREY_M + DEFAULT_ROOF_M
        c.say(f"  Kõrgus puudub registrist, eeldan {n(c.height)} m.")
        c.assume(f"kõrgus {n(c.height)} m ({c.floors} korrust x {n(DEFAULT_STOREY_M)} m ja katus {n(DEFAULT_ROOF_M)} m)")
    else:
        c.height = DEFAULT_HEIGHT_NO_FLOORS_M
        c.say(f"  Kõrgus puudub registrist, eeldan {n(c.height)} m")
        c.assume(f"kõrgus {n(c.height)} m (registris puudub kõrgus ja korruste arv)")
    if not c.floors:
        c.floors = max(1, round(c.height / DEFAULT_STOREY_M))
        c.say("  Korruste arv puudub registrist.")
        c.assume(f"{korrust(c.floors)} (kõrgus jagatud {n(DEFAULT_STOREY_M)} m korrusega)")
    if b.year:
        c.say(f"  Ehitusaasta {b.year} ({b.year_note})")
    else:
        c.say("  Ehitusaasta puudub registrist, jätan tühjaks.")
    if b.structure:
        c.say(f"  Kandekonstruktsioon: {b.structure}, katusekate: {b.roof_material or 'puudub'}")
    if b.parcels:
        c.say(f"  Katastriüksus: {', '.join(b.parcels)}")


def step_georef_and_map(c: Ctx) -> None:
    assert c.b and c.cand
    centre = geom.centroid(c.footprint)
    geo = c.b.set_georeference(centre)
    c.say(f"Panen dokumendile georeferentsi {geo['crs']}, lähtepunkt "
          f"E {n(c.b.origin[0], 0)} N {n(c.b.origin[1], 0)} m (L-EST97).")
    # Everything that the later steps need is made here. A later step then changes only objects, so
    # the review panel can revert it alone (a step that also adds a layer or a class cannot be reverted).
    optional = not c.args.no_extras
    keys = ["building", "plot", "background", "landscape", "report", "extension"]
    if optional and c.extras.terrain_grid:
        keys += ["ground"]
    if optional and c.extras.nearby_features:
        keys += ["neighbours", "trees"]
    c.b.make_layers([LAYERS[k] for k in keys])
    c.b.make_class("Agendi ettepanek", [220, 120, 20, 255], [250, 200, 140, 255])
    c.b.make_class("Maapind", [120, 100, 80, 255])
    c.b.make_class("Naabrid", [140, 140, 140, 255], [200, 200, 200, 255])
    c.b.make_record_formats()
    c.say("Loon kihid: " + ", ".join(LAYERS[k] for k in keys) + ".")
    xmin, ymin, xmax, ymax = geom.bbox(c.footprint)
    half = max(50.0, max(xmax - xmin, ymax - ymin) / 2 + 40.0)
    box = (centre[0] - half, centre[1] - half, centre[0] + half, centre[1] + half)
    c.say(f"Küsin Maa- ja Ruumiametilt taustakaardi ({n(2 * half, 0)} x {n(2 * half, 0)} m) ...")
    layers = [c.args.map_layer] + [x for x in ("MA-ALUS",) if x != c.args.map_layer]
    for layer_name in layers:
        try:
            png = src.fetch_map(c.fetcher, box, layer_name, 1024)
        except (src.FetchError, src.OfflineMiss) as err:
            c.say(f"  Kaardikiht {layer_name} ei tulnud ({'pole vahemälus' if isinstance(err, src.OfflineMiss) else err}).")
            continue
        folder = c.out_dir / "map"
        path = src.write_world_files(png, box, 1024, folder, "taustakaart")
        try:
            c.b.background(path)
        except Exception as err:  # noqa: BLE001
            c.say(f"  Kaarti ei õnnestunud mudelisse panna ({err}). Jätan taustakaardi vahele.")
            return
        c.say(f"Taustakaart ({layer_name}) on kihil {LAYERS['background']}.")
        c.sources.append(f"Taustakaart ({layer_name}): {SOURCE_LAND_BOARD}, päritud {c.today}")
        return
    c.say("Taustakaarti ei saanud, jätkan ilma selleta.")


def step_building(c: Ctx) -> None:
    assert c.b and c.building
    b, e = c.b, c.building
    name = c.cand.address if c.cand else e.address
    short = short_address(name)
    outline = b.outline(c.footprint, LAYERS["building"])
    c.ids["outline"] = outline.id
    c.say(f"Joonistasin hoone kontuuri kihile {LAYERS['building']} ({len(c.footprint)} nurka).")
    made = b.massing(c.footprint, c.height, f"Hoone {short}", LAYERS["building"])
    mid = made["id"]
    c.ids["building"] = mid
    q = made.get("quantities", {})
    c.say(f"Tegin kontuurist {n(c.height)} m kõrguse hoonemahu (massing.add, üks samm).")
    values = {
        "EHR_kood": e.code, "Aadress": short, "Nimetus": e.name or "", "Korruseid": str(c.floors),
        "Korgus_m": round(c.height, 2),
        "Korguse_allikas": "Ehitisregister" if e.height is not None else "eeldus",
        "Ehitusaasta": str(e.year) if e.year else "puudub", "Kandekonstruktsioon": e.structure or "puudub",
        "Katusekate": e.roof_material or "puudub", "Katastritunnus": ", ".join(e.parcels),
        "Allikas": "Ehitisregister", "Andmed_laaditud": c.today,
        "Eeldused": "; ".join(c.assumptions) if c.assumptions else "puuduvad",
    }
    b.attach([mid, outline.id], RECORD_BUILDING, values)
    c.say("Lisasin hoonele atribuudid: EHR-kood, aadress, korrused, kõrgus, aasta, konstruktsioon, allikas, kuupäev.")
    xmin, ymin, xmax, ymax = geom.bbox(c.footprint)
    label = f"{short}  |  {korrust(c.floors)}  |  {n(c.height)} m"
    b.text(((xmin + xmax) / 2 - 0.5 * 0.45 * len(label) / 2, ymin - 2.5), label, 450, LAYERS["building"])
    c.say("Lisasin aadressisildi.")
    c.results["volume_m3"] = q.get("volume_m3")
    if q.get("volume_m3"):
        c.say(f"Hoone maht mudelis: {n(q['volume_m3'], 0)} m3, ehitisealune pind {n(q.get('footprint_m2'), 0)} m2.")


def step_plot(c: Ctx) -> None:
    assert c.b
    centre = geom.centroid(c.footprint)
    info = None
    if c.extras.plot_boundary and not c.args.no_extras:
        c.say("Küsin katastriüksuse piiri ...")
        info = c.extras.plot_boundary(centre[0], centre[1], ehr_code=c.building.code if c.building else None)
    if info and info.get("polygon"):
        c.plot = geom.ccw([tuple(p) for p in info["polygon"] if p is not None][:-1]
                          if tuple(info["polygon"][0]) == tuple(info["polygon"][-1]) else [tuple(p) for p in info["polygon"]])
        c.plot_info = info
        c.say(f"Leidsin katastriüksuse {info.get('cadastral_number')}, pindala {n(info.get('area_m2'), 0)} m2, "
              f"sihtotstarve: {info.get('purpose') or 'puudub'}.")
        c.sources.append(f"Krunt ({info.get('source') or 'katastri WFS'}): {SOURCE_LAND_BOARD}, päritud {c.today}")
        assumed = False
    else:
        c.plot = lay.assumed_plot(c.footprint)
        assumed = True
        c.say("Katastripiiri ei saanud.")
        c.assume(f"krundi piir on hoone ümber {n(lay.DEFAULT_PLOT_MARGIN, 0)} m nihutatud ristkülik")
    c.ids["plot_is_assumed"] = assumed
    line = c.b.outline(c.plot, LAYERS["plot"])
    c.ids["plot"] = line.id
    label = (f"Krunt {info.get('cadastral_number')}" if not assumed else "Krunt (eeldatud piir)")
    xmin, ymin, xmax, ymax = geom.bbox(c.plot)
    c.b.text((xmin, ymax + 1.0), label, 400, LAYERS["plot"])
    c.say(f"Joonistasin krundi piiri kihile {LAYERS['plot']}.")
    c.layout = lay.plan_yard(c.footprint, c.plot, assumed, c.height)


def step_terrain(c: Ctx) -> None:
    assert c.b
    if not c.extras.terrain_grid or c.args.no_extras:
        c.say("Maapinna andmeallikas pole selles töös kasutusel, jätan sammu vahele.")
        return
    centre = geom.centroid(c.footprint)
    px0, py0, px1, py1 = geom.bbox(c.plot or c.footprint)
    half = int(min(60, max(20, round(max(px1 - px0, py1 - py0) / 2 + 5))))
    c.say(f"Küsin Maa- ja Ruumiametilt maapinna kõrgusmudelit (lidar, 1 m), ala {n(2 * half, 0)} x {n(2 * half, 0)} m, "
          f"2 m sammuga ...")
    raw = c.extras.terrain_grid(centre[0], centre[1], half_size=half, step=2)
    grid = terr.clean_grid(raw) if raw else None
    if not grid:
        c.say("Maapinna andmeid ei saanud, jätan sammu vahele. Mudel jääb tasasele pinnale.")
        return
    c.grid = grid
    z0 = round(terr.z_at(grid, *centre), 2)
    c.b.z0 = z0
    c.b.terrain_surface(grid, z0, LAYERS["ground"], "Maapind")
    zs = [v for row in grid["z"] for v in row]
    c.say(f"Maapinna mudel: {grid['nx']} x {grid['ny']} sõlme, samm {n(grid['step'], 0)} m, "
          f"kõrgus {n(min(zs), 2)} kuni {n(max(zs), 2)} m. Mudeli null on hoone juures ({n(z0, 2)} m).")
    area = c.plot if c.plot else None
    level_building = 0.0 + z0
    cf = terr.cut_fill(grid, level_building, area)
    best = terr.balance_level(grid, area)
    cf_b = terr.cut_fill(grid, best, area)
    c.results["cut_fill"] = {"level": level_building, "cut": cf["cut"], "fill": cf["fill"], "net": cf["net"],
                             "balance_level": best, "balance_cut": cf_b["cut"], "balance_fill": cf_b["fill"],
                             "area_m2": cf["area"]}
    c.say(f"Mulla mahud tasasel tasemel {n(level_building, 2)} m (hoone nullkõrgus), "
          f"pindala {n(cf['area'], 0)} m2:")
    c.say(f"  Kaeve {n(cf['cut'], 0)} m3, täide {n(cf['fill'], 0)} m3, vahe {n(cf['net'], 0)} m3 (positiivne tähendab äravedu).")
    c.say(f"  Tasakaalutase {n(best, 2)} m: kaeve {n(cf_b['cut'], 0)} m3, täide {n(cf_b['fill'], 0)} m3.")
    c.assume(f"mulla mahud on arvutatud tasasele tasemele {n(level_building, 2)} m krundi piires, mullakihti ei arvestata")
    c.sources.append(f"Maapind (lidari 1 m kõrgusmudel): {SOURCE_LAND_BOARD}, päritud {c.today}")
    xmin, ymin, xmax, ymax = geom.bbox(c.plot or c.footprint)
    c.b.text((xmin, ymin - 3.0), f"Kaeve {n(cf['cut'], 0)} m3, täide {n(cf['fill'], 0)} m3 tasemel {n(level_building, 2)} m",
             350, LAYERS["ground"])


def _poly_of(item: Any) -> list | None:
    if isinstance(item, dict):
        poly = item.get("polygon") or item.get("footprint")
        if poly is None and isinstance(item.get("geometry"), dict):
            coords = item["geometry"].get("coordinates")
            if coords and item["geometry"].get("type") == "Polygon":
                poly = coords[0]
        if poly is None:
            return None
    else:
        poly = item
    try:
        ring = [(float(p[0]), float(p[1])) for p in poly]
    except (TypeError, ValueError, IndexError):
        return None
    if len(ring) > 1 and ring[0] == ring[-1]:
        ring = ring[:-1]
    return ring if len(ring) >= 3 else None


def _point_of(item: Any) -> tuple | None:
    try:
        if isinstance(item, dict):
            if "x" in item and "y" in item:
                return float(item["x"]), float(item["y"])
            g = item.get("geometry")
            if isinstance(g, dict) and g.get("type") == "Point":
                return float(g["coordinates"][0]), float(g["coordinates"][1])
            if item.get("point"):
                return float(item["point"][0]), float(item["point"][1])
            return None
        return float(item[0]), float(item[1])
    except (TypeError, ValueError, IndexError, KeyError):
        return None


def step_neighbours(c: Ctx) -> None:
    assert c.b
    if not c.extras.nearby_features or c.args.no_extras:
        c.say("Naabrite andmeallikas pole selles töös kasutusel, jätan sammu vahele.")
        return
    centre = geom.centroid(c.footprint)
    c.say("Küsin ümbruse hooned ja puud ...")
    feats = c.extras.nearby_features(centre[0], centre[1], radius=80)
    if not feats:
        c.say("Ümbruse andmeid ei saanud, jätan sammu vahele.")
        return
    own = geom.centroid(c.footprint)
    count = guessed = 0
    for item in (feats.get("buildings") or [])[:40]:
        ring = _poly_of(item)
        if not ring:
            continue
        ctr = geom.centroid(ring)
        if abs(ctr[0] - own[0]) + abs(ctr[1] - own[1]) < 1.0 or geom.point_in_poly(ctr, c.footprint):
            continue  # the building itself
        height = None
        if isinstance(item, dict):
            height = src.to_float(item.get("height_m") or item.get("korgus_m") or item.get("height"))
        h = height if height else 4.0
        guessed += 0 if height else 1
        try:
            c.b.massing(ring, h, "Naaber", LAYERS["neighbours"], "Naabrid")
            count += 1
        except Exception:  # noqa: BLE001
            continue
    trees = 0
    for item in (feats.get("trees") or [])[:80]:
        pt = _point_of(item)
        if pt:
            c.b.doc.add.circle(c.b.to_mm(pt), 1500, layer=LAYERS["trees"], class_="Naabrid")
            trees += 1
    c.say(f"Panin naabrite kihile {count} hoonet (halliga) ja {trees} puud.")
    if guessed:
        c.assume(f"{guessed} naabrihoone kõrgus puudub allikast, eeldan 4,0 m")
    origin = str(feats.get("source") or "ETAK")
    if "openstreetmap" in origin.lower():
        c.sources.append(f"Ümbrus: © OpenStreetMap contributors (ODbL), päritud {c.today}")
    else:
        c.sources.append(f"Ümbrus ({origin}): {SOURCE_LAND_BOARD}, päritud {c.today}")


def step_yard(c: Ctx) -> None:
    if not c.layout:
        c.say("Krundi plaani pole, jätan õuekontseptsiooni vahele.")
        return
    assert c.b
    L = c.layout
    c.say("Maastikuarhitekti samm: agent teeb krundile õuekontseptsiooni.")
    c.b.make_plant_styles()
    c.say("Lisasin taimestiku: " + ", ".join(p[1] for p in PLANTS) + ".")
    place = LAYERS["landscape"]
    if L.terrace:
        c.b.pave(L.terrace, "Puitterrass", place)
        c.say("Terrass 5,0 x 3,5 m hoone küljel, kus on vaba ruumi.")
    if L.path:
        c.b.pave(L.path, "Killustik", place)
        c.say("Jalgtee 1,2 m laiune terrassilt krundi piirini.")
    if L.parking:
        c.b.pave(L.parking, "Kivisillutis", place)
        c.say("Parkimiskoht 2,5 x 5,0 m.")
    ids = c.b.plant_points("kuusk", L.trees[:1], place) + c.b.plant_points("kask", L.trees[1:2], place) \
        + c.b.plant_points("pihlakas", L.trees[2:3], place)
    shrub = c.b.plant_points("sirel", L.shrubs[:2], place) + c.b.plant_points("maran", L.shrubs[2:], place)
    hedge = c.b.plant_hedge(L.hedge, place) if L.hedge else []
    c.ids["plants"] = ids + shrub + hedge
    c.say(f"Istutasin {len(ids)} puud, {len(shrub)} põõsast ja {len(hedge)} taime heki jaoks.")
    rows = c.b.plant_schedule()
    c.results["schedule"] = rows
    c.say("Taimede ostunimekiri:")
    for row in rows:
        c.say(f"  {row.get('id_code') or row.get('code') or '':3} {row.get('name') or row.get('common_name') or row.get('style') or '':22} "
              f"{row.get('count', '')} tk")
    xmin, ymin, xmax, ymax = geom.bbox(c.plot)
    y = ymax - 2.0
    c.b.text((xmax + 3.0, y), "Taimede nimekiri", 450, place)
    for row in rows:
        y -= 0.9
        c.b.text((xmax + 3.0, y), f"{row.get('id_code') or ''} {row.get('name') or row.get('common_name') or ''}: {row.get('count', '')} tk",
                 330, place)
    c.assume("õueplaan on kontseptsioon, mitte projekt: mõõdud ja taimed on näidis")


def step_extension(c: Ctx) -> None:
    if not c.layout:
        c.say("Krundi plaani pole, jätan laienduse vahele.")
        return
    assert c.b
    L = c.layout
    c.say("Ehitusarhitekti samm: agent pakub välja laienduse mahu eraldi kihile.")
    if not L.extension:
        c.say(f"  {L.extension_note}")
        c.say("  Laiendust ei lisa.")
        return
    floor_h = c.height / max(c.floors, 1)
    lower = c.height - floor_h
    ext_h = max(lower, lay.MIN_STOREY)
    clamped = lower < lay.MIN_STOREY
    size = geom.min_rect(L.extension)
    long_side, short_side = max(size["length"], size["width"]), min(size["length"], size["width"])
    area = abs(geom.signed_area(L.extension))
    name = f"Laiendus (agendi ettepanek) {n(long_side)} x {n(short_side)} m, kõrgus {n(ext_h)} m"
    # One call, one undo step: the person can revert it alone. The layer and the class exist since step 3.
    made = c.b.massing(L.extension, ext_h, name, LAYERS["extension"], "Agendi ettepanek")
    c.ids["extension"] = made["id"]
    c.results["extension"] = {"length": long_side, "width": short_side, "height": ext_h, "area": area, "clamped": clamped}
    c.say(f"Laienduse mõõdud on {n(long_side)} x {n(short_side)} m, kõrgus {n(ext_h)} m, pindala {n(area)} m2.")
    if clamped:
        c.say(f"  Põhihoone on madal, seepärast on laiendus alampiirini {n(lay.MIN_STOREY)} m kõrge.")
    else:
        c.say(f"  See on põhihoonest üks korrus ({n(floor_h)} m) madalam.")
    c.say(f"Valisin külje, kus krundil on kõige rohkem vaba ruumi ({n(L.sides[L.extension_side], 0)} m).")
    if L.extension_note:
        c.say("  " + L.extension_note)
    c.say(f"See on eraldi kihil {LAYERS['extension']}, ühe sammuna: inimene võtab selle vastu või tühistab (Revert).")
    c.assume("laiendus on mahu ettepanek, mitte projekt: seina- ja katuse detaile pole")
    xmin, ymin, xmax, ymax = geom.bbox(L.extension)
    c.b.text((xmin, ymax + 0.8), f"Laiendus (ettepanek) {n(ext_h)} m", 350, LAYERS["extension"])


def step_report(c: Ctx) -> None:
    assert c.b
    lines = ["ALLIKAD JA EELDUSED", f"Koostatud {c.today} Kadakase agendi poolt", ""]
    lines += ["Allikad:"] + ["  " + s for s in dict.fromkeys(c.sources)] + ["", "Eeldused:"]
    lines += ["  " + a for a in c.assumptions] or ["  puuduvad"]
    lines += ["", "Mudel on mahumudel, mitte projekt. Kõik agendi muudatused vaatab inimene üle."]
    xmin, ymin, xmax, ymax = geom.bbox(c.plot or c.footprint)
    y = ymin + 22.0 if (ymax - ymin) > 22 else ymax
    x = xmax + 3.0
    c.say("Panen allikad ja eeldused aruandena mudelisse:")
    for i, line in enumerate(lines):
        c.b.text((x, y - 0.7 * i - 14.0), line, 380 if i else 520, LAYERS["report"])
    for s in lines[3:]:
        if s:
            c.say("  " + s.strip())


def step_export(c: Ctx) -> None:
    assert c.b
    out = c.out_dir
    out.mkdir(parents=True, exist_ok=True)
    short = "hoone"
    ifc = out / f"{short}.ifc"
    try:
        rep = c.b.export(ifc, "ifc", {"massing_as_building": True})
        c.say(f"IFC fail (georeferentsiga EPSG:3301): {ifc}")
        c.results["ifc"] = str(ifc)
    except Exception as err:  # noqa: BLE001
        c.say(f"IFC eksport ei õnnestunud: {err}")
        c.say(HINT_DIRS)
    try:
        sheet = c.b.doc.api.sheet_create(name="Ülevaade", paper="a3", orientation="landscape", number="L-001",
                                         title="Krundi ülevaade")["layer"]
        c.b.doc.api.viewport_create(sheet=sheet, scale="fit", view="plan")
        for fmt, ext in (("pdf", "pdf"), ("sheet_svg", "svg")):
            path = out / f"leht.{ext}"
            c.b.export(path, fmt, {"sheets": [sheet]} if fmt == "pdf" else {"sheet": sheet})
            c.say(f"Joonise leht ({ext.upper()}): {path}")
            c.results[ext] = str(path)
    except Exception as err:  # noqa: BLE001
        c.say(f"Joonise lehe eksport ei õnnestunud: {err}")
    kad = out / "mudel.kad"
    try:
        c.b.doc.save(kad)
        c.say(f"Kadakase dokument: {kad}")
        c.results["kad"] = str(kad)
    except Exception as err:  # noqa: BLE001
        c.say(f"Dokumenti ei saanud salvestada: {err}")
    for view, name in (("top", "ylalt"), ("right_isometric", "isomeetria")):
        try:
            c.b.render(out / f"{name}.png", view)
            c.say(f"Pilt: {out / (name + '.png')}")
        except Exception:  # noqa: BLE001
            pass


def step_review(c: Ctx) -> None:
    assert c.b
    log = c.b.doc.agent_activity()
    steps = log.get("steps", [])
    c.results["agent_steps"] = len(steps)
    c.say(f"Agent tegi {len(steps)} muudatussammu. Need on rakenduse agendi tegevuse paneelis (Agent Activity).")
    c.say("Viimased sammud (rakenduse tekst inglise keeles):")
    for s in steps[-6:]:
        c.say(f"  {s.get('summary')}")
    ext_step = next((s for s in reversed(steps) if s.get("method") == "massing.add"), None)
    if c.ids.get("extension") and ext_step is not None:
        ok = bool(ext_step.get("revertible"))
        c.results["extension_revertible"] = ok
        c.say(f"Laienduse samm (nr {ext_step.get('id')}) on {'tagasivõetav (Revert)' if ok else 'EI OLE tagasivõetav'}.")
    ext = c.ids.get("extension")
    if ext:
        try:
            c.b.doc.view_zoom(ids=[ext], margin=2.0)
            c.b.doc.view_highlight(ids=[ext], text="Uus laiendus: vaata üle", color="orange", until="dismissed")
            c.say("Tõstsin laienduse ekraanil esile. Vajuta paneelis sellel sammul Revert.")
        except Exception:  # noqa: BLE001  # headless sessions have no window
            c.say("Esiletõstmine töötab ainult rakenduses (mitte aknata sessioonis).")
    c.say("Valmis.")


# ------------------------------------------------------------------ plumbing

STEPS: list[tuple[str, str, str, Callable[[Ctx], None]]] = [
    ("search", "Aadressi otsing",
     "otsib aadressi Maa- ja Ruumiameti aadressiregistrist (inaadress.maaamet.ee), valib parima vaste ja küsib kinnitust", step_search),
    ("register", "Hoone andmed ehitisregistrist",
     "küsib ehitisregistrist (livekluster.ehr.ee) kontuuri, kõrguse, korruste arvu ja muud; puuduvad väljad märgib eeldusena", step_register),
    ("map", "Georeferents ja taustakaart",
     "paneb dokumendile georeferentsi EPSG:3301, loob kihid ja toob Maa- ja Ruumiameti WMS-ist taustakaardi", step_georef_and_map),
    ("building", "Hoone mudel",
     "joonistab kontuuri, teeb hoonemahu ühe käsuga (massing.add), lisab atribuudid ja aadressisildi", step_building),
    ("plot", "Krunt",
     "toob katastripiiri (valikuline allikas) või teeb kontuuri nihutatud eeldatud piiri", step_plot),
    ("terrain", "Maapind ja mullatööd",
     "valikuline: toob lidari kõrgusmudeli, teeb maapinna mudeli ja arvutab kaevamise ning täite mahud", step_terrain),
    ("neighbours", "Ümbrus",
     "valikuline: toob naabrihooned ja puud hallina eraldi kihtidele", step_neighbours),
    ("yard", "Õuekontseptsioon (maastikuarhitekt)",
     "paigutab terrassi, tee, parkimiskoha, puud, põõsad ja heki ning teeb taimede nimekirja", step_yard),
    ("extension", "Laiendus (ehitusarhitekt)",
     "pakub umbes 5 x 8 m laienduse mahu eraldi kihile, ühe korruse võrra madalama, ühe sammuna", step_extension),
    ("report", "Allikad ja eeldused", "kirjutab allikate ja eelduste aruande mudelisse", step_report),
    ("export", "Eksport",
     "salvestab IFC (georeferentsiga), PDF- ja SVG-lehe, Kadakase faili ja pildid", step_export),
    ("review", "Ülevaatus",
     "näitab agendi muudatuste logi ja tõstab laienduse rakenduses esile", step_review),
]


FATAL_STEPS = {"search", "register", "map", "building"}

HINT_DIRS = ("  Vihje: rakendus kirjutab ainult lubatud kausta. Käivita see võtmega --allow-dir <vahemälu kaust> "
             "või kasuta --out-dir kausta, mis on lubatud.")


def short_address(full: str) -> str:
    """'County, Municipality, Place, Street 5' -> 'Street 5, Place'."""
    parts = [p.strip() for p in full.split(",") if p.strip()]
    if len(parts) >= 2:
        return f"{parts[-1]}, {parts[-2]}"
    return full


def pause(c: Ctx, label: str) -> None:
    mode = c.args.pause
    if mode == "enter":
        if sys.stdin.isatty():
            input(f"\n--- Vajuta Enter, et jätkata ({label}) ---")
        print()
    else:
        time.sleep(float(mode))
        print()


def open_document(args: argparse.Namespace, allow: list[Path], say: Callable[[str], None] = print):
    mode = args.mode
    if mode in ("auto", "app"):
        try:
            return kadakas.connect(), "rakendus"
        except Exception as err:  # noqa: BLE001
            if mode == "app":
                raise DemoError("Kadakase rakendus ei vasta. Käivita see võtmega --agent-port 0 (vt README) "
                                f"või kasuta --mode headless. ({str(err)[:80]})")
            say("!" * 70)
            say("!!  EI OLE RAKENDUSEGA ÜHENDATUD, töötan aknata sessioonis (publik ei näe midagi).")
            say("!!  Käivita rakendus võtmega --agent-port 0 ja kasuta --mode app.")
            say("!" * 70)
            if sys.stdin.isatty() and not args.yes:
                input("Vajuta Enter, et jätkata aknata sessioonis, või Ctrl+C, et katkestada ... ")
    return kadakas.open(binary=args.binary, allow_dir=[str(p) for p in allow]), "aknata sessioon"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Aadress mudeliks: Kadakase agendi demo.")
    p.add_argument("address", help="Eesti aadress, näiteks 'Näidistänav 1, Näidislinn'")
    p.add_argument("--dry", action="store_true", help="ainult prindi, mida tehtaks")
    p.add_argument("--cache-dir", default=str(HERE / ".cache"), help="HTTP vastuste vahemälu (vaikimisi .cache/)")
    p.add_argument("--offline", action="store_true", help="kasuta ainult vahemälu")
    p.add_argument("--pause", default="3", help="paus sammude vahel: sekundid või 'enter' (vaikimisi 3)")
    p.add_argument("--yes", action="store_true", help="ära küsi kinnitust, võta parim aadress")
    p.add_argument("--pick", type=int, default=0, help="vali aadressi vaste number")
    p.add_argument("--mode", choices=["auto", "app", "headless"], default="app",
                   help="app: ühendu töötava rakendusega (vaikimisi); headless: käivita kadakas serve aknata; auto: proovi rakendust, kuid hoiata")
    p.add_argument("--binary", default=os.environ.get("KADAKAS_BIN"), help="kadakas programmi asukoht")
    p.add_argument("--out-dir", default=None, help="väljundkaust (vaikimisi <cache-dir>/out)")
    p.add_argument("--map-layer", default="of10000", help="WMS kiht: of10000 (ortofoto) või MA-ALUS (aluskaart)")
    p.add_argument("--extras-budget", type=float, default=EXTRAS_BUDGET_S,
                   help="sekundeid, mis valikulised allikad kokku võivad kuluda (vaikimisi 20)")
    p.add_argument("--no-extras", action="store_true", help="ära kasuta valikulisi allikaid (krunt, maapind, ümbrus)")
    a = p.parse_args(argv)
    if a.pause != "enter":
        try:
            float(a.pause)
        except ValueError:
            p.error("--pause: anna sekundid või 'enter'")
    return a


def run(argv: list[str] | None = None, *, extras: Extras | None = None, fetcher: src.Fetcher | None = None,
        say: Callable[[str], None] = print) -> dict:
    args = parse_args(argv)
    cache = Path(args.cache_dir).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else cache / "out"
    c = Ctx(args=args, say=say, fetcher=fetcher or src.Fetcher(cache, offline=args.offline, log=say),
            extras=extras if extras is not None else (Extras() if args.dry else load_extras(cache, args.offline, args.extras_budget)),
            out_dir=out_dir)
    if args.dry:
        say(f"Kuivproov (--dry): ei küsi võrgust ega muuda midagi. Aadress: {args.address}")
        for i, (key, title, what, _) in enumerate(STEPS, 1):
            say(f"Samm {i}/{len(STEPS)}: {title}\n  Teeks: {what}.")
        say("Allikad: " + "; ".join([SOURCE_LAND_BOARD, SOURCE_EHR]))
        return {"dry": True, "steps": [s[0] for s in STEPS]}
    try:
        cache.mkdir(parents=True, exist_ok=True)
        out_dir.mkdir(parents=True, exist_ok=True)
        c.doc, how = open_document(args, [cache, out_dir], say)
        c.b = Builder(c.doc, out_dir)
        if c.doc.summary().get("objects_by_kind"):
            say("HOIATUS: dokumendis on juba objekte. Uued objektid lisatakse nende kõrvale. "
                "Puhta lehe jaoks vali rakenduses Fail > Uus ja käivita skript uuesti.")
            if sys.stdin.isatty() and not args.yes:
                if input("Jätkan ikkagi? (Enter = jah, n = ei): ").strip().lower().startswith("n"):
                    raise DemoError("Katkestatud: dokument ei ole tühi.")
            say("")
        say(f"Ühendatud: {how}. Vahemälu: {cache}{' (võrguta režiim)' if args.offline else ''}\n")
        for i, (key, title, _what, fn) in enumerate(STEPS, 1):
            say(f"=== Samm {i}/{len(STEPS)}: {title} ===")
            try:
                fn(c)
            except DemoError:
                raise
            except Exception as err:  # noqa: BLE001
                say(f"VIGA sammus {title}: {str(err)[:200]}. Mudel on alles, vaata Agendi tegevust.")
                c.results.setdefault("step_errors", []).append(key)
                if key in FATAL_STEPS:
                    raise DemoError(f"Samm „{title}“ ebaõnnestus, edasi ei saa.") from err
            if i < len(STEPS):
                pause(c, title)
        say(f"Võrgupäringuid: {c.fetcher.downloads}, vahemälust: {c.fetcher.hits}.")
    except DemoError as err:
        say(f"\nVIGA: {err}")
        c.results["error"] = str(err)
    except KeyboardInterrupt:
        say("\nKatkestatud.")
        c.results["error"] = "Katkestatud"
    finally:
        if c.doc is not None and how_serve(c.doc):
            try:
                c.doc.close()
            except Exception:  # noqa: BLE001
                pass
    return {"ctx": c, "results": c.results, "ids": c.ids, "assumptions": c.assumptions}


def how_serve(doc) -> bool:
    """True when the document is a private `kadakas serve` process that we close ourselves."""
    return doc.__class__.__name__ == "Document" and type(getattr(doc, "_transport", None)).__name__ == "StdioTransport"


if __name__ == "__main__":
    result = run()
    sys.exit(1 if result.get("results", {}).get("error") else 0)
