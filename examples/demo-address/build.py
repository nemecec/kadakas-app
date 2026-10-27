"""Kadakas model operations for the address demo.

`Builder` wraps a kadakas Document. The map frame is EPSG:3301 in metres. The model frame is
millimetres relative to the origin that the georeference stores (a whole-metre point near the
building), so the model needs no huge numbers. Every call here comes from the Python client, so
Kadakas marks it with the origin `agent` and the review panel shows it.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import geom
import terrain as terr
from geom import Pt

LAYERS = {
    "building": "Hoone",
    "plot": "Krunt",
    "background": "Taustakaart",
    "landscape": "Haljastus",
    "report": "Aruanne",
    "extension": "Laiendus (agent)",
    "ground": "Maapind",
    "neighbours": "Naabrid",
    "trees": "Puud",
}

RECORD_BUILDING = "Ehitisregister"

PLANTS = [
    # key, common name, botanical name, id code, category, spread mm, height mm, form, fill
    ("kuusk", "Harilik kuusk", "Picea abies", "T1", "tree", 4000, 12000, "conical", [40, 110, 60, 255]),
    ("kask", "Arukask", "Betula pendula", "T2", "tree", 6000, 14000, "tree", [110, 170, 80, 255]),
    ("pihlakas", "Harilik pihlakas", "Sorbus aucuparia", "T3", "tree", 4500, 8000, "tree", [150, 190, 90, 255]),
    ("sirel", "Harilik sirel", "Syringa vulgaris", "S1", "shrub", 2500, 3000, "shrub_mound", [150, 110, 170, 255]),
    ("maran", "Põõsasmaran", "Dasiphora fruticosa", "S2", "shrub", 1000, 800, "shrub_mound", [230, 200, 60, 255]),
    ("elupuu", "Elupuu Smaragd", "Thuja occidentalis 'Smaragd'", "H1", "shrub", 900, 2000, "columnar_conifer", [30, 100, 50, 255]),
]


class Builder:
    def __init__(self, doc, work_dir: Path):
        self.doc = doc
        self.work_dir = Path(work_dir)
        self.origin: tuple[float, float] = (0.0, 0.0)
        self.z0: float = 0.0  # absolute height (m) that z = 0 stands for, when a terrain is known
        self.ids: dict[str, Any] = {}
        self.styles: dict[str, str] = {}

    # ----------------------------------------------------------- frames

    def to_mm(self, p: Pt) -> list[float]:
        return [round((p[0] - self.origin[0]) * 1000.0, 1), round((p[1] - self.origin[1]) * 1000.0, 1)]

    def ring_mm(self, ring: list[Pt]) -> list[list[float]]:
        return [self.to_mm(p) for p in ring]

    # ----------------------------------------------------------- document

    def set_georeference(self, centre: Pt, crs: str = "EPSG:3301") -> dict:
        self.origin = (float(round(centre[0])), float(round(centre[1])))
        result = self.doc.api.gis_georef_set(crs=crs, origin=list(self.origin))
        return result["georeference"]

    def make_layers(self, names: list[str]) -> None:
        existing = {l.name for l in self.doc.layers}
        for name in names:
            if name not in existing:
                self.doc.add_layer(name)

    def make_class(self, name: str, pen: list[int], fill: list[int] | None = None) -> None:
        if name not in {c.name for c in self.doc.classes}:
            attrs: dict[str, Any] = {"pen": pen}
            if fill:
                attrs["fill"] = fill
            self.doc.add_class(name, **attrs)

    def make_record_formats(self) -> None:
        existing = {f.get("name") for f in self.doc.api.record_format_list().get("formats", [])}
        if RECORD_BUILDING not in existing:
            self._make_building_format()

    def _make_building_format(self) -> None:
        self.doc.api.record_format_create(name=RECORD_BUILDING, fields=[
            {"name": "EHR_kood", "type": "text"},
            {"name": "Aadress", "type": "text"},
            {"name": "Nimetus", "type": "text"},
            {"name": "Korruseid", "type": "text"},
            {"name": "Korgus_m", "type": "number"},
            {"name": "Korguse_allikas", "type": "text"},
            {"name": "Ehitusaasta", "type": "text"},
            {"name": "Kandekonstruktsioon", "type": "text"},
            {"name": "Katusekate", "type": "text"},
            {"name": "Katastritunnus", "type": "text"},
            {"name": "Allikas", "type": "text"},
            {"name": "Andmed_laaditud", "type": "date"},
            {"name": "Eeldused", "type": "text"},
        ])

    # ----------------------------------------------------------- shapes

    def massing(self, ring: list[Pt], height_m: float, name: str, layer: str, class_: str | None = None) -> str:
        """A building block in ONE call (massing.add): one undo step, a closed solid, a flat roof.
        Returns the id of the object."""
        params: dict[str, Any] = {"footprint": self.ring_mm(geom.ccw(ring)), "height": round(height_m * 1000.0, 1),
                                  "roof": "flat", "name": name, "layer": layer}
        if class_:
            params["class"] = class_
        return self.doc.call("massing.add", **params)

    def outline(self, ring: list[Pt], layer: str, class_: str | None = None) -> Any:
        kw: dict[str, Any] = {"layer": layer}
        if class_:
            kw["class_"] = class_
        return self.doc.add.polyline(self.ring_mm(ring), closed=True, **kw)

    def text(self, at: Pt, text: str, height_mm: float, layer: str) -> Any:
        return self.doc.add.text(self.to_mm(at), text, height=height_mm, layer=layer)

    def attach(self, ids: list[str], record: str, values: dict) -> None:
        self.doc.api.record_attach(ids=ids, record=record, values=values)

    # ----------------------------------------------------------- background

    def background(self, png_path: Path, name: str = "Taustakaart") -> dict:
        result = self.doc.api.gis_geoimage_import(path=str(png_path), name=name, layer=LAYERS["background"],
                                                  crs="EPSG:3301")
        return result

    # ----------------------------------------------------------- plants and paving

    def make_plant_styles(self) -> dict[str, str]:
        known = {s.get("name"): s.get("id") for s in self.doc.plants.styles().get("styles", [])}
        for key, common, botanical, code, cat, spread, height, form, fill in PLANTS:
            if common in known:
                self.styles[key] = known[common]
                continue
            res = self.doc.api.plant_style_create(
                name=common, common_name=common, botanical_name=botanical, id_code=code,
                category=cat, spread=spread, height=height, form=form, fill=fill)
            self.styles[key] = res["id"]
        return self.styles

    def pave(self, ring: list[Pt], material: str, layer: str) -> Any:
        return self.doc.api.hardscape_add(boundary=self.ring_mm(geom.ccw(ring)), layer=layer, material=material)

    def plant_points(self, key: str, points: list[Pt], layer: str) -> list[str]:
        ids: list[str] = []
        for p in points:
            res = self.doc.api.plants_place(mode="single", style=self.styles[key], at=self.to_mm(p), layer=layer)
            ids += res.get("created", [])
        return ids

    def plant_hedge(self, line: list[Pt], layer: str) -> list[str]:
        res = self.doc.api.plants_place(mode="hedgerow", style=self.styles["elupuu"],
                                        path=self.ring_mm(line), spacing=900, rows=1, layer=layer)
        return res.get("created", [])

    def plant_schedule(self) -> list[dict]:
        rep = self.doc.report.plant_schedule()
        return rep.get("rows") or rep.get("items") or []

    # ----------------------------------------------------------- terrain

    def terrain_surface(self, grid: dict, z0: float, layer: str, class_: str | None) -> str:
        """A surface model from the grid: triangles with the height relative to z0."""
        made = self.doc.api.mesh_create(name="Maapind", layer=layer, **({"class": class_} if class_ else {}))
        mid = made["id"]
        seen: set = set()
        edges: list = []
        for tri in terr.triangles(grid, z0):
            pts = [[*self.to_mm((p[0], p[1])), round(p[2] * 1000.0, 1)] for p in tri]
            for i in range(3):
                a, b = pts[i], pts[(i + 1) % 3]
                key = tuple(sorted((tuple(a), tuple(b))))
                if key not in seen:
                    seen.add(key)
                    edges.append([a, b])
        for start in range(0, len(edges), 4000):
            self.doc.api.mesh_add_edges(id=mid, edges=edges[start:start + 4000])
        return mid

    # ----------------------------------------------------------- export and pictures

    def export(self, path: Path, fmt: str, options: dict | None = None) -> dict:
        return self.doc.api.export_file(path=str(path), format=fmt, overwrite=True, **({"options": options} if options else {}))

    def render(self, path: Path, view: str, width: int = 1400, height: int = 900) -> None:
        self.doc.render_png(str(path), view=view, width=width, height=height)
