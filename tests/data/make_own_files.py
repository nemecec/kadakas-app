"""Makes the small DXF files in tests/data/dxf-own/ (IOP-15).

The files are written here, from the DXF specification and the ezdxf library (MIT). They are our
own work, under the licence of this repository. They are small, so that the tests that read them
run offline in CI. No file of a CAD program and no file with an unclear licence is copied.

Run: uv run --with ezdxf python -I tests/data/make_own_files.py
"""

from pathlib import Path

import ezdxf
from ezdxf.render import MeshBuilder

OUT = Path(__file__).resolve().parent / "dxf-own"


def r12_unitless(path):
    doc = ezdxf.new("R12")
    doc.layers.add("Walls", color=1)
    doc.layers.add("Trees", color=3)
    msp = doc.modelspace()
    msp.add_line((0, 0), (10000, 0), dxfattribs={"layer": "Walls"})
    msp.add_line((10000, 0), (10000, 6000), dxfattribs={"layer": "Walls"})
    msp.add_circle((5000, 3000), 800, dxfattribs={"layer": "Trees"})
    msp.add_arc((0, 0), 2000, 0, 90, dxfattribs={"layer": "Walls"})
    pl = msp.add_polyline2d([], dxfattribs={"layer": "Walls"})
    pl.append_formatted_vertices([(0, 6000, 0, 0, 0), (4000, 6000, 0, 0, 0.5), (4000, 8000, 0, 0, 0)], format="xyseb")
    msp.add_text("Garden", dxfattribs={"insert": (500, 500), "height": 300, "layer": "Trees"})
    doc.saveas(path)
    return doc


def binary(path, version):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_line((0, 0), (1000, 0))
    msp.add_line((1000, 0), (1000, 500))
    msp.add_text("Binary", dxfattribs={"insert": (10, 10), "height": 50})
    with open(path, "wb") as fp:
        doc.write_binary(fp) if hasattr(doc, "write_binary") else doc.write(fp, fmt="bin")


def codepage(path, version, encoding, text, layer):
    doc = ezdxf.new(version)
    doc.encoding = encoding
    doc.header["$DWGCODEPAGE"] = {"cp1251": "ANSI_1251", "cp1257": "ANSI_1257", "cp1250": "ANSI_1250"}[encoding]
    doc.layers.add(layer)
    msp = doc.modelspace()
    msp.add_text(text, dxfattribs={"insert": (0, 0), "height": 100, "layer": layer})
    msp.add_line((0, 0), (1000, 0), dxfattribs={"layer": layer})
    doc.saveas(path, encoding=encoding)


def unicode_file(path):
    doc = ezdxf.new("R2018")
    doc.units = 4
    doc.layers.add("Põõsad ja puud")
    msp = doc.modelspace()
    msp.add_text("Õunapuu \u00d8300 \u00b1 5\u00b0", dxfattribs={"insert": (0, 0), "height": 100, "layer": "Põõsad ja puud"})
    msp.add_mtext("{\\fArial|b0;Ülemine}\\Palumine \\S1/2;", dxfattribs={"insert": (0, 500), "layer": "Põõsad ja puud"})
    msp.add_text("\u4e2d\u6587", dxfattribs={"insert": (0, 900), "height": 100})
    doc.saveas(path)


def meters(path):
    doc = ezdxf.new("R2018")
    doc.units = 6
    msp = doc.modelspace()
    msp.add_lwpolyline([(0, 0), (30, 0), (30, 20), (0, 20)], close=True)
    doc.saveas(path)


def feet(path):
    doc = ezdxf.new("R2018")
    doc.units = 2
    msp = doc.modelspace()
    msp.add_lwpolyline([(0, 0), (100, 0), (100, 60), (0, 60)], close=True)
    doc.saveas(path)


def layouts(path):
    doc = ezdxf.new("R2018", setup=True)
    doc.units = 6
    for name, color in (("Walls", 1), ("Trees", 3), ("Old", 8), ("Annotations", 7)):
        doc.layers.add(name, color=color)
    msp = doc.modelspace()
    msp.add_lwpolyline([(0, 0), (30, 0), (30, 20), (0, 20)], close=True, dxfattribs={"layer": "Walls"})
    msp.add_circle((10, 10), 1.5, dxfattribs={"layer": "Trees"})
    msp.add_line((0, 0), (30, 20), dxfattribs={"layer": "Old"})
    block = doc.blocks.new("TITLEBLOCK")
    block.add_lwpolyline([(0, 0), (90, 0), (90, 30), (0, 30)], close=True)
    block.add_text("Project", dxfattribs={"insert": (3, 20), "height": 3.5})
    block.add_attdef("SHEET", (3, 8), dxfattribs={"height": 5})
    one = doc.layouts.new("A101")
    one.page_setup(size=(420, 297), margins=(0, 0, 0, 0), units="mm")
    one.add_lwpolyline([(10, 10), (410, 10), (410, 287), (10, 287)], close=True, dxfattribs={"layer": "Annotations"})
    one.add_blockref("TITLEBLOCK", (320, 10)).add_auto_attribs({"SHEET": "A101"})
    vp = one.add_viewport(center=(160, 160), size=(280, 200), view_center_point=(15, 10), view_height=20)
    vp.frozen_layers = ["Old"]
    one.add_text("Plan 1:100", height=5, dxfattribs={"insert": (30, 20), "layer": "Annotations"})
    two = doc.layouts.new("A102")
    two.page_setup(size=(297, 420), margins=(0, 0, 0, 0), units="mm")
    two.add_text("Second sheet", height=6, dxfattribs={"insert": (20, 380)})
    doc.saveas(path)


def entities(path):
    doc = ezdxf.new("R2018", setup=True)
    doc.units = 4
    for name in ("Meshes", "Fills", "Notes", "Dims", "Curves", "Tilt"):
        doc.layers.add(name)
    msp = doc.modelspace()
    cube = MeshBuilder()
    cube.add_mesh(
        vertices=[(0, 0, 0), (1000, 0, 0), (1000, 1000, 0), (0, 1000, 0), (0, 0, 1000), (1000, 0, 1000), (1000, 1000, 1000), (0, 1000, 1000)],
        faces=[(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)],
    )
    cube.render_mesh(msp, dxfattribs={"layer": "Meshes"})
    hatch = msp.add_hatch(color=1, dxfattribs={"layer": "Fills"})
    hatch.paths.add_polyline_path([(0, 0), (4000, 0), (4000, 4000), (0, 4000)], is_closed=True)
    hatch.paths.add_polyline_path([(1000, 1000), (3000, 1000), (3000, 3000), (1000, 3000)], is_closed=True)
    wipe = msp.add_wipeout([(500, 500), (1500, 500), (1500, 1500), (500, 1500)])
    wipe.dxf.layer = "Fills"
    msp.add_solid([(0, 5000), (1000, 5000), (0, 6000), (1000, 6000)], dxfattribs={"layer": "Fills", "color": 3})
    msp.add_leader([(0, 7000), (1000, 8000), (2000, 8000)], dxfattribs={"layer": "Notes"})
    msp.add_linear_dim(base=(0, -1000), p1=(0, 0), p2=(3000, 0), dxfattribs={"layer": "Dims"}).render()
    msp.add_mtext("{\\fArial|b0;Bold}\\Pline %%c20 \\S1/2;", dxfattribs={"layer": "Notes", "insert": (0, 9000)})
    msp.add_spline(fit_points=[(0, 0), (1000, 1000), (2000, 0)], dxfattribs={"layer": "Curves"})
    msp.add_ellipse((0, 0), major_axis=(1000, 0), ratio=0.5, dxfattribs={"layer": "Curves"})
    msp.add_circle((0, 0, 0), 500, dxfattribs={"layer": "Tilt", "extrusion": (1, 0, 0)})
    image_def = doc.add_image_def(filename="photos/site.png", size_in_pixel=(100, 100))
    msp.add_image(image_def, insert=(0, 0), size_in_units=(1000, 1000), rotation=0)
    doc.saveas(path)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    r12_unitless(OUT / "r12_unitless.dxf")
    binary(OUT / "r12_binary.dxf", "R12")
    binary(OUT / "r2000_binary.dxf", "R2000")
    codepage(OUT / "r2000_cp1251.dxf", "R2000", "cp1251", "\u041f\u0440\u0438\u0432\u0435\u0442", "\u0421\u0442\u0435\u043d\u044b")
    codepage(OUT / "r2000_cp1257.dxf", "R2000", "cp1257", "\u00d5unapuu \u0160okolaad", "Puud")
    unicode_file(OUT / "r2018_unicode.dxf")
    meters(OUT / "r2018_meters.dxf")
    feet(OUT / "r2018_feet.dxf")
    layouts(OUT / "r2018_layouts.dxf")
    entities(OUT / "r2018_entities.dxf")


main()
