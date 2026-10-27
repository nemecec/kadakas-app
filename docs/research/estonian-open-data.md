# Estonian open data for the address demo

Research date: 2026-10-27. All sources below need no key and no login.
We tested each one live with a request from Python. The example values in this note are invented.
The functions are in `examples/demo-address/sources_extra.py`. The tests use invented data only.

## Summary

| Need | Source | Verdict |
|---|---|---|
| Cadastral parcel | Land Board cadastre WFS | Works |
| Terrain heights | Land Board lidar DTM 1 m, GeoTIFF by map sheet | Works, read with range requests |
| Buildings, roads, trees, water, fences | ETAK WFS (Land Board) | Works |
| Same, fallback | OpenStreetMap through Overpass | Works, slow |
| Heritage, protected areas, flood risk | EELIS and heritage register WFS | Works |
| Soil | Land Board soil map WFS | Works in the countryside, no data in towns |
| Elevation WCS or point query | none found | Not available |

## Sources

| Source | URL | Format | CRS | Licence | Attribution text | Tested | Notes |
|---|---|---|---|---|---|---|---|
| Cadastre WFS, layer `kataster:ky_kehtiv` | `https://gsavalik.envir.ee/geoserver/kataster/ows` | GeoJSON, GML | EPSG:3301 | CC BY 4.0 (default of the service) | Maa- ja Ruumiamet, katastri andmed, date of the extract | yes | Fields: `tunnus`, `pindala`, `siht1` to `siht3`, `l_aadress`, `omvorm`. About 0.1 s. |
| Lidar DTM 1 m | `https://geoportaal.maaruum.ee/index.php?lang_id=1&plugin_act=otsing&kaardiruut=SHEET&andmetyyp=dem_1m_geotiff&dl=1&f=SHEET_dtm_1m.tif&page_id=614` | GeoTIFF, float32, tiled, deflate | EPSG:3301, heights EH2000 | Land Board open data licence (2025-01-01) | Kõrgusandmed 2021-2024, Maa- ja Ruumiamet | yes | 5000 x 5000 pixels, 84 MB per sheet. Range requests work. Needs a User-Agent header. |
| ETAK WFS, layers `etak:e_*` | `https://gsavalik.envir.ee/geoserver/ows` | GeoJSON, GML | EPSG:3301 | Land Board open data licence | Eesti topograafia andmekogu, Maa- ja Ruumiamet, date | yes | Buildings carry `korgus_m` and `ehr_gid`. About 0.1 s per layer. |
| Soil map WFS, layer `veeveeb:liht_mullakaart` | `https://gsavalik.envir.ee/geoserver/ows` | GeoJSON | EPSG:3301 | CC BY 4.0 (default of the service) | Maa- ja Ruumiamet, mullakaart | yes | Colour number in `varv`. No polygons in towns. |
| Heritage register WFS, `muinsuskaitse:kpo_malestised`, `kpo_kaitsevoondid` | `https://gsavalik.envir.ee/geoserver/ows` | GeoJSON | EPSG:3301 | CC BY 4.0 (default of the service) | Muinsuskaitseamet, kultuurimälestiste register | yes | Points or polygons. Protection zones are large polygons. |
| EELIS WFS, `eelis:kr_kaitseala`, `kr_loodusala`, `kr_linnuala`, `kr_vep`, `kr_yleujutusohuga_ala` | `https://gsavalik.envir.ee/geoserver/ows` | GeoJSON | EPSG:3301 | CC BY 4.0 | EELIS, Keskkonnaagentuur | yes | Flood polygons are large (up to 4 MB per box), so we test the point only. |
| Overpass API | `https://overpass-api.de/api/interpreter` | JSON | WGS84 | ODbL 1.0 | © OpenStreetMap contributors | yes | Needs a User-Agent header. 30 to 40 s for 100 m. |
| Cadastre numbers in EHR | `https://livekluster.ehr.ee/api/building/v3/buildingdata?ehr_code=CODE` | JSON | EPSG:3301 | public register data | Ehitisregister | yes | Field `katastritunnus` lists the parcel numbers. |

## Requests

Parcel at a point. In the CQL filter the northing comes first.

    GET https://gsavalik.envir.ee/geoserver/kataster/ows?service=WFS&version=2.0.0
        &request=GetFeature&typeNames=kataster:ky_kehtiv&outputFormat=application/json
        &srsName=EPSG:3301&cql_filter=INTERSECTS(geom,POINT(6498765 612345))

Objects in a box. The `bbox` parameter also lists the northing first.

    GET https://gsavalik.envir.ee/geoserver/ows?service=WFS&version=2.0.0&request=GetFeature
        &typeNames=etak:e_401_hoone_ka&outputFormat=application/json&srsName=EPSG:3301
        &bbox=6498665,612245,6498865,612445,urn:ogc:def:crs:EPSG::3301

ETAK layers used: `e_401_hoone_ka` (buildings), `e_501_tee_j` (roads), `e_305_puittaimestik_p/j/a`
(trees), `e_203_vooluveekogu_a/j` and `e_202_seisuveekogu_a/p` (water), `e_405_piire_j` (fences).

The GeoJSON answer lists x (east) first, then y (north). Only the filter parameters use the other order.

## Terrain

The Land Board publishes no WCS and no point service for heights in the public list.
Calls to `kaart.maaamet.ee/wcs/korgus` and `teenus.maaamet.ee/ows/dtm` fail.
The WMS layer `nDSM` (surface model) in `/wms/fotokaart` gives pictures only, not values.
The download is the one way to get values. We read a window of the GeoTIFF with HTTP range requests.

Facts about the file:

- The sheet is 5 km x 5 km with 1 m pixels. The origin is the upper-left corner of the sheet.
- Tiles are 512 x 512 pixels. Each tile is deflate compressed with the floating-point predictor (TIFF predictor 3).
- No-data value: -9999.
- A 40 m window needs one to four tiles. That is 0.1 to 0.6 MB. We measured 0.5 s in total.
- A second file per sheet, `SHEET_dem_1m_2017-2020.tif`, holds the older model.

Sheet number rule, found by reading the origins of seven sheets. Five digits `a b c d q`:

- `a` = floor(y / 100 km) - 59
- `b` = floor(x / 100 km) - 2
- `c` = tens of kilometres of y, `d` = tens of kilometres of x
- `q` = 1 for south-west, 2 for south-east, 3 for north-west, 4 for north-east inside the 10 km block

Example with the invented point x 612345, y 6498765: a = 5, b = 4, c = 9, d = 1, q = 3 (west, north). The sheet is 54913.

Other model files from the same page: DTM 5 m (`dem_5m_geotiff`), DTM 10 m (`dem_10m_geotiff`, also one 2 GB file for all of Estonia), DSM and nDSM 1 m and 5 m, slope, relief shading, ALS point cloud (`lidar_laz_*`, sheets of 1:2000).
The point cloud sheet rule differs (six digits). We did not use it.

## Limits and reliability

- No source publishes a rate limit. We send one request at a time and keep a local cache.
- The WFS services state that the data are informative and that the service has no availability guarantee.
- The geoportal sits behind Cloudflare. A request without a User-Agent header gets HTTP 403.
- Overpass is a shared service. A busy server answers slowly. The module tries a second server.
- Measured latency (one request at a time, from Estonia): parcel 0.1 s, terrain window 0.5 s, ETAK all layers 0.8 s, flags 1 s, soil 0.05 s, Overpass 35 s.

## Not available or not working

| Tried | Result |
|---|---|
| `kaart.maaamet.ee/wms/kataster`, `/wms/korgus`, `/wms/hybriid`, `/wms/mullakaart` | 403 or 404 |
| `kaart.maaamet.ee/wfs/kataster`, `/wfs/etak`, `/wcs/korgus` | 403 or 404 |
| `kaart.maaamet.ee/wms/alus`, layer `mullaraster`, GetFeatureInfo | Layer is not queryable |
| `teenus.maaamet.ee/ows/dtm` | MapServer error, map file missing |
| `gsavalik.envir.ee/geoserver/wcs` with a height coverage | The capabilities list no height coverage |

## Terms of use

- Land Board open data licence, 2025-01-01: free use, also commercial. Derived works are allowed.
  Name the licensor, the dataset and the age or extract date. Example: "Kõrgusandmed 2021-2024, Maa- ja Ruumiamet".
  Add the licence text or the link `https://geoportaal.maaruum.ee/avaandmete-litsents` when you pass the data on.
- WFS services of the Land Board and the Ministry of Climate: CC BY 4.0 unless a layer says otherwise.
  The service text asks for a reference to the data owner. See the layer metadata for the owner.
- OpenStreetMap: ODbL 1.0. Show "© OpenStreetMap contributors". A database made from the data stays under ODbL.
- The attribution rows are in `docs/attribution.md` and `docs/attribution-assets.tsv`.

## Privacy

Parcel, building and register answers describe real property and real people.
The cache folder holds raw answers. Never commit it. The demo cache path is git-ignored.
