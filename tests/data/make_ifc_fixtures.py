"""Writes tests/fixtures/ifc/all_kinds.ifc: one small hand-made IFC4 file with every entity type that the
IFC importer reads (IOP-29): CSG primitives and booleans, revolved and swept solids, profiles, advanced
breps (spline, spherical and toroidal surfaces, indexed poly curves, surface curves), openings, grids,
placements, networks, face sets, property and quantity sets, units. The numbers are small and the
geometry is not always a valid solid: the file feeds the tests of damaged files (`ifc_damage.rs`) and
the coverage of the importer. Run: python3 -I tests/data/make_ifc_fixtures.py
Standard library only. The file is our own work (Apache-2.0 OR MIT)."""

from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "fixtures" / "ifc" / "all_kinds.ifc"
lines = []
n = [0]


def e(name, *attrs):
    n[0] += 1
    lines.append(f"#{n[0]}={name}({','.join(attrs)});")
    return f"#{n[0]}"


def pt(*c):
    return e("IFCCARTESIANPOINT", "(" + ",".join(f"{float(x)}" for x in c) + ")")


def d(*c):
    return e("IFCDIRECTION", "(" + ",".join(f"{float(x)}" for x in c) + ")")


def ax3(at=(0, 0, 0), z=None, x=None):
    return e("IFCAXIS2PLACEMENT3D", pt(*at), z or "$", x or "$")


def ax2(at=(0, 0)):
    return e("IFCAXIS2PLACEMENT2D", pt(*at), "$")


def lst(*xs):
    return "(" + ",".join(xs) + ")"


# ---- units and context -----------------------------------------------------------------------
length = e("IFCSIUNIT", "*", ".LENGTHUNIT.", ".MILLI.", ".METRE.")
area = e("IFCSIUNIT", "*", ".AREAUNIT.", "$", ".SQUARE_METRE.")
volume = e("IFCSIUNIT", "*", ".VOLUMEUNIT.", "$", ".CUBIC_METRE.")
radian = e("IFCSIUNIT", "*", ".PLANEANGLEUNIT.", "$", ".RADIAN.")
exps = e("IFCDIMENSIONALEXPONENTS", "0", "0", "0", "0", "0", "0", "0")
measure = e("IFCMEASUREWITHUNIT", "IFCPLANEANGLEMEASURE(0.01745329252)", radian)
degree = e("IFCCONVERSIONBASEDUNIT", exps, ".PLANEANGLEUNIT.", "'DEGREE'", measure)
money = e("IFCMONETARYUNIT", ".USD.")
elem = e("IFCDERIVEDUNITELEMENT", length, "1")
derived = e("IFCDERIVEDUNIT", lst(elem), ".LINEARVELOCITYUNIT.", "$")
units = e("IFCUNITASSIGNMENT", lst(length, area, volume, degree, money, derived))
world = ax3()
context = e("IFCGEOMETRICREPRESENTATIONCONTEXT", "$", "'Model'", "3", "1.E-5", world, "$")
body = e("IFCGEOMETRICREPRESENTATIONSUBCONTEXT", "'Body'", "'Model'", "*", "*", "*", "*", context, "$", ".MODEL_VIEW.", "$")
project = e("IFCPROJECT", "'0000000000000000000001'", "$", "'All kinds'", "$", "$", "$", "$", lst(context), units)
site_place = e("IFCLOCALPLACEMENT", "$", world)
site = e("IFCSITE", "'0000000000000000000002'", "$", "'Site'", "$", "$", site_place, "$", "$", ".ELEMENT.", "$", "$", "$", "$", "$")
facility = e("IFCFACILITY", "'0000000000000000000003'", "$", "'Facility'", "$", "$", site_place, "$", "$", ".ELEMENT.")
part = e("IFCFACILITYPART", "'0000000000000000000004'", "$", "'Part'", "$", "$", site_place, "$", "$", ".ELEMENT.")
storey = e("IFCBUILDINGSTOREY", "'0000000000000000000005'", "$", "'Level'", "$", "$", site_place, "$", "$", ".ELEMENT.", "0.")
e("IFCRELAGGREGATES", "'0000000000000000000006'", "$", "$", "$", project, lst(site))
e("IFCRELAGGREGATES", "'0000000000000000000007'", "$", "$", "$", site, lst(facility, storey))
e("IFCRELAGGREGATES", "'0000000000000000000008'", "$", "$", "$", facility, lst(part))
contained = []
n_guid = [100]


def guid():
    n_guid[0] += 1
    return f"'{n_guid[0]:022d}'"


def product(cls, name, items, rep_type="SweptSolid", predefined="$", place=None, extra=()):
    rep = e("IFCSHAPEREPRESENTATION", body, "'Body'", f"'{rep_type}'", lst(*items))
    shape = e("IFCPRODUCTDEFINITIONSHAPE", "$", "$", lst(rep))
    p = e(cls, guid(), "$", f"'{name}'", "$", "$", place or site_place, shape, "$", predefined, *extra)
    contained.append(p)
    return p


def extrude(profile, depth=1000, at=(0, 0, 0)):
    return e("IFCEXTRUDEDAREASOLID", profile, ax3(at), d(0, 0, 1), str(float(depth)))


# ---- profiles ----------------------------------------------------------------------------------
rect = e("IFCRECTANGLEPROFILEDEF", ".AREA.", "$", ax2(), "400.", "300.")
rounded = e("IFCROUNDEDRECTANGLEPROFILEDEF", ".AREA.", "$", ax2(), "400.", "300.", "50.")
circle = e("IFCCIRCLEPROFILEDEF", ".AREA.", "$", ax2(), "200.")
hollow_circle = e("IFCCIRCLEHOLLOWPROFILEDEF", ".AREA.", "$", ax2(), "300.", "40.")
hollow_rect = e("IFCRECTANGLEHOLLOWPROFILEDEF", ".AREA.", "$", ax2(), "400.", "300.", "30.")
ellipse = e("IFCELLIPSEPROFILEDEF", ".AREA.", "$", ax2(), "300.", "200.")
ishape = e("IFCISHAPEPROFILEDEF", ".AREA.", "$", ax2(), "200.", "400.", "10.", "16.", "20.", "$", "$")
cshape = e("IFCCSHAPEPROFILEDEF", ".AREA.", "$", ax2(), "200.", "100.", "10.", "10.", "$", "$")
composite = e("IFCCOMPOSITEPROFILEDEF", ".AREA.", "$", lst(rect, circle), "$")
square = [pt(0, 0), pt(500, 0), pt(500, 500), pt(0, 500)]
hole = [pt(100, 100), pt(200, 100), pt(200, 200), pt(100, 200)]
outline = e("IFCPOLYLINE", lst(*square, square[0]))
inner = e("IFCPOLYLINE", lst(*hole, hole[0]))
arbitrary = e("IFCARBITRARYCLOSEDPROFILEDEF", ".AREA.", "$", outline)
with_voids = e("IFCARBITRARYPROFILEDEFWITHVOIDS", ".AREA.", "$", outline, lst(inner))
points2 = e("IFCCARTESIANPOINTLIST2D", "((0.,0.),(300.,0.),(400.,100.),(300.,200.),(0.,200.))")
poly2 = e("IFCINDEXEDPOLYCURVE", points2, lst("IFCLINEINDEX((1,2))", "IFCARCINDEX((2,3,4))", "IFCLINEINDEX((4,5,1))"), ".F.")
indexed_profile = e("IFCARBITRARYCLOSEDPROFILEDEF", ".AREA.", "$", poly2)
for k, prof in enumerate([rect, rounded, circle, hollow_circle, hollow_rect, ellipse, ishape, cshape, composite, arbitrary, with_voids, indexed_profile]):
    product("IFCBUILDINGELEMENTPROXY", f"Extrusion {k}", [extrude(prof, 800, (k * 1000, 0, 0))])

# ---- CSG ---------------------------------------------------------------------------------------
block = e("IFCBLOCK", ax3(), "2000.", "800.", "800.")
cyl = e("IFCRIGHTCIRCULARCYLINDER", ax3((1000, 400, -100)), "1000.", "200.")
sphere = e("IFCSPHERE", ax3((500, 0, 0)), "300.")
cone = e("IFCRIGHTCIRCULARCONE", ax3((2000, 0, 0)), "600.", "200.")
pyramid = e("IFCRECTANGULARPYRAMID", ax3((3000, 0, 0)), "300.", "200.", "450.")
for k, prim in enumerate([block, sphere, cone, pyramid]):
    product("IFCBUILDINGELEMENTPROXY", f"CSG primitive {k}", [e("IFCCSGSOLID", prim)], "CSG")
for op in ("DIFFERENCE", "UNION", "INTERSECTION"):
    tree = e("IFCBOOLEANRESULT", f".{op}.", block, cyl)
    product("IFCBUILDINGELEMENTPROXY", f"CSG {op}", [e("IFCCSGSOLID", tree)], "CSG")
slab_solid = extrude(rect, 300)
plane = e("IFCPLANE", ax3((0, 0, 150)))
half = e("IFCHALFSPACESOLID", plane, ".F.")
clip = e("IFCBOOLEANCLIPPINGRESULT", ".DIFFERENCE.", slab_solid, half)
product("IFCSLAB", "Clipped", [clip], "Clipping", ".FLOOR.")
square2 = [pt(-50, -50), pt(50, -50), pt(50, 50), pt(-50, 50)]
bounded = e("IFCPOLYGONALBOUNDEDHALFSPACE", plane, ".T.", ax3(), e("IFCPOLYLINE", lst(*square2, square2[0])))
clip2 = e("IFCBOOLEANCLIPPINGRESULT", ".DIFFERENCE.", extrude(rect, 300), bounded)
product("IFCSLAB", "Bounded clip", [clip2], "Clipping", ".FLOOR.")
# ---- revolved and swept ------------------------------------------------------------------------
offset = e("IFCRECTANGLEPROFILEDEF", ".AREA.", "$", ax2((250, 25)), "100.", "50.")
axis1 = e("IFCAXIS1PLACEMENT", pt(0, 0, 0), d(0, 1, 0))
product("IFCBUILDINGELEMENTPROXY", "Revolved", [e("IFCREVOLVEDAREASOLID", offset, ax3(), axis1, "90.")], "SweptSolid")
path = e("IFCPOLYLINE", lst(pt(0, 0, 0), pt(1000, 0, 0), pt(1000, 800, 600)))
product("IFCBUILDINGELEMENTPROXY", "Pipe", [e("IFCSWEPTDISKSOLID", path, "25.", "$", "$", "$")], "AdvancedSweptSolid")
product("IFCBUILDINGELEMENTPROXY", "Hollow pipe", [e("IFCSWEPTDISKSOLID", path, "30.", "20.", "0.", "1.")], "AdvancedSweptSolid")
product("IFCBUILDINGELEMENTPROXY", "Polygonal pipe", [e("IFCSWEPTDISKSOLIDPOLYGONAL", path, "25.", "$", "$", "$", "10.")], "AdvancedSweptSolid")
circ = e("IFCCIRCLE", ax2(), "300.")
trimmed = e("IFCTRIMMEDCURVE", circ, lst("IFCPARAMETERVALUE(0.)"), lst("IFCPARAMETERVALUE(90.)"), ".T.", ".PARAMETER.")
product("IFCBUILDINGELEMENTPROXY", "Arc pipe", [e("IFCSWEPTDISKSOLID", trimmed, "25.", "$", "$", "$")], "AdvancedSweptSolid")

# ---- advanced breps ---------------------------------------------------------------------------
def face_on(surface, curve, a, b):
    va, vb = e("IFCVERTEXPOINT", a), e("IFCVERTEXPOINT", b)
    edge = e("IFCEDGECURVE", va, vb, curve, ".T.")
    closing = e("IFCEDGECURVE", vb, va, e("IFCLINE", b, e("IFCVECTOR", d(-1, 0, 0), "1.")), ".T.")
    loop = e("IFCEDGELOOP", lst(e("IFCORIENTEDEDGE", "*", "*", edge, ".T."), e("IFCORIENTEDEDGE", "*", "*", closing, ".T.")))
    return e("IFCADVANCEDFACE", lst(e("IFCFACEOUTERBOUND", loop, ".T.")), surface, ".T.")


p00, p10, p01, p11 = pt(0, 0, 0), pt(100, 0, 0), pt(0, 100, 0), pt(100, 100, 50)
spline_surface = e("IFCBSPLINESURFACEWITHKNOTS", "1", "1", lst(lst(p00, p01), lst(p10, p11)), ".UNSPECIFIED.", ".F.", ".F.", ".U.", "(2,2)", "(2,2)", "(0.,1.)", "(0.,1.)", ".UNSPECIFIED.")
rational_surface = e("IFCRATIONALBSPLINESURFACEWITHKNOTS", "1", "1", lst(lst(p00, p01), lst(p10, p11)), ".UNSPECIFIED.", ".F.", ".F.", ".U.", "(2,2)", "(2,2)", "(0.,1.)", "(0.,1.)", ".UNSPECIFIED.", "((1.,1.),(1.,1.))")
sphere_surface = e("IFCSPHERICALSURFACE", ax3(), "100.")
torus_surface = e("IFCTOROIDALSURFACE", ax3(), "100.", "20.")
cyl_surface = e("IFCCYLINDRICALSURFACE", ax3(), "100.")
spline_curve = e("IFCBSPLINECURVEWITHKNOTS", "2", lst(p00, p10, p11), ".UNSPECIFIED.", ".F.", ".U.", "(3,3)", "(0.,1.)", ".UNSPECIFIED.")
rational_curve = e("IFCRATIONALBSPLINECURVEWITHKNOTS", "2", lst(p00, p10, p11), ".UNSPECIFIED.", ".F.", ".U.", "(3,3)", "(0.,1.)", ".UNSPECIFIED.", "(1.,2.,1.)")
ellipse_curve = e("IFCELLIPSE", ax3(), "100.", "50.")
points3 = e("IFCCARTESIANPOINTLIST3D", "((0.,0.,0.),(50.,50.,0.),(100.,0.,0.),(0.,0.,0.))")
indexed = e("IFCINDEXEDPOLYCURVE", points3, lst("IFCARCINDEX((1,2,3))", "IFCLINEINDEX((3,4))"), ".F.")
pcurve = e("IFCPCURVE", spline_surface, e("IFCLINE", pt(0, 0), e("IFCVECTOR", d(1, 0), "1.")))
surface_curve = e("IFCSURFACECURVE", spline_curve, lst(pcurve), ".CURVE3D.")
intersection = e("IFCINTERSECTIONCURVE", spline_curve, lst(pcurve), ".CURVE3D.")
seam = e("IFCSEAMCURVE", spline_curve, lst(pcurve), ".CURVE3D.")
bounded_surface_curve = e("IFCBOUNDEDSURFACECURVE", spline_curve, lst(pcurve), ".CURVE3D.")
composite_curve = e("IFCCOMPOSITECURVE", lst(e("IFCCOMPOSITECURVESEGMENT", ".CONTINUOUS.", ".T.", spline_curve)), ".F.")
faces = []
for k, (surface, curve) in enumerate([
    (spline_surface, spline_curve), (rational_surface, rational_curve), (sphere_surface, ellipse_curve),
    (torus_surface, indexed), (cyl_surface, surface_curve), (spline_surface, intersection),
    (spline_surface, seam), (spline_surface, bounded_surface_curve), (spline_surface, composite_curve),
    (spline_surface, rational_curve),
]):
    faces.append(face_on(surface, curve, pt(0, 0, k), pt(100, 0, k)))
shell = e("IFCCLOSEDSHELL", lst(*faces))
void_shell = e("IFCCLOSEDSHELL", lst(faces[0]))
product("IFCBUILDINGELEMENTPROXY", "Advanced brep", [e("IFCADVANCEDBREP", shell)], "AdvancedBrep")
product("IFCBUILDINGELEMENTPROXY", "Advanced brep with voids", [e("IFCADVANCEDBREPWITHVOIDS", shell, lst(void_shell))], "AdvancedBrep")
product("IFCBUILDINGELEMENTPROXY", "Shell model", [e("IFCSHELLBASEDSURFACEMODEL", lst(e("IFCOPENSHELL", lst(*faces[:3]))))], "SurfaceModel")
product("IFCBUILDINGELEMENTPROXY", "Face model", [e("IFCFACEBASEDSURFACEMODEL", lst(e("IFCCONNECTEDFACESET", lst(*faces[:3]))))], "SurfaceModel")
# faceted breps with polygon loops
def tri_face(a, b, c):
    return e("IFCFACE", lst(e("IFCFACEOUTERBOUND", e("IFCPOLYLOOP", lst(a, b, c)), ".T.")))


a, b, c, t = pt(0, 0, 0), pt(100, 0, 0), pt(0, 100, 0), pt(0, 0, 100)
faceted = e("IFCCLOSEDSHELL", lst(tri_face(a, c, b), tri_face(a, b, t), tri_face(b, c, t), tri_face(c, a, t)))
product("IFCBUILDINGELEMENTPROXY", "Faceted brep", [e("IFCFACETEDBREP", faceted)], "Brep")
product("IFCBUILDINGELEMENTPROXY", "Faceted brep with voids", [e("IFCFACETEDBREPWITHVOIDS", faceted, lst(e("IFCCLOSEDSHELL", lst(tri_face(a, c, b)))))], "Brep")

# ---- face sets and networks ---------------------------------------------------------------------
coords = e("IFCCARTESIANPOINTLIST3D", "((0.,0.,0.),(1000.,0.,100.),(1000.,1000.,200.),(0.,1000.,100.),(500.,500.,300.))")
tri = e("IFCTRIANGULATEDFACESET", coords, "$", ".F.", "((1,2,5),(2,3,5),(3,4,5),(4,1,5))", "$")
product("IFCGEOGRAPHICELEMENT", "Terrain", [tri], "Tessellation", ".TERRAIN.")
net = e("IFCTRIANGULATEDIRREGULARNETWORK", coords, "$", ".F.", "((1,2,5),(2,3,5),(3,4,5),(4,1,5))", "(0,-1,7,-2)")
product("IFCGEOGRAPHICELEMENT", "Network", [net], "Tessellation", ".TERRAIN.")
poly_faces = e("IFCINDEXEDPOLYGONALFACE", "(1,2,3)")
voids = e("IFCINDEXEDPOLYGONALFACEWITHVOIDS", "(1,2,3,4)", "((1,2,5))")
product("IFCBUILDINGELEMENTPROXY", "Polygonal set", [e("IFCPOLYGONALFACESET", coords, "$", ".F.", lst(poly_faces, voids), "$")], "Tessellation")
product("IFCBUILDINGELEMENTPROXY", "Box", [e("IFCBOUNDINGBOX", pt(0, 0, 0), "100.", "200.", "300.")], "BoundingBox")
product("IFCBUILDINGELEMENTPROXY", "Curve set", [e("IFCGEOMETRICCURVESET", lst(path, circ))], "GeometricCurveSet")
product("IFCBUILDINGELEMENTPROXY", "Set", [e("IFCGEOMETRICSET", lst(pt(1, 2, 3)))], "GeometricSet")

# ---- mapped items --------------------------------------------------------------------------------
source = e("IFCSHAPEREPRESENTATION", body, "'Body'", "'SweptSolid'", lst(extrude(rect, 100)))
rmap = e("IFCREPRESENTATIONMAP", ax3(), source)
op3 = e("IFCCARTESIANTRANSFORMATIONOPERATOR3D", "$", "$", pt(0, 2000, 0), "1.", "$")
op2 = e("IFCCARTESIANTRANSFORMATIONOPERATOR2D", "$", "$", pt(0, 3000), "1.")
for k, op in enumerate([op3, op2]):
    product("IFCBUILDINGELEMENTPROXY", f"Mapped {k}", [e("IFCMAPPEDITEM", rmap, op)], "MappedRepresentation")

# ---- openings, grids, placements, helpers ----------------------------------------------------------
wall = product("IFCWALL", "Wall", [extrude(e("IFCRECTANGLEPROFILEDEF", ".AREA.", "$", ax2(), "4000.", "200."), 2800, (0, 5000, 0))], predefined=".STANDARD.")
hole_solid = e("IFCEXTRUDEDAREASOLID", e("IFCRECTANGLEPROFILEDEF", ".AREA.", "$", ax2(), "1000.", "300."), ax3((0, 5000, 100)), d(0, 0, 1), "2000.")
opening = product("IFCOPENINGSTANDARDCASE", "Opening", [hole_solid], predefined=".OPENING.")
e("IFCRELVOIDSELEMENT", guid(), "$", "$", "$", wall, opening)
product("IFCSPACE", "Space", [extrude(rect, 100)])
product("IFCEXTERNALSPATIALELEMENT", "External", [extrude(rect, 100)])
product("IFCVIRTUALELEMENT", "Virtual", [extrude(rect, 100)])
ua = e("IFCGRIDAXIS", "'A'", e("IFCPOLYLINE", lst(pt(0, 0), pt(0, 1000))), ".T.")
ub = e("IFCGRIDAXIS", "'B'", e("IFCPOLYLINE", lst(pt(1000, 0), pt(1000, 1000))), ".T.")
va = e("IFCGRIDAXIS", "'1'", e("IFCPOLYLINE", lst(pt(0, 0), pt(1000, 0))), ".T.")
vb = e("IFCGRIDAXIS", "'2'", e("IFCLINE", pt(0, 1000), e("IFCVECTOR", d(1, 0), "1.")), ".T.")
grid_place = site_place
grid = e("IFCGRID", guid(), "$", "'Grid'", "$", "$", grid_place, "$", "$", lst(ua, ub), lst(va, vb), "$", "$")
contained.append(grid)
inter = e("IFCVIRTUALGRIDINTERSECTION", lst(ub, vb), "(10.,20.,0.)")
other = e("IFCVIRTUALGRIDINTERSECTION", lst(ua, va), "(0.,0.,0.)")
gp = e("IFCGRIDPLACEMENT", inter, other)
product("IFCCOLUMN", "On the grid", [extrude(rect, 3000)], place=gp)
lp = e("IFCLINEARPLACEMENT", "$", "$", "$")
product("IFCBUILDINGELEMENTPROXY", "Linear", [extrude(rect, 100)], place=lp)
e("IFCRELCONTAINEDINSPATIALSTRUCTURE", guid(), "$", "$", "$", lst(*contained), site)

# ---- georeference, layers, styles, properties ---------------------------------------------------------
crs = e("IFCPROJECTEDCRS", "'EPSG:3067'", "$", "$", "$", "$", "$", e("IFCSIUNIT", "*", ".LENGTHUNIT.", "$", ".METRE."))
e("IFCMAPCONVERSIONSCALED", context, crs, "385000.", "6672000.", "0.", "1.", "0.", "0.001", "1.0002", "1.0003", "1.")
layer = e("IFCPRESENTATIONLAYERWITHSTYLE", "'Layer'", "$", lst(rect), "$", ".T.", ".F.", ".F.", ".F.", "$")
shading = e("IFCSURFACESTYLESHADING", e("IFCCOLOURRGB", "$", "1.", "0.", "0."), "0.")
style = e("IFCSURFACESTYLE", "$", ".BOTH.", lst(shading))
e("IFCSTYLEDITEM", rect, lst(e("IFCPRESENTATIONSTYLEASSIGNMENT", lst(style))), "$")
props = [
    e("IFCPROPERTYSINGLEVALUE", "'Label'", "$", "IFCLABEL('x')", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Text'", "$", "IFCTEXT('long')", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Id'", "$", "IFCIDENTIFIER('i-1')", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Int'", "$", "IFCINTEGER(3)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Logical'", "$", "IFCLOGICAL(.U.)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Bool'", "$", "IFCBOOLEAN(.T.)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Length'", "$", "IFCLENGTHMEASURE(1200.)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'PLength'", "$", "IFCPOSITIVELENGTHMEASURE(1200.)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'NLength'", "$", "IFCNONNEGATIVELENGTHMEASURE(1200.)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Area'", "$", "IFCAREAMEASURE(2.5)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Volume'", "$", "IFCVOLUMEMEASURE(0.5)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Angle'", "$", "IFCPLANEANGLEMEASURE(1.5)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'PAngle'", "$", "IFCPOSITIVEPLANEANGLEMEASURE(1.5)", "$"),
    e("IFCPROPERTYSINGLEVALUE", "'Description'", "$", "IFCDESCRIPTIVEMEASURE('d')", "$"),
    e("IFCPROPERTYBOUNDEDVALUE", "'Bounded'", "$", "IFCLENGTHMEASURE(2.)", "IFCLENGTHMEASURE(1.)", "$", "$"),
    e("IFCPROPERTYENUMERATEDVALUE", "'Enum'", "$", "(IFCLABEL('a'),IFCLABEL('b'))", "$"),
    e("IFCPROPERTYLISTVALUE", "'List'", "$", "(IFCINTEGER(1),IFCINTEGER(2))", "$"),
]
complex_prop = e("IFCCOMPLEXPROPERTY", "'Complex'", "$", "'Usage'", lst(props[0], props[3]))
pset = e("IFCPROPERTYSET", guid(), "$", "'Pset_All'", "$", lst(*props, complex_prop))
quantities = [
    e("IFCQUANTITYLENGTH", "'L'", "$", "$", "1.", "$"), e("IFCQUANTITYAREA", "'A'", "$", "$", "2.", "$"),
    e("IFCQUANTITYVOLUME", "'V'", "$", "$", "3.", "$"), e("IFCQUANTITYCOUNT", "'C'", "$", "$", "4.", "$"),
    e("IFCQUANTITYWEIGHT", "'W'", "$", "$", "5.", "$"), e("IFCQUANTITYTIME", "'T'", "$", "$", "6.", "$"),
]
qset = e("IFCELEMENTQUANTITY", guid(), "$", "'Qto_All'", "$", "$", lst(*quantities))
e("IFCRELDEFINESBYPROPERTIES", guid(), "$", "$", "$", lst(wall), pset)
e("IFCRELDEFINESBYPROPERTIES", guid(), "$", "$", "$", lst(wall), qset)
wall_type = e("IFCWALLTYPE", guid(), "$", "'Type'", "$", "$", lst(pset), "$", "$", "$", ".STANDARD.")
e("IFCRELDEFINESBYTYPE", guid(), "$", "$", "$", lst(wall), wall_type)

DATA = "\n".join(lines)
HEAD = """ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');
FILE_NAME('all_kinds.ifc','2026-01-01T00:00:00',(''),(''),'Kadakas tests','make_ifc_fixtures.py','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
"""
OUT.write_text(HEAD + DATA + "\nENDSEC;\nEND-ISO-10303-21;\n", encoding="utf-8", newline="\n")
print(f"{OUT.name}: {len(lines)} entities")
