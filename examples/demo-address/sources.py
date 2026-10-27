"""Public Estonian data sources for the address demo.

Every HTTP answer goes through `Fetcher`, which stores it in a cache folder. With `offline=True`
the fetcher reads only the cache, so the demo runs without a network after one online run.

Sources (no key needed):
  1. Land Board address search (inaadress gazetteer)
  2. Building register (EHR) open API
  3. Land Board WMS (orthophoto, basemap)

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import re
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

GAZETTEER_URL = "https://inaadress.maaamet.ee/inaadress/gazetteer"
EHR_URL = "https://livekluster.ehr.ee/api/building/v3/buildingdata"
WMS_URL = "https://kaart.maaamet.ee/wms/alus"
FOTOKAART_URL = "https://kaart.maaamet.ee/wms/fotokaart"
USER_AGENT = "kadakas-demo-address/0.1 (open source CAD demo)"


class OfflineMiss(Exception):
    """Offline mode and the answer is not in the cache."""


class FetchError(Exception):
    """The network or the service failed."""


def cache_key(url: str) -> str:
    """The name of a cache entry. The address of an address search is normalised (case, spaces,
    commas, unicode form), so "Street 1, Place" and "street 1 place" share one entry."""
    parts = urllib.parse.urlsplit(url)
    params = urllib.parse.parse_qsl(parts.query, keep_blank_values=True)
    fixed = []
    for key, value in params:
        if key == "address":
            value = unicodedata.normalize("NFC", value).casefold()
            value = re.sub(r"[\s,;.]+", " ", value).strip()
        fixed.append((key, value))
    fixed.sort()
    canonical = urllib.parse.urlunsplit((parts.scheme, parts.netloc.lower(), parts.path, urllib.parse.urlencode(fixed), ""))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()[:32]


def _host(url: str) -> str:
    return urllib.parse.urlsplit(url).netloc or "teenus"


class Fetcher:
    """GET with a disk cache. One file per request, named by `cache_key`.

    A bad answer is never stored: `validate` checks the body first. `log` prints the retry notes.
    Default timeout 8 s and one retry. A client error (HTTP 4xx) is not retried."""

    def __init__(self, cache_dir: Path | str, offline: bool = False, timeout: float = 8.0, retries: int = 1,
                 log=None):
        self.cache_dir = Path(cache_dir)
        self.offline = offline
        self.timeout = timeout
        self.retries = retries
        self.log = log or (lambda text: None)
        self.hits = 0
        self.downloads = 0

    @property
    def http_dir(self) -> Path:
        return self.cache_dir / "http"

    def _path(self, url: str) -> Path:
        return self.http_dir / (cache_key(url) + ".bin")

    def cached(self, url: str) -> bool:
        return self._path(url).exists()

    def get(self, url: str, validate=None) -> bytes:
        path = self._path(url)
        if path.exists():
            data = path.read_bytes()
            if validate is None or validate(data):
                self.hits += 1
                return data
            path.unlink()  # a bad answer from an older run
        if self.offline:
            raise OfflineMiss(_host(url))
        last: Exception | None = None
        data = b""
        for attempt in range(self.retries + 1):
            try:
                req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
                with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                    data = resp.read()
                last = None
                break
            except urllib.error.HTTPError as err:
                last = err
                if 400 <= err.code < 500:
                    break
            except (urllib.error.URLError, TimeoutError, OSError) as err:
                last = err
            if attempt < self.retries:
                self.log(f"  {_host(url)} ei vastanud, proovin uuesti ...")
                time.sleep(0.5)
        if last is not None:
            raise FetchError(f"{_host(url)}: {getattr(last, 'reason', last)}")
        if validate is not None and not validate(data):
            raise FetchError(f"{_host(url)} andis vigase vastuse")
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".tmp")
        tmp.write_bytes(data)
        tmp.replace(path)
        self.downloads += 1
        return data

    def get_json(self, url: str, key: str | None = None, nonempty: bool = True):
        def valid(data: bytes) -> bool:
            try:
                body = json.loads(data.decode("utf-8"))
            except ValueError:
                return False
            if not isinstance(body, dict):
                return False
            return (bool(body.get(key)) if nonempty else key in body) if key else bool(body)

        data = self.get(url, validate=valid)
        return json.loads(data.decode("utf-8"))


# ---------------------------------------------------------------- numbers

def fmt_num(value: float | int | None, decimals: int = 1) -> str:
    """Estonian number format: decimal comma, no thousands separator below 10 000, a space above."""
    if value is None:
        return "puudub"
    text = f"{abs(float(value)):,.{decimals}f}".replace(",", " ")
    whole, _, frac = text.partition(".")
    if len(whole.replace(" ", "")) < 5:
        whole = whole.replace(" ", "")
    text = whole + ("," + frac if frac else "")
    return ("-" if value < 0 and float(text.replace(",", ".").replace(" ", "")) != 0 else "") + text


def to_float(value) -> float | None:
    """Read a number that the register gives as text, '7.0', '7,0' or None."""
    if value is None:
        return None
    try:
        out = float(str(value).strip().replace(",", "."))
    except ValueError:
        return None
    return out


def to_int(value) -> int | None:
    number = to_float(value)
    return None if number is None else int(round(number))


# ---------------------------------------------------------------- addresses

@dataclass
class Candidate:
    address: str
    kind: str
    code: str
    x: float
    y: float
    lon: float | None = None
    lat: float | None = None

    @property
    def is_building(self) -> bool:
        return self.kind == "E"


def _norm(text: str) -> str:
    return re.sub(r"\s+", " ", text.lower().replace(",", " ").replace(".", " ")).strip()


def parse_candidates(payload: dict) -> list[Candidate]:
    out = []
    for item in (payload or {}).get("addresses") or []:
        x, y = to_float(item.get("viitepunkt_x")), to_float(item.get("viitepunkt_y"))
        if x is None or y is None:
            continue
        out.append(Candidate(
            address=str(item.get("pikkaadress") or item.get("taisaadress") or ""),
            kind=str(item.get("liik") or ""),
            code=str(item.get("tunnus") or ""),
            x=x, y=y,
            lon=to_float(item.get("viitepunkt_l")), lat=to_float(item.get("viitepunkt_b")),
        ))
    return out


def score_candidate(cand: Candidate, query: str) -> float:
    """Higher is better. A building beats other kinds. A word of the query that matches the place
    (not the county) counts most."""
    words = [w for w in _norm(query).split(" ") if w]
    parts = [p.strip().lower() for p in cand.address.split(",")]
    county = parts[0] if parts else ""
    places = " ".join(parts[1:])
    score = 10.0 if cand.is_building else 0.0
    for word in words:
        pat = r"(?<!\w)" + re.escape(word) + r"(?!\w)"
        if re.search(pat, places):
            score += 5.0 if not word.isdigit() else 3.0
        elif re.search(pat, county):
            score += 1.0
    return score


def rank_candidates(cands: list[Candidate], query: str) -> list[Candidate]:
    """Best first. Ties keep the order of the service."""
    indexed = list(enumerate(cands))
    indexed.sort(key=lambda p: (-score_candidate(p[1], query), p[0]))
    return [c for _, c in indexed]


def search_address(fetcher: Fetcher, text: str, results: int = 5) -> list[Candidate]:
    url = GAZETTEER_URL + "?" + urllib.parse.urlencode({"address": text, "results": results})
    return parse_candidates(fetcher.get_json(url, "addresses", nonempty=False))


# ---------------------------------------------------------------- building register

@dataclass
class Building:
    code: str
    address: str = ""
    name: str | None = None
    use: str | None = None
    footprint: list[tuple[float, float]] | None = None  # EPSG:3301, ring without the closing point
    footprint_area: float | None = None
    height: float | None = None
    floors: int | None = None
    floors_below: int | None = None
    built_area: float | None = None
    length: float | None = None
    width: float | None = None
    abs0: float | None = None
    year: int | None = None
    year_note: str | None = None
    parcels: list[str] = field(default_factory=list)
    foundation: str | None = None
    structure: str | None = None
    roof: str | None = None
    roof_material: str | None = None
    wall: str | None = None
    missing: list[str] = field(default_factory=list)  # names of the missing fields, in Estonian


def _ring(geometry) -> list[tuple[float, float]] | None:
    """The outer ring of a Polygon or the largest of a MultiPolygon, without the closing point."""
    if not isinstance(geometry, dict):
        return None
    polys = []
    if geometry.get("type") == "Polygon":
        polys = [geometry.get("coordinates")]
    elif geometry.get("type") == "MultiPolygon":
        polys = list(geometry.get("coordinates") or [])
    best, best_area = None, -1.0
    for poly in polys:
        try:
            ring = [(float(p[0]), float(p[1])) for p in poly[0]]
        except (TypeError, IndexError, ValueError):
            continue
        if len(ring) > 1 and ring[0] == ring[-1]:
            ring = ring[:-1]
        if len(ring) < 3:
            continue
        area = abs(polygon_area(ring))
        if area > best_area:
            best, best_area = ring, area
    return best


def polygon_area(ring: list[tuple[float, float]]) -> float:
    """Signed area by the shoelace formula, counterclockwise is positive."""
    total = 0.0
    for i, (x1, y1) in enumerate(ring):
        x2, y2 = ring[(i + 1) % len(ring)]
        total += x1 * y2 - x2 * y1
    return total / 2.0


def _tech(items, code: str) -> str | None:
    for item in items:
        if item.get("klKood") == code and item.get("nimetus"):
            return str(item["nimetus"])
    return None


def parse_building(payload: dict, code: str = "") -> Building:
    """Read the EHR answer. Every field may be missing. A missing field goes into `missing`."""
    e = (payload or {}).get("ehitis") or {}
    base = e.get("ehitisePohiandmed") or {}
    data = e.get("ehitiseAndmed") or {}
    shapes = (e.get("ehitiseKujud") or {}).get("ruumikuju") or []
    b = Building(code=str(data.get("ehrKood") or code))
    b.address = str(data.get("taisaadress") or "")
    b.name = data.get("nimetus")
    b.use = data.get("kaosIdTxt")
    for shape in shapes:
        if shape.get("tyhistatud_kp"):
            continue
        ring = _ring(shape.get("geometry"))
        if ring:
            b.footprint = ring
            b.footprint_area = to_float(shape.get("pindala")) or abs(polygon_area(ring))
            break
    b.height = to_float(base.get("korgus"))
    if b.height is not None and b.height <= 0:
        b.height = None
    b.floors = to_int(base.get("maxKorrusteArv"))
    if b.floors is not None and b.floors <= 0:
        b.floors = None
    b.floors_below = to_int(base.get("maaalusteKorrusteArv"))
    b.built_area = to_float(base.get("ehitisalunePind"))
    b.length = to_float(base.get("pikkus"))
    b.width = to_float(base.get("laius"))
    b.abs0 = to_float(base.get("abs0Korgus"))
    first = str(data.get("esmaneKasutus") or "")
    if re.match(r"\d{4}", first):
        b.year, b.year_note = int(first[:4]), "esmane kasutus"
    else:
        permit = str(base.get("kavKasutusKp") or "")
        if re.match(r"\d{4}", permit):
            b.year, b.year_note = int(permit[:4]), "kasutusloa kuupäev"
    b.parcels = [str(p["katastritunnus"]) for p in
                 ((e.get("ehitiseKatastriyksused") or {}).get("ehitiseKatastriyksus") or [])
                 if isinstance(p, dict) and p.get("katastritunnus")]
    tech = (e.get("ehitiseTehnilisedNaitajad") or {}).get("tehnilineNaitaja") or []
    b.foundation = _tech(tech, "KONS_VUND")
    b.structure = _tech(tech, "KONS_KKONSTR")
    b.roof_material = _tech(tech, "KONS_KATUS")
    b.wall = _tech(tech, "KONS_VSEIN")
    for label, value in (("kontuur", b.footprint), ("kõrgus", b.height), ("korruste arv", b.floors),
                         ("ehitusaasta", b.year), ("kandekonstruktsioon", b.structure)):
        if value is None:
            b.missing.append(label)
    return b


def fetch_building(fetcher: Fetcher, code: str) -> Building:
    url = EHR_URL + "?" + urllib.parse.urlencode({"ehr_code": code})
    payload = fetcher.get_json(url, "ehitis")
    return parse_building(payload, code)


# ---------------------------------------------------------------- map background

def wms_getmap_url(layer: str, bbox: tuple[float, float, float, float], size: int = 1024,
                   service: str = WMS_URL) -> str:
    """WMS 1.1.1 GetMap in EPSG:3301. Version 1.1.1 has the box as minx,miny,maxx,maxy (east, north),
    so no axis-order trouble."""
    q = {"service": "WMS", "version": "1.1.1", "request": "GetMap", "layers": layer, "styles": "",
         "srs": "EPSG:3301", "bbox": ",".join(f"{v:.2f}" for v in bbox), "width": size, "height": size,
         "format": "image/png"}
    return service + "?" + urllib.parse.urlencode(q)


def fetch_map(fetcher: Fetcher, bbox, layer: str = "of10000", size: int = 1024) -> bytes:
    service = FOTOKAART_URL if layer == "EESTIFOTO" else WMS_URL
    return fetcher.get(wms_getmap_url(layer, bbox, size, service), validate=lambda d: d.startswith(b"\x89PNG"))


def write_world_files(png: bytes, bbox, size: int, folder: Path, name: str) -> Path:
    """Write the PNG with a world file (.pgw) and a .prj, the way gis.geoimage.import reads them."""
    folder.mkdir(parents=True, exist_ok=True)
    x0, y0, x1, y1 = bbox
    px = (x1 - x0) / size
    py = (y1 - y0) / size
    png_path = folder / f"{name}.png"
    png_path.write_bytes(png)
    (folder / f"{name}.pgw").write_text(
        f"{px:.6f}\n0\n0\n{-py:.6f}\n{x0 + px / 2:.6f}\n{y1 - py / 2:.6f}\n", encoding="utf-8")
    (folder / f"{name}.prj").write_text(EPSG3301_WKT, encoding="utf-8")
    return png_path


EPSG3301_WKT = (
    'PROJCS["Estonian Coordinate System of 1997",GEOGCS["EST97",DATUM["Estonia_1997",'
    'SPHEROID["GRS 1980",6378137,298.257222101]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],'
    'PROJECTION["Lambert_Conformal_Conic_2SP"],PARAMETER["latitude_of_origin",57.51755393055556],'
    'PARAMETER["central_meridian",24],PARAMETER["standard_parallel_1",59.33333333333334],'
    'PARAMETER["standard_parallel_2",58],PARAMETER["false_easting",500000],'
    'PARAMETER["false_northing",6375000],UNIT["metre",1],AUTHORITY["EPSG","3301"]]'
)
