"""Extra public data sources for the address demo (Estonia, no API keys).

Every function takes plain numbers in EPSG:3301 (L-EST97, x east, y north) and returns plain Python
data (dicts, lists, tuples of floats). A function never raises: on a failure it returns None, sets
``<function>.last_error`` to a short reason and logs a warning.

Caching: each function takes ``cache_dir`` (raw responses are stored there, key = SHA-256 of the
request) and ``offline`` (read the cache only, never touch the network). Real responses describe real
property, so the cache directory must never be committed.

Sources (see docs/research/estonian-open-data.md for terms and attribution):
  * Parcels:   Maa- ja Ruumiamet cadastre WFS on gsavalik.envir.ee (kataster:ky_kehtiv)
  * Terrain:   Maa- ja Ruumiamet lidar DTM 1 m GeoTIFF, read by HTTP range requests
  * Features:  Maa- ja Ruumiamet ETAK WFS, fallback OpenStreetMap through Overpass
  * Flags:     Keskkonnaamet and Muinsuskaitseamet layers on the same GeoServer
  * Soil:      Maa- ja Ruumiamet soil map (WFS veeveeb:liht_mullakaart)
Pure standard library. No numpy, no GDAL.
"""
from __future__ import annotations

import functools
import hashlib
import json
import logging
import math
import os
import struct
import time
import urllib.error
import urllib.parse
import urllib.request
import zlib
from itertools import accumulate

log = logging.getLogger("demo.sources_extra")

USER_AGENT = "kadakas-demo/0.1 (+https://github.com/nemecec/kadakas-app)"
GEOSERVER = "https://gsavalik.envir.ee/geoserver"
DTM_URL = (
    "https://geoportaal.maaruum.ee/index.php?lang_id=1&plugin_act=otsing&kaardiruut={sheet}"
    "&andmetyyp=dem_1m_geotiff&dl=1&f={sheet}_dtm_1m.tif&page_id=614"
)
OVERPASS_URLS = (
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
)
DEFAULT_TIMEOUT = 40


class SourceError(Exception):
    """A source failed. The message is the reason."""


# ----------------------------------------------------------------------------------------------
# EPSG:3301 (L-EST97) <-> WGS84. Lambert conformal conic, two standard parallels, GRS80.
# ----------------------------------------------------------------------------------------------
_A = 6378137.0
_F = 1 / 298.257222101
_E = math.sqrt(2 * _F - _F * _F)
_LAT1 = math.radians(59 + 20 / 60)
_LAT2 = math.radians(58.0)
_LAT0 = math.radians(57.517553930555556)
_LON0 = math.radians(24.0)
_FE = 500000.0
_FN = 6375000.0


def _m(lat):
    s = math.sin(lat)
    return math.cos(lat) / math.sqrt(1 - _E * _E * s * s)


def _t(lat):
    s = math.sin(lat)
    return math.tan(math.pi / 4 - lat / 2) / ((1 - _E * s) / (1 + _E * s)) ** (_E / 2)


_N = (math.log(_m(_LAT1)) - math.log(_m(_LAT2))) / (math.log(_t(_LAT1)) - math.log(_t(_LAT2)))
_FF = _m(_LAT1) / (_N * _t(_LAT1) ** _N)
_R0 = _A * _FF * _t(_LAT0) ** _N


def wgs84_to_lest97(lat, lon):
    """(lat, lon) in degrees -> (x east, y north) in EPSG:3301."""
    la, lo = math.radians(lat), math.radians(lon)
    r = _A * _FF * _t(la) ** _N
    th = _N * (lo - _LON0)
    return (_FE + r * math.sin(th), _FN + _R0 - r * math.cos(th))


def lest97_to_wgs84(x, y):
    """(x east, y north) in EPSG:3301 -> (lat, lon) in degrees."""
    dx, dy = x - _FE, y - _FN
    rho = math.copysign(math.hypot(dx, _R0 - dy), _N)
    th = math.atan2(dx, _R0 - dy)
    t = (rho / (_A * _FF)) ** (1 / _N)
    lat = math.pi / 2 - 2 * math.atan(t)
    for _ in range(12):
        s = math.sin(lat)
        lat = math.pi / 2 - 2 * math.atan(t * ((1 - _E * s) / (1 + _E * s)) ** (_E / 2))
    return (math.degrees(lat), math.degrees(th / _N + _LON0))


# ----------------------------------------------------------------------------------------------
# HTTP with a request cache and an offline mode
# ----------------------------------------------------------------------------------------------
def _cache_key(method, url, headers, data):
    h = hashlib.sha256()
    h.update(method.encode())
    h.update(b"\0" + url.encode())
    h.update(b"\0" + (headers or {}).get("Range", "").encode())
    h.update(b"\0" + (data or b""))
    return h.hexdigest()


def _http(url, *, cache_dir=None, offline=False, headers=None, data=None, timeout=DEFAULT_TIMEOUT,
          retries=1):
    """GET (or POST when data is given). Returns bytes. Raises SourceError."""
    method = "POST" if data is not None else "GET"
    path = None
    if cache_dir:
        path = os.path.join(cache_dir, _cache_key(method, url, headers, data) + ".bin")
        if os.path.exists(path):
            with open(path, "rb") as fh:
                return fh.read()
    if offline:
        raise SourceError("offline mode and the response is not in the cache")
    hdr = {"User-Agent": USER_AGENT, "Accept": "*/*"}
    hdr.update(headers or {})
    last = None
    for attempt in range(retries + 1):
        try:
            req = urllib.request.Request(url, data=data, headers=hdr, method=method)
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                body = resp.read()
            break
        except urllib.error.HTTPError as exc:
            last = f"HTTP {exc.code} from {urllib.parse.urlsplit(url).netloc}"
            if exc.code < 500 and exc.code != 429:
                raise SourceError(last) from exc
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            last = f"network error: {getattr(exc, 'reason', exc)}"
        time.sleep(1.0)
    else:
        raise SourceError(last or "request failed")
    if path:
        os.makedirs(cache_dir, exist_ok=True)
        tmp = path + ".part"
        with open(tmp, "wb") as fh:
            fh.write(body)
        os.replace(tmp, path)
    return body


def _json(url, **kw):
    body = _http(url, **kw)
    try:
        return json.loads(body.decode("utf-8"))
    except ValueError as exc:
        raise SourceError(f"not JSON: {body[:80]!r}") from exc


def _never_raises(fn):
    """Return None instead of raising. The reason goes to fn.last_error and the log."""

    @functools.wraps(fn)
    def wrapper(*args, **kwargs):
        wrapper.last_error = None
        try:
            return fn(*args, **kwargs)
        except SourceError as exc:
            wrapper.last_error = str(exc)
        except Exception as exc:  # noqa: BLE001 - the demo must not crash on a source
            wrapper.last_error = f"{type(exc).__name__}: {exc}"
        log.warning("%s failed: %s", fn.__name__, wrapper.last_error)
        return None

    wrapper.last_error = None
    return wrapper


# ----------------------------------------------------------------------------------------------
# Geometry helpers
# ----------------------------------------------------------------------------------------------
def _seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    L = dx * dx + dy * dy
    t = 0.0 if L == 0 else max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / L))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def _inside(px, py, ring):
    c = False
    n = len(ring)
    for i in range(n):
        x1, y1 = ring[i]
        x2, y2 = ring[(i + 1) % n]
        if (y1 > py) != (y2 > py) and px < (x2 - x1) * (py - y1) / (y2 - y1) + x1:
            c = not c
    return c


def distance_to(px, py, pts, closed=False):
    """Distance in metres from a point to a polyline or polygon outline. Zero inside a polygon."""
    if not pts:
        return float("inf")
    if len(pts) == 1:
        return math.hypot(px - pts[0][0], py - pts[0][1])
    if closed and _inside(px, py, pts):
        return 0.0
    segs = zip(pts, pts[1:] + ([pts[0]] if closed else []))
    return min(_seg_dist(px, py, a[0], a[1], b[0], b[1]) for a, b in segs)


def _ring(coords):
    return [(float(c[0]), float(c[1])) for c in coords]


def _geoms(geometry):
    """GeoJSON geometry -> list of ('poly'|'line'|'point', points). Holes are dropped."""
    if not geometry:
        return []
    t, c = geometry.get("type"), geometry.get("coordinates")
    if t == "Polygon":
        return [("poly", _ring(c[0]))]
    if t == "MultiPolygon":
        return [("poly", _ring(p[0])) for p in c]
    if t == "LineString":
        return [("line", _ring(c))]
    if t == "MultiLineString":
        return [("line", _ring(p)) for p in c]
    if t == "Point":
        return [("point", [(float(c[0]), float(c[1]))])]
    if t == "MultiPoint":
        return [("point", [(float(p[0]), float(p[1]))]) for p in c]
    return []


def _polygon_area(ring):
    return abs(sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(ring, ring[1:] + ring[:1]))) / 2


# ----------------------------------------------------------------------------------------------
# WFS helper
# ----------------------------------------------------------------------------------------------
def _wfs(layer, *, bbox=None, cql=None, count=500, workspace_path="ows", **kw):
    """GetFeature as GeoJSON in EPSG:3301. bbox = (xmin, ymin, xmax, ymax) in x/y order."""
    q = {
        "service": "WFS", "version": "2.0.0", "request": "GetFeature",
        "outputFormat": "application/json", "srsName": "EPSG:3301",
        "typeNames": layer, "count": str(count),
    }
    if bbox:
        # The GeoServer reads the EPSG:3301 bbox with the northing first.
        q["bbox"] = "%.2f,%.2f,%.2f,%.2f,urn:ogc:def:crs:EPSG::3301" % (
            bbox[1], bbox[0], bbox[3], bbox[2])
    if cql:
        q["cql_filter"] = cql
    url = f"{GEOSERVER}/{workspace_path}?" + urllib.parse.urlencode(q, safe=":,()'")
    data = _json(url, **kw)
    if not isinstance(data, dict) or "features" not in data:
        raise SourceError("unexpected WFS answer")
    return data["features"]


def _around(x, y, r):
    return (x - r, y - r, x + r, y + r)


# ----------------------------------------------------------------------------------------------
# (1) Cadastral parcel
# ----------------------------------------------------------------------------------------------
def _parcel_dict(feature):
    p = feature.get("properties", {})
    polys = [g for kind, g in _geoms(feature.get("geometry")) if kind == "poly"]
    if not polys:
        raise SourceError("parcel without a polygon")
    return {
        "cadastral_number": p.get("tunnus"),
        "area_m2": p.get("pindala"),
        "purpose": p.get("siht1"),
        "purposes": [p[k] for k in ("siht1", "siht2", "siht3") if p.get(k)],
        "address": p.get("l_aadress"),
        "ownership": p.get("omvorm"),
        "municipality": p.get("ov_nimi"),
        "polygon": max(polys, key=_polygon_area),
        "source": "Maa- ja Ruumiamet katastri WFS",
    }


@_never_raises
def plot_boundary(x, y, ehr_code=None, *, cache_dir=None, offline=False):
    """Cadastral parcel at (x, y), or the parcel of an EHR building when the point misses.

    Returns {cadastral_number, area_m2, purpose, purposes, address, ownership, municipality,
    polygon: [(x, y), ...], source} or None.
    """
    kw = {"cache_dir": cache_dir, "offline": offline}
    layer = "kataster:ky_kehtiv"
    feats = []
    if x is not None and y is not None:
        # In a CQL point the northing comes first.
        feats = _wfs(layer, cql="INTERSECTS(geom,POINT(%.2f %.2f))" % (y, x), count=5,
                     workspace_path="kataster/ows", **kw)
    if not feats and ehr_code:
        for num in ehr_cadastral_numbers(ehr_code, **kw):
            feats = _wfs(layer, cql="tunnus='%s'" % num.replace("'", ""), count=1,
                         workspace_path="kataster/ows", **kw)
            if feats:
                break
    if not feats:
        raise SourceError("no cadastral parcel at this point")
    return _parcel_dict(feats[0])


def ehr_cadastral_numbers(ehr_code, *, cache_dir=None, offline=False):
    """Cadastral numbers listed in the Building Register record. Raises SourceError."""
    url = ("https://livekluster.ehr.ee/api/building/v3/buildingdata?ehr_code="
           + urllib.parse.quote(str(ehr_code)))
    data = _json(url, cache_dir=cache_dir, offline=offline)
    out = []

    def walk(o):
        if isinstance(o, dict):
            for k, v in o.items():
                if k == "katastritunnus" and isinstance(v, str) and v not in out:
                    out.append(v)
                else:
                    walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)

    walk(data)
    if not out:
        raise SourceError("the EHR record lists no cadastral number")
    return out


# ----------------------------------------------------------------------------------------------
# (2) Terrain: lidar DTM 1 m, read as a window of a remote GeoTIFF
# ----------------------------------------------------------------------------------------------
def dtm_sheet(x, y):
    """Number of the 5 km x 5 km map sheet of the 1 m DTM that holds the point.

    Digits a b c d q: a = floor(y / 100 km) - 59, b = floor(x / 100 km) - 2,
    c = tens of km of y, d = tens of km of x, q = 1 SW, 2 SE, 3 NW, 4 NE inside the 10 km block.
    """
    a = int(y // 100000) - 59
    b = int(x // 100000) - 2
    c = int(y // 10000) % 10
    d = int(x // 10000) % 10
    q = 1 + (1 if x % 10000 >= 5000 else 0) + (2 if y % 10000 >= 5000 else 0)
    return f"{a}{b}{c}{d}{q}"


_TYPES = {1: ("B", 1), 2: ("s", 1), 3: ("H", 2), 4: ("I", 4), 5: ("II", 8), 11: ("f", 4),
          12: ("d", 8)}


class _RemoteTiff:
    """Tiled, little-endian, classic TIFF read with HTTP range requests. Float32 or 16 bit."""

    def __init__(self, url, **kw):
        self.url, self.kw = url, kw
        self.tiles = {}
        head = self._range(0, 32767)
        if head[:4] != b"II*\x00":
            raise SourceError("not a little-endian classic TIFF")
        ifd = struct.unpack("<I", head[4:8])[0]
        if ifd + 2 > len(head):
            head = self._range(0, ifd + 65535)
        n = struct.unpack("<H", head[ifd:ifd + 2])[0]
        tags = {}
        for i in range(n):
            tag, typ, cnt, val = struct.unpack("<HHI4s", head[ifd + 2 + 12 * i:ifd + 14 + 12 * i])
            fmt, size = _TYPES.get(typ, ("B", 1))
            total = size * cnt
            if total <= 4:
                raw = val[:total]
            else:
                off = struct.unpack("<I", val)[0]
                if off + total > len(head):
                    head = self._range(0, off + total + 4096)
                raw = head[off:off + total]
            if typ == 2:
                tags[tag] = raw.rstrip(b"\0").decode("latin-1")
            elif typ == 5:
                tags[tag] = struct.unpack("<%dI" % (2 * cnt), raw)
            else:
                tags[tag] = struct.unpack("<%d%s" % (cnt, fmt), raw)

        def g(t, d=None):
            v = tags.get(t)
            return d if v is None else (v if isinstance(v, str) else v[0])

        self.w, self.h = g(256), g(257)
        self.bits = g(258, 8)
        self.compression = g(259, 1)
        self.predictor = g(317, 1)
        self.fmt = g(339, 1)
        self.tw, self.th = g(322), g(323)
        if not self.tw or 324 not in tags:
            raise SourceError("the GeoTIFF is not tiled")
        if self.compression not in (8, 32946):
            raise SourceError("unsupported TIFF compression %s" % self.compression)
        if not (self.fmt == 3 and self.bits == 32):
            raise SourceError("only 32-bit float rasters are supported")
        self.offsets, self.counts = tags[324], tags[325]
        self.across = -(-self.w // self.tw)
        scale, tie = tags.get(33550), tags.get(33922)
        if not scale or not tie:
            raise SourceError("the GeoTIFF has no georeference")
        self.ps = scale[0]
        self.ox, self.oy = tie[3], tie[4]  # upper-left corner of the upper-left pixel
        keys = tags.get(34735, ())
        self.pixel_is_point = any(keys[i] == 1025 and keys[i + 3] == 2
                                  for i in range(4, len(keys) - 3, 4))
        nod = tags.get(42113)
        try:
            self.nodata = float(nod) if nod is not None else None
        except ValueError:
            self.nodata = None

    def _range(self, a, b):
        return _http(self.url, headers={"Range": "bytes=%d-%d" % (a, b)}, **self.kw)

    def tile(self, col, row):
        """Decoded tile as a list of rows (lists of floats)."""
        idx = row * self.across + col
        if idx in self.tiles:
            return self.tiles[idx]
        off, cnt = self.offsets[idx], self.counts[idx]
        if cnt == 0:
            raise SourceError("empty tile")
        raw = zlib.decompress(self._range(off, off + cnt - 1))
        tw, th = self.tw, self.th
        rows = []
        if self.predictor == 3:
            rb = tw * 4
            for r in range(th):
                acc = bytes(accumulate(raw[r * rb:(r + 1) * rb], lambda a, b: (a + b) & 255))
                out = bytearray(rb)
                out[0::4], out[1::4], out[2::4], out[3::4] = (
                    acc[0:tw], acc[tw:2 * tw], acc[2 * tw:3 * tw], acc[3 * tw:4 * tw])
                rows.append(list(struct.unpack(">%df" % tw, out)))
        elif self.predictor == 1:
            rows = [list(struct.unpack("<%df" % tw, raw[r * tw * 4:(r + 1) * tw * 4]))
                    for r in range(th)]
        else:
            raise SourceError("unsupported TIFF predictor %s" % self.predictor)
        self.tiles[idx] = rows
        return rows

    def pixel(self, c, r):
        c = min(max(c, 0), self.w - 1)
        r = min(max(r, 0), self.h - 1)
        v = self.tile(c // self.tw, r // self.th)[r % self.th][c % self.tw]
        if v != v or (self.nodata is not None and v == self.nodata) or v < -1000:
            return None
        return v

    def sample(self, x, y):
        """Bilinear height at an EPSG:3301 point. None over no-data."""
        half = 0.0 if self.pixel_is_point else 0.5
        fc = (x - self.ox) / self.ps - half
        fr = (self.oy - y) / self.ps - half
        c0, r0 = math.floor(fc), math.floor(fr)
        tc, tr = fc - c0, fr - r0
        vals = [self.pixel(c0, r0), self.pixel(c0 + 1, r0), self.pixel(c0, r0 + 1),
                self.pixel(c0 + 1, r0 + 1)]
        if None in vals:
            near = self.pixel(round(fc), round(fr))
            return near
        top = vals[0] * (1 - tc) + vals[1] * tc
        bot = vals[2] * (1 - tc) + vals[3] * tc
        return top * (1 - tr) + bot * tr


@_never_raises
def terrain_grid(x, y, half_size=20, step=2, *, cache_dir=None, offline=False):
    """Heights of the ground around (x, y) from the lidar DTM (1 m), bilinear at `step` metres.

    Returns {origin: (x0, y0), step, nx, ny, z: [[...]], nodata_count, sheets, source}.
    z[j][i] is the height in metres (EH2000 height system) at (x0 + i*step, y0 + j*step), so row 0
    is the south edge. A cell without data is None. The grid has (2*half_size/step + 1) points
    per side. Returns None when no cell has data.
    """
    kw = {"cache_dir": cache_dir, "offline": offline}
    n = int(round(2 * half_size / step)) + 1
    x0, y0 = x - half_size, y - half_size
    tiffs = {}
    z = []
    missing = 0
    for j in range(n):
        row = []
        for i in range(n):
            px, py = x0 + i * step, y0 + j * step
            sheet = dtm_sheet(px, py)
            if sheet not in tiffs:
                try:
                    tiffs[sheet] = _RemoteTiff(DTM_URL.format(sheet=sheet), **kw)
                except SourceError as exc:
                    if not tiffs:
                        raise SourceError("DTM sheet %s: %s" % (sheet, exc)) from exc
                    tiffs[sheet] = None
            t = tiffs[sheet]
            v = t.sample(px, py) if t else None
            if v is None:
                missing += 1
            row.append(None if v is None else round(v, 3))
        z.append(row)
    if missing == n * n:
        raise SourceError("the DTM has no data here")
    return {"origin": (x0, y0), "step": float(step), "nx": n, "ny": n, "z": z,
            "nodata_count": missing, "sheets": sorted(tiffs),
            "source": "Maa- ja Ruumiamet maapinna kõrgusmudel 1 m (lidar)"}


def contour_lines(grid, interval=0.5):
    """Contour lines of a terrain_grid() result by marching squares.

    Returns {level: [[(x, y), ...], ...]}. Segments are joined where the ends meet.
    """
    z, step = grid["z"], grid["step"]
    x0, y0 = grid["origin"]
    ny, nx = len(z), len(z[0])
    vals = [v for row in z for v in row if v is not None]
    if not vals:
        return {}
    lo = math.ceil(min(vals) / interval) * interval
    out = {}
    lvl = lo
    while lvl <= max(vals) + 1e-9:
        segs = []
        for j in range(ny - 1):
            for i in range(nx - 1):
                c = (z[j][i], z[j][i + 1], z[j + 1][i + 1], z[j + 1][i])
                if None in c:
                    continue
                pts = []
                corners = ((0, 0), (1, 0), (1, 1), (0, 1))
                for k in range(4):
                    a, b = c[k], c[(k + 1) % 4]
                    if (a >= lvl) != (b >= lvl):
                        t = (lvl - a) / (b - a)
                        (ax, ay), (bx, by) = corners[k], corners[(k + 1) % 4]
                        pts.append((x0 + (i + ax + t * (bx - ax)) * step,
                                    y0 + (j + ay + t * (by - ay)) * step))
                if len(pts) == 2:
                    segs.append((pts[0], pts[1]))
                elif len(pts) == 4:
                    segs.append((pts[0], pts[1]))
                    segs.append((pts[2], pts[3]))
        lines = _join(segs)
        if lines:
            out[round(lvl, 6)] = lines
        lvl += interval
    return out


def _join(segs):
    key = lambda p: (round(p[0], 3), round(p[1], 3))
    ends = {}
    for s in segs:
        for p in s:
            ends.setdefault(key(p), []).append(s)
    used, lines = set(), []
    for s in segs:
        if id(s) in used:
            continue
        used.add(id(s))
        line = [s[0], s[1]]
        for forward in (True, False):
            while True:
                tip = line[-1] if forward else line[0]
                nxt = next((t for t in ends.get(key(tip), []) if id(t) not in used), None)
                if nxt is None:
                    break
                used.add(id(nxt))
                other = nxt[1] if key(nxt[0]) == key(tip) else nxt[0]
                if forward:
                    line.append(other)
                else:
                    line.insert(0, other)
        lines.append(line)
    return lines


# ----------------------------------------------------------------------------------------------
# (3) Nearby objects: ETAK (official), OpenStreetMap as a fallback
# ----------------------------------------------------------------------------------------------
_ETAK = {
    "buildings": ["etak:e_401_hoone_ka"],
    "roads": ["etak:e_501_tee_j"],
    "trees": ["etak:e_305_puittaimestik_p", "etak:e_305_puittaimestik_j",
              "etak:e_305_puittaimestik_a"],
    "water": ["etak:e_203_vooluveekogu_a", "etak:e_203_vooluveekogu_j",
              "etak:e_202_seisuveekogu_a", "etak:e_202_seisuveekogu_p"],
    "fences": ["etak:e_405_piire_j"],
}


def _etak_item(group, layer, kind, geom, p):
    base = {"kind": layer.rsplit(":", 1)[1], "geometry": kind, "points": geom,
            "name": p.get("nimetus") or p.get("ads_nimetus") or p.get("nimi"),
            "source": "ETAK"}
    if group == "buildings":
        base.update(polygon=geom, height_m=p.get("korgus_m"), ehr_code=p.get("ehr_gid"),
                    address=p.get("ads_lahiaadress"), use=p.get("tyyp_tekst"))
    elif group == "roads":
        base.update(line=geom, width_m=p.get("laius"), surface=p.get("teekate_tekst"),
                    road_class=p.get("tyyp_tekst"))
    elif group == "trees":
        base.update(type=p.get("tyyp_tekst") or p.get("kood_tekst"))
        if kind == "point":
            base["point"] = geom[0]
    return base


def _etak_features(x, y, radius, kw):
    out = {k: [] for k in _ETAK}
    errors, ok = [], 0
    for group, layers in _ETAK.items():
        for layer in layers:
            try:
                feats = _wfs(layer, bbox=_around(x, y, radius), **kw)
            except SourceError as exc:
                errors.append("%s: %s" % (layer, exc))
                continue
            ok += 1
            for f in feats:
                for kind, geom in _geoms(f.get("geometry")):
                    d = distance_to(x, y, geom, closed=(kind == "poly"))
                    if d <= radius:
                        item = _etak_item(group, layer, kind, geom, f.get("properties", {}))
                        item["distance_m"] = round(d, 1)
                        out[group].append(item)
    if not ok:
        raise SourceError("; ".join(errors) or "no ETAK layer answered")
    out["errors"] = errors
    out["source"] = "ETAK"
    return out


def _overpass(lat, lon, radius, kw):
    q = ('[out:json][timeout:25];('
         'way["building"](around:{r},{a},{o});way["highway"](around:{r},{a},{o});'
         'node["natural"="tree"](around:{r},{a},{o});way["natural"="tree_row"](around:{r},{a},{o});'
         'way["natural"="water"](around:{r},{a},{o});way["waterway"](around:{r},{a},{o});'
         'way["barrier"~"fence|wall|hedge"](around:{r},{a},{o});'
         'way["landuse"](around:{r},{a},{o}););out geom tags;').format(r=int(radius), a=lat, o=lon)
    data = urllib.parse.urlencode({"data": q}).encode()
    last = None
    for url in OVERPASS_URLS:
        try:
            return json.loads(_http(url, data=data, timeout=60, **kw).decode("utf-8"))
        except SourceError as exc:
            last = exc
        except ValueError as exc:
            last = SourceError("Overpass answer is not JSON")
            _ = exc
    raise last or SourceError("Overpass failed")


def _osm_features(x, y, radius, kw):
    lat, lon = lest97_to_wgs84(x, y)
    data = _overpass(lat, lon, radius, kw)
    out = {k: [] for k in ("buildings", "roads", "trees", "water", "fences", "landuse")}
    for el in data.get("elements", []):
        tags = el.get("tags", {})
        if el["type"] == "node":
            pts = [wgs84_to_lest97(el["lat"], el["lon"])]
            kind = "point"
        else:
            pts = [wgs84_to_lest97(p["lat"], p["lon"]) for p in el.get("geometry", [])]
            kind = "poly" if len(pts) > 3 and pts[0] == pts[-1] else "line"
        if not pts:
            continue
        if kind == "poly":
            pts = pts[:-1]
        d = distance_to(x, y, pts, closed=(kind == "poly"))
        item = {"geometry": kind, "points": pts, "name": tags.get("name"), "tags": tags,
                "distance_m": round(d, 1), "source": "OpenStreetMap", "osm_id": el["id"]}
        if "building" in tags:
            h = tags.get("height")
            try:
                height = float(h) if h else (float(tags["building:levels"]) * 3.0
                                             if tags.get("building:levels") else None)
            except ValueError:
                height = None
            item.update(polygon=pts, height_m=height, kind=tags.get("building"))
            out["buildings"].append(item)
        elif "highway" in tags:
            item.update(line=pts, kind=tags["highway"], surface=tags.get("surface"))
            out["roads"].append(item)
        elif tags.get("natural") in ("tree", "tree_row"):
            item.update(kind=tags.get("natural"), species=tags.get("species") or tags.get("genus"))
            if kind == "point":
                item["point"] = pts[0]
            out["trees"].append(item)
        elif tags.get("natural") == "water" or "waterway" in tags:
            item["kind"] = tags.get("water") or tags.get("waterway") or "water"
            out["water"].append(item)
        elif "barrier" in tags:
            item["kind"] = tags["barrier"]
            out["fences"].append(item)
        elif "landuse" in tags:
            item["kind"] = tags["landuse"]
            out["landuse"].append(item)
    out["errors"] = []
    out["source"] = "OpenStreetMap"
    return out


@_never_raises
def nearby_features(x, y, radius=100, source="auto", *, cache_dir=None, offline=False):
    """Buildings, roads, trees, water and fences within `radius` m of (x, y).

    source "etak" uses the official Estonian Topographic Database, "osm" uses OpenStreetMap,
    "auto" tries ETAK first and OpenStreetMap when ETAK fails. Returns
    {buildings, roads, trees, water, fences, errors, source} (OSM adds landuse) or None.
    Every item has points (list of (x, y)), geometry ("poly", "line" or "point"), distance_m and
    source. Buildings add polygon, height_m, ehr_code (ETAK) and address. Roads add line, width_m.
    """
    kw = {"cache_dir": cache_dir, "offline": offline}
    if source in ("etak", "auto"):
        try:
            return _etak_features(x, y, radius, kw)
        except SourceError as exc:
            if source == "etak":
                raise
            log.info("ETAK failed (%s), trying OpenStreetMap", exc)
    return _osm_features(x, y, radius, kw)


# ----------------------------------------------------------------------------------------------
# (4) Environment flags: protected areas, heritage, flood risk
# ----------------------------------------------------------------------------------------------
# (layer, kind, name property, geometry name used in CQL, search radius in m)
_FLAG_LAYERS = [
    ("muinsuskaitse:kpo_malestised", "heritage_monument", "nimi", "geom", 300),
    ("muinsuskaitse:kpo_kaitsevoondid", "heritage_protection_zone", "nimi", "geom", 0),
    ("eelis:kr_kaitseala", "protected_area", "nimi", "shape", 300),
    ("eelis:kr_loodusala", "natura_habitat_area", "nimi", "shape", 300),
    ("eelis:kr_linnuala", "natura_bird_area", "nimi", "shape", 300),
    ("eelis:kr_kohalik_objekt", "local_protected_object", "nimi", "shape", 300),
    ("eelis:kr_vep", "key_habitat", "nimi", "shape", 300),
    ("eelis:kr_yleujutusohuga_ala", "flood_risk_area", "nimi", "shape", 0),
    ("keskkonnaportaal:yleujutusalad_1", "flood_100_year", "nimi", "shape", 0),
]


def _flag_name(p, default):
    for k in ("nimi", "voond_nimi", "nimetus", "name", "kr_kood", "tyyp_nimi"):
        if p.get(k):
            return str(p[k])
    return default


@_never_raises
def environment_flags(x, y, radius=200, *, cache_dir=None, offline=False):
    """Heritage, protected-area and flood-risk objects at or near (x, y).

    Returns a list of {kind, name, distance_m, source}, sorted by distance. The list is empty
    when nothing is near. Flood layers are only tested at the point (distance 0) because their
    polygons are very large. Returns None when every layer failed.
    """
    kw = {"cache_dir": cache_dir, "offline": offline}
    found, errors, ok = [], [], 0
    for layer, kind, _name, geomname, rad in _FLAG_LAYERS:
        try:
            if rad:
                feats = _wfs(layer, bbox=_around(x, y, min(radius, rad)), count=50, **kw)
            else:
                feats = _wfs(layer, cql="INTERSECTS(%s,POINT(%.2f %.2f))" % (geomname, y, x),
                             count=5, **kw)
        except SourceError as exc:
            errors.append("%s: %s" % (layer, exc))
            continue
        ok += 1
        for f in feats:
            p = f.get("properties", {})
            dist = min((distance_to(x, y, g, closed=(k == "poly"))
                        for k, g in _geoms(f.get("geometry"))), default=0.0)
            if rad and dist > min(radius, rad):
                continue
            found.append({"kind": kind, "name": _flag_name(p, kind), "distance_m": round(dist, 1),
                          "source": layer})
    if not ok:
        raise SourceError("; ".join(errors))
    found.sort(key=lambda d: d["distance_m"])
    return found


# ----------------------------------------------------------------------------------------------
# Soil type (bonus): Land Board soil map, WFS layer veeveeb:liht_mullakaart
# ----------------------------------------------------------------------------------------------
# Colour number (attribute "varv") -> soil group, from the Land Board colour code table.
SOIL_GROUPS = {
    1: "Leetunud mullad", 2: "Rähk- ja klibumullad",
    3: "Väga õhukesed ja õhukesed paepealsed mullad", 4: "Leostunud ja leetjad mullad",
    5: "Leetunud huumuslikud leedemullad", 6: "Kahkjad leetunud mullad",
    7: "Gleistunud kahkjad leetunud mullad", 8: "Gleistunud leetunud mullad",
    9: "Kahkjad leetunud gleimullad", 10: "Leede-turvastunud mullad",
    11: "Siirdesoo- ja rabamullad", 12: "Gleistunud rähk- ja klibumullad",
    13: "Gleistunud paepealsed mullad", 14: "Koreserikkad ja leostunud gleimullad",
    15: "Gleistunud lammimullad", 16: "Turvastunud mullad", 17: "Madalsoomullad",
    18: "Erodeeritud mullad", 19: "Deluviaalmullad",
}


@_never_raises
def soil_type(x, y, *, cache_dir=None, offline=False):
    """Soil map unit at (x, y) from the Land Board soil map (mullakaart, scale 1:10 000).

    Returns {group, group_code, attributes, source} or None. The map has no data in towns.
    """
    feats = _wfs("veeveeb:liht_mullakaart", bbox=_around(x, y, 1.0), count=3,
                 cache_dir=cache_dir, offline=offline)
    if not feats:
        raise SourceError("no soil map polygon at this point")
    p = feats[0].get("properties", {})
    code = p.get("varv")
    return {"group": SOIL_GROUPS.get(int(code)) if code is not None else None,
            "group_code": code, "attributes": p, "source": "Maa- ja Ruumiamet mullakaart"}
