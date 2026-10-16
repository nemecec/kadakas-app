"""A drawing with one or more objects of every kind that the API can make. Tests use it to check
a whole output (SVG, DXF, IFC) for the class of bug that only a rare kind shows."""

SQUARE = [[0, 0], [4000, 0], [4000, 2500], [0, 2500]]


def moved(points, dx, dy):
    return [[x + dx, y + dy] for x, y in points]


def build_all_kinds(cad):
    """Fills `cad` and returns {kind: [ids]} of what it made."""
    made = {}

    def add(kind, ids):
        made.setdefault(kind, []).extend(ids if isinstance(ids, list) else [ids])

    def shape(kind, **fields):
        return cad.execute({"op": "add_object", "shape": {"kind": kind, **fields}})

    add("line", shape("line", a=[0, 0], b=[3000, 0]))
    add("circle", shape("circle", center=[500, 500], radius=300))
    add("arc", shape("arc", center=[0, 0], radius=500, start=0, end=1.57))
    add("polyline", shape("polyline", points=[[0, 0], [100, 100], [200, 0]], closed=False))
    add("text", shape("text", at=[0, 2000], text="A & B <tag>", height=100))
    add("hardscape", shape("hardscape", boundary=moved(SQUARE, 6000, 0), material="Setts"))
    add("landscape_area", shape("landscape_area", boundary=moved(SQUARE, 12000, 0), name="Meadow"))
    add("existing_tree", shape("existing_tree", at=[20000, 0], species="Oak", dbh=600, spread=9000, height=18000, condition="good"))
    for i, roof in enumerate(["flat", "gable", "hip"]):
        r = cad.result("massing.add", {"footprint": moved(SQUARE, 25000 + 6000 * i, 0), "height": 6000, "roof": roof})
        add("massing", r["id"])
    cad.result("hardscape_style.create", {"name": "Plaza granite", "material": "Granite", "thickness": 60})
    add("hardscape", cad.result("hardscape.add", {"boundary": moved(SQUARE, 6000, 4000), "style": "Plaza granite"})["id"])
    cad.result("plant_style.create", {"name": "Lavender", "id_code": "LA", "spread": 500, "price": 4})
    placed = cad.result("plants.place", {"mode": "single", "style": "Lavender", "at": [14000, 4000], "tag": True})
    add("plant", placed["created"])
    add("wall", cad.result("wall.add", {"path": [[0, 6000], [8000, 6000]], "top": {"mode": "terrain", "offset": 900}})["id"])
    add("irrigation", cad.result("irrigation.poc.add", {"at": [0, 9000], "static_pressure": 400, "available_flow": 100, "name": "Meter"})["id"])
    add("irrigation", cad.result("irrigation.controller.add", {"at": [1000, 9000], "name": "C1", "stations": 4})["id"])
    add("irrigation", cad.result("irrigation.line.add", {"points": [[0, 9000], [5000, 9000]], "line_type": "mainline", "size": "50", "material": "PVC"})["id"])
    add("irrigation", cad.result("irrigation.valve.add", {"at": [5000, 9000], "zone": 1, "kv": 6, "name": "V1", "controller": "C1", "station": 1})["id"])
    add("irrigation", cad.result("irrigation.line.add", {"points": [[5000, 9000], [9000, 9000]], "size": "32", "material": "PE"})["id"])
    add("irrigation", cad.result("irrigation.outlet.add", {"at": [9000, 9000], "model": "Spray-4.0"})["id"])
    locus = []
    for i in range(5):
        for j in range(5):
            locus += shape("locus3d", p=[40000 + i * 1000, j * 1000, i * 100])
    add("locus3d", locus)
    add("site_model", cad.result("site.create", {"source_ids": locus})["id"])
    return made
