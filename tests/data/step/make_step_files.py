#!/usr/bin/env python3
"""Writes the small STEP test files of IOP-11 (own work, written from public knowledge of the
format). Run: python3 -I make_step_files.py [output folder]. The files are checked in.
The Rust tests (crates/kadakas-step/tests) read them and compare volumes and areas with the
analytic values noted here."""
import math
import sys
from pathlib import Path


def num(x):
    s = repr(float(x))
    if "e" in s or "E" in s:
        m, e = s.lower().split("e")
        if "." not in m:
            m += "."
        return f"{m}E{int(e)}"
    return s if "." in s else s + "."


def vec(v):
    return "(" + ",".join(num(c) for c in v) + ")"


def norm(v):
    l = math.sqrt(sum(c * c for c in v))
    return tuple(c / l for c in v)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


class Step:
    """One file. `unit` is a tuple (kind, ...) of the length unit."""

    def __init__(self, schema="AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }", unit="mm"):
        self.schema = schema
        self.lines = []
        self.n = 0
        self.unit = unit
        self.ctx = None
        self.pts = {}

    def add(self, text):
        self.n += 1
        self.lines.append(f"#{self.n}={text};")
        return self.n

    def raw(self, line):
        self.lines.append(line)

    def point(self, p):
        key = tuple(p)
        if key not in self.pts:
            self.pts[key] = self.add(f"CARTESIAN_POINT('',{vec(p)})")
        return self.pts[key]

    def direction(self, d):
        return self.add(f"DIRECTION('',{vec(norm(d))})")

    def axis(self, o=(0, 0, 0), z=(0, 0, 1), x=(1, 0, 0)):
        return self.add(f"AXIS2_PLACEMENT_3D('',#{self.point(o)},#{self.direction(z)},#{self.direction(x)})")

    def vertex(self, p):
        return self.add(f"VERTEX_POINT('',#{self.point(p)})")

    def line(self, a, b):
        d = sub(b, a)
        l = math.sqrt(sum(c * c for c in d))
        v = self.add(f"VECTOR('',#{self.direction(d)},{num(l)})")
        return self.add(f"LINE('',#{self.point(a)},#{v})")

    def circle(self, ax, r):
        return self.add(f"CIRCLE('',#{ax},{num(r)})")

    def edge(self, v1, v2, curve, same=True):
        return self.add(f"EDGE_CURVE('',#{v1},#{v2},#{curve},{'.T.' if same else '.F.'})")

    def oriented(self, edge, fwd=True):
        return self.add(f"ORIENTED_EDGE('',*,*,#{edge},{'.T.' if fwd else '.F.'})")

    def loop(self, oriented):
        return self.add("EDGE_LOOP('',(" + ",".join(f"#{o}" for o in oriented) + "))")

    def bound(self, loop, outer=True, ori=True):
        name = "FACE_OUTER_BOUND" if outer else "FACE_BOUND"
        return self.add(f"{name}('',#{loop},{'.T.' if ori else '.F.'})")

    def face(self, bounds, surface, same=True):
        return self.add(
            "ADVANCED_FACE('',(" + ",".join(f"#{b}" for b in bounds) + f"),#{surface},{'.T.' if same else '.F.'})"
        )

    def plane(self, o, n, x):
        return self.add(f"PLANE('',#{self.axis(o, n, x)})")

    def shell(self, faces, closed=True):
        kind = "CLOSED_SHELL" if closed else "OPEN_SHELL"
        return self.add(f"{kind}('',(" + ",".join(f"#{f}" for f in faces) + "))")

    def solid(self, shell, name="solid"):
        return self.add(f"MANIFOLD_SOLID_BREP('{name}',#{shell})")

    # Whole bodies.

    def poly_face_edges(self, verts, cache):
        """Edges of a polygon of vertex points; cache maps point pairs to (edge, start)."""
        out = []
        n = len(verts)
        for i in range(n):
            a, b = verts[i], verts[(i + 1) % n]
            if (b, a) in cache:
                e, start = cache[(b, a)]
                out.append(self.oriented(e, False))
            else:
                va, vb = self.vertex_of(a), self.vertex_of(b)
                e = self.edge(va, vb, self.line(a, b))
                cache[(a, b)] = (e, a)
                out.append(self.oriented(e, True))
        return out

    def vertex_of(self, p):
        key = ("v", tuple(p))
        if key not in self.pts:
            self.pts[key] = self.vertex(p)
        return self.pts[key]

    def polyhedron(self, verts, faces, name="solid", bad_face=None):
        """Faces are lists of corner indices, counterclockwise seen from outside."""
        cache = {}
        fs = []
        for k, f in enumerate(faces):
            corners = [tuple(verts[i]) for i in f]
            nrm = (0, 0, 0)
            for i in range(len(corners)):
                c = cross(corners[i], corners[(i + 1) % len(corners)])
                nrm = tuple(x + y for x, y in zip(nrm, c))
            x = sub(corners[1], corners[0])
            surf = self.plane(corners[0], nrm, x)
            if k == bad_face:
                # A surface type that is not read: this face is skipped.
                surf = self.add(f"OFFSET_SURFACE('',#{surf},1.,.F.)")
            loop = self.loop(self.poly_face_edges(corners, cache))
            fs.append(self.face([self.bound(loop)], surf))
        return self.solid(self.shell(fs), name)

    def box(self, w, d, h, at=(0, 0, 0), name="box", bad_face=None):
        x0, y0, z0 = at
        v = [(x0 + (i & 1) * w, y0 + ((i >> 1) & 1) * d, z0 + ((i >> 2) & 1) * h) for i in range(8)]
        faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
        return self.polyhedron(v, faces, name, bad_face)

    def cylinder(self, r, h, at=(0, 0, 0), name="cylinder", hole_in=None):
        """A cylinder along Z. Returns the solid."""
        x0, y0, z0 = at
        p0, p1 = (x0 + r, y0, z0), (x0 + r, y0, z0 + h)
        v0, v1 = self.vertex(p0), self.vertex(p1)
        a0 = self.axis((x0, y0, z0), (0, 0, 1), (1, 0, 0))
        a1 = self.axis((x0, y0, z0 + h), (0, 0, 1), (1, 0, 0))
        c0, c1 = self.circle(a0, r), self.circle(a1, r)
        e0, e1 = self.edge(v0, v0, c0), self.edge(v1, v1, c1)
        es = self.edge(v0, v1, self.line(p0, p1))
        side_surf = self.add(f"CYLINDRICAL_SURFACE('',#{a0},{num(r)})")
        side = self.face(
            [self.bound(self.loop([self.oriented(e0), self.oriented(es), self.oriented(e1, False), self.oriented(es, False)]))],
            side_surf,
        )
        top = self.face([self.bound(self.loop([self.oriented(e1)]))], self.add(f"PLANE('',#{a1})"))
        bot_ax = self.axis((x0, y0, z0), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(e0, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, top, bot]), name)

    def plate_with_hole(self, w, d, h, r, name="plate"):
        """A box with a round hole through the middle."""
        cx, cy = w / 2, d / 2
        v = [((i & 1) * w, ((i >> 1) & 1) * d, ((i >> 2) & 1) * h) for i in range(8)]
        cache = {}
        faces = []
        # Four side faces.
        for f in [[0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]:
            corners = [v[i] for i in f]
            x = sub(corners[1], corners[0])
            nrm = cross(x, sub(corners[2], corners[1]))
            surf = self.plane(corners[0], nrm, x)
            faces.append(self.face([self.bound(self.loop(self.poly_face_edges(corners, cache)))], surf))
        # The hole.
        pb, pt = (cx + r, cy, 0.0), (cx + r, cy, h)
        vb, vt = self.vertex(pb), self.vertex(pt)
        ab, at_ = self.axis((cx, cy, 0.0)), self.axis((cx, cy, h))
        eb, et = self.edge(vb, vb, self.circle(ab, r)), self.edge(vt, vt, self.circle(at_, r))
        es = self.edge(vb, vt, self.line(pb, pt))
        # The hole side has its normal pointing into the hole: the face is not same sense.
        hs = self.add(f"CYLINDRICAL_SURFACE('',#{ab},{num(r)})")
        faces.append(
            self.face(
                [self.bound(self.loop([self.oriented(eb), self.oriented(es), self.oriented(et, False), self.oriented(es, False)]))],
                hs,
                same=False,
            )
        )
        # Bottom and top with the hole as a second bound.
        for z, corners, up in [(0.0, [v[0], v[2], v[3], v[1]], False), (h, [v[4], v[5], v[7], v[6]], True)]:
            nz = (0, 0, 1) if up else (0, 0, -1)
            surf = self.plane((0, 0, z), nz, (1, 0, 0))
            outer = self.loop(self.poly_face_edges(corners, cache))
            e = et if up else eb
            # The hole runs against the outline: clockwise seen from the normal.
            inner = self.loop([self.oriented(e, False if up else True)])
            faces.append(self.face([self.bound(outer), self.bound(inner, outer=False)], surf))
        return self.solid(self.shell(faces), name)

    def sphere_cap_solid(self, r, name="hemisphere"):
        """Upper half of a ball: a spherical cap with one loop (the equator) and a disk."""
        p = (r, 0, 0)
        v = self.vertex(p)
        a = self.axis()
        e = self.edge(v, v, self.circle(a, r))
        cap = self.face([self.bound(self.loop([self.oriented(e)]))], self.add(f"SPHERICAL_SURFACE('',#{a},{num(r)})"))
        disk_ax = self.axis((0, 0, 0), (0, 0, -1), (1, 0, 0))
        disk = self.face([self.bound(self.loop([self.oriented(e, False)]))], self.add(f"PLANE('',#{disk_ax})"))
        return self.solid(self.shell([cap, disk]), name)

    def sphere_full(self, r, name="sphere"):
        """A ball as one face with a seam and a degenerate edge at each pole."""
        s, n = (0, 0, -r), (0, 0, r)
        vs, vn = self.vertex(s), self.vertex(n)
        # The meridian in the XZ plane from the south to the north pole, counterclockwise.
        arc_ax = self.axis((0, 0, 0), (0, -1, 0), (1, 0, 0))
        # In the frame (X, Z-of-frame = -Y), the angle runs from X toward Z.
        seam = self.edge(vs, vn, self.circle(arc_ax, r))
        deg_n = self.edge(vn, vn, self.line_degenerate(n))
        deg_s = self.edge(vs, vs, self.line_degenerate(s))
        a = self.axis()
        loop = self.loop([
            self.oriented(seam), self.oriented(deg_n), self.oriented(seam, False), self.oriented(deg_s),
        ])
        f = self.face([self.bound(loop)], self.add(f"SPHERICAL_SURFACE('',#{a},{num(r)})"))
        return self.solid(self.shell([f]), name)

    def line_degenerate(self, p):
        v = self.add(f"VECTOR('',#{self.direction((0, 0, 1))},0.)")
        return self.add(f"LINE('',#{self.point(p)},#{v})")

    def torus(self, big, small, name="torus"):
        p = (big + small, 0, 0)
        v = self.vertex(p)
        a = self.axis()
        cu = self.circle(a, big + small)
        # The meridian circle in the XZ plane around (big, 0, 0).
        am = self.axis((big, 0, 0), (0, -1, 0), (1, 0, 0))
        cv = self.circle(am, small)
        eu, ev = self.edge(v, v, cu), self.edge(v, v, cv)
        loop = self.loop([self.oriented(eu), self.oriented(ev), self.oriented(eu, False), self.oriented(ev, False)])
        f = self.face([self.bound(loop)], self.add(f"TOROIDAL_SURFACE('',#{a},{num(big)},{num(small)})"))
        return self.solid(self.shell([f]), name)

    def frustum_by_cone(self, r0, r1, h, name="frustum"):
        """A cone frustum along Z with the radius r0 at z = 0 and r1 at z = h."""
        p0, p1 = (r0, 0, 0), (r1, 0, h)
        v0, v1 = self.vertex(p0), self.vertex(p1)
        a0, a1 = self.axis(), self.axis((0, 0, h))
        e0, e1 = self.edge(v0, v0, self.circle(a0, r0)), self.edge(v1, v1, self.circle(a1, r1))
        es = self.edge(v0, v1, self.line(p0, p1))
        half = math.atan2(r1 - r0, h)
        side = self.face(
            [self.bound(self.loop([self.oriented(e0), self.oriented(es), self.oriented(e1, False), self.oriented(es, False)]))],
            self.add(f"CONICAL_SURFACE('',#{a0},{num(r0)},{num(half)})"),
        )
        top = self.face([self.bound(self.loop([self.oriented(e1)]))], self.add(f"PLANE('',#{a1})"))
        bot_ax = self.axis((0, 0, 0), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(e0, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, top, bot]), name)

    def frustum_by_revolution(self, r0, r1, h, name="revolved"):
        """The same frustum, its side as a surface of revolution of a line."""
        p0, p1 = (r0, 0, 0), (r1, 0, h)
        v0, v1 = self.vertex(p0), self.vertex(p1)
        a0, a1 = self.axis(), self.axis((0, 0, h))
        e0, e1 = self.edge(v0, v0, self.circle(a0, r0)), self.edge(v1, v1, self.circle(a1, r1))
        es = self.edge(v0, v1, self.line(p0, p1))
        ax1 = self.add(f"AXIS1_PLACEMENT('',#{self.point((0, 0, 0))},#{self.direction((0, 0, 1))})")
        surf = self.add(f"SURFACE_OF_REVOLUTION('',#{self.line(p0, p1)},#{ax1})")
        side = self.face(
            [self.bound(self.loop([self.oriented(e0), self.oriented(es), self.oriented(e1, False), self.oriented(es, False)]))],
            surf,
        )
        top = self.face([self.bound(self.loop([self.oriented(e1)]))], self.add(f"PLANE('',#{a1})"))
        bot_ax = self.axis((0, 0, 0), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(e0, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, top, bot]), name)

    def elliptic_extrusion(self, a_, b_, h, name="extrusion"):
        """An ellipse extruded along Z: the side is a surface of linear extrusion."""
        p0, p1 = (a_, 0, 0), (a_, 0, h)
        v0, v1 = self.vertex(p0), self.vertex(p1)
        ax0, ax1 = self.axis(), self.axis((0, 0, h))
        ell0 = self.add(f"ELLIPSE('',#{ax0},{num(a_)},{num(b_)})")
        ell1 = self.add(f"ELLIPSE('',#{ax1},{num(a_)},{num(b_)})")
        e0, e1 = self.edge(v0, v0, ell0), self.edge(v1, v1, ell1)
        es = self.edge(v0, v1, self.line(p0, p1))
        vec_ = self.add(f"VECTOR('',#{self.direction((0, 0, 1))},1.)")
        surf = self.add(f"SURFACE_OF_LINEAR_EXTRUSION('',#{ell0},#{vec_})")
        side = self.face(
            [self.bound(self.loop([self.oriented(e0), self.oriented(es), self.oriented(e1, False), self.oriented(es, False)]))],
            surf,
        )
        top = self.face([self.bound(self.loop([self.oriented(e1)]))], self.add(f"PLANE('',#{ax1})"))
        bot_ax = self.axis((0, 0, 0), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(e0, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, top, bot]), name)

    def quarter_cylinder_patch(self, r, h, name="patch"):
        """A quarter of a cylinder as a rational B-spline patch, an open shell in a surface
        model. The area is pi/2 r h."""
        w = math.sqrt(0.5)
        grid = [[(r, 0, 0), (r, 0, h)], [(r, r, 0), (r, r, h)], [(0, r, 0), (0, r, h)]]
        ids = [[self.point(p) for p in row] for row in grid]
        rows = "(" + ",".join("(" + ",".join(f"#{i}" for i in row) + ")" for row in ids) + ")"
        wts = "((1.,1.),(" + num(w) + "," + num(w) + "),(1.,1.))"
        surf = self.add(
            "(BOUNDED_SURFACE() B_SPLINE_SURFACE(2,1," + rows + ",.UNSPECIFIED.,.F.,.F.,.F.) "
            "B_SPLINE_SURFACE_WITH_KNOTS((3,3),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.) "
            "GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(" + wts + ") "
            "REPRESENTATION_ITEM('') SURFACE())"
        )
        def arc(z):
            ps = [self.point((r, 0, z)), self.point((r, r, z)), self.point((0, r, z))]
            return self.add(
                "(BOUNDED_CURVE() B_SPLINE_CURVE(2,(" + ",".join(f"#{p}" for p in ps) + "),.UNSPECIFIED.,.F.,.F.) "
                "B_SPLINE_CURVE_WITH_KNOTS((3,3),(0.,1.),.UNSPECIFIED.) CURVE() "
                "GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE((1.," + num(w) + ",1.)) REPRESENTATION_ITEM(''))"
            )
        va, vb, vc, vd = (self.vertex(p) for p in [(r, 0, 0), (0, r, 0), (0, r, h), (r, 0, h)])
        bottom = self.edge(va, vb, arc(0.0))
        right = self.edge(vb, vc, self.line((0, r, 0), (0, r, h)))
        top = self.edge(vd, vc, arc(h))
        left = self.edge(va, vd, self.line((r, 0, 0), (r, 0, h)))
        loop = self.loop([self.oriented(bottom), self.oriented(right), self.oriented(top, False), self.oriented(left, False)])
        f = self.face([self.bound(loop)], surf)
        sh = self.shell([f], closed=False)
        return self.add(f"SHELL_BASED_SURFACE_MODEL('{name}',(#{sh}))")

    def flat_bspline_patch(self, w, d, name="flat"):
        """A bicubic patch whose control net is a flat grid: the area is w * d."""
        grid = [[(i * w / 3, j * d / 3, 0.0) for j in range(4)] for i in range(4)]
        ids = [[self.point(p) for p in row] for row in grid]
        rows = "(" + ",".join("(" + ",".join(f"#{i}" for i in row) + ")" for row in ids) + ")"
        surf = self.add(
            f"B_SPLINE_SURFACE_WITH_KNOTS('',3,3,{rows},.UNSPECIFIED.,.F.,.F.,.F.,(4,4),(4,4),(0.,1.),(0.,1.),.UNSPECIFIED.)"
        )
        return self.patch_face_model(surf, [(0, 0, 0), (w, 0, 0), (w, d, 0), (0, d, 0)], name)

    def bumpy_patch(self, name="bump"):
        """A bicubic patch with a bump in the middle."""
        grid = [[(i * 10.0, j * 10.0, 6.0 if i in (1, 2) and j in (1, 2) else 0.0) for j in range(4)] for i in range(4)]
        ids = [[self.point(p) for p in row] for row in grid]
        rows = "(" + ",".join("(" + ",".join(f"#{i}" for i in row) + ")" for row in ids) + ")"
        surf = self.add(
            f"B_SPLINE_SURFACE_WITH_KNOTS('',3,3,{rows},.UNSPECIFIED.,.F.,.F.,.F.,(4,4),(4,4),(0.,1.),(0.,1.),.UNSPECIFIED.)"
        )
        return self.patch_face_model(surf, [(0, 0, 0), (30, 0, 0), (30, 30, 0), (0, 30, 0)], name)

    def patch_face_model(self, surf, corners, name):
        cache = {}
        loop = self.loop(self.poly_face_edges(corners, cache))
        f = self.face([self.bound(loop)], surf)
        sh = self.shell([f], closed=False)
        return self.add(f"SHELL_BASED_SURFACE_MODEL('{name}',(#{sh}))")

    def faceted_tetra(self, name="tetra"):
        pts = [(0, 0, 0), (10, 0, 0), (0, 10, 0), (0, 0, 10)]
        ids = [self.point(p) for p in pts]
        faces = []
        for f in [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]]:
            pl = self.add("POLY_LOOP('',(" + ",".join(f"#{ids[i]}" for i in f) + "))")
            b = self.add(f"FACE_OUTER_BOUND('',#{pl},.T.)")
            faces.append(self.add(f"FACE('',(#{b}))"))
        sh = self.add("CLOSED_SHELL('',(" + ",".join(f"#{f}" for f in faces) + "))")
        return self.add(f"FACETED_BREP('{name}',#{sh})")

    def tessellated_box(self, w, d, h, name="tessellated"):
        v = [(float(i & 1) * w, float((i >> 1) & 1) * d, float((i >> 2) & 1) * h) for i in range(8)]
        cl = self.add("COORDINATES_LIST('',8," + "(" + ",".join(vec(p) for p in v) + "))")
        # Counterclockwise from outside, 1-based.
        quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
        tris = []
        for q in quads:
            tris.append((q[0] + 1, q[1] + 1, q[2] + 1))
            tris.append((q[0] + 1, q[2] + 1, q[3] + 1))
        tl = "(" + ",".join("(" + ",".join(str(i) for i in t) + ")" for t in tris) + ")"
        tf = self.add(f"TRIANGULATED_FACE('',#{cl},8,(),$,(),{tl})")
        return self.add(f"TESSELLATED_SOLID('{name}',(#{tf}),$)")

    def curve_set(self, name="curves"):
        a = self.point((0, 0, 0))
        b = self.point((10, 0, 0))
        c = self.point((10, 10, 5))
        poly = self.add(f"POLYLINE('',(#{a},#{b},#{c}))")
        ax = self.axis((0, 0, 20))
        circ = self.circle(ax, 5.0)
        trim = self.add(f"TRIMMED_CURVE('',#{circ},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(3.141592653589793)),.T.,.PARAMETER.)")
        full = self.add(f"TRIMMED_CURVE('',#{circ},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(6.283185307179586)),.T.,.PARAMETER.)")
        return self.add(f"GEOMETRIC_CURVE_SET('{name}',(#{poly},#{trim},#{full}))")

    def color(self, item, rgb):
        c = self.add(f"COLOUR_RGB('',{num(rgb[0])},{num(rgb[1])},{num(rgb[2])})")
        r = self.add(f"SURFACE_STYLE_RENDERING(.NORMAL_SHADING.,#{c})")
        side = self.add(f"SURFACE_SIDE_STYLE('',(#{r}))")
        use = self.add(f"SURFACE_STYLE_USAGE(.BOTH.,#{side})")
        psa = self.add(f"PRESENTATION_STYLE_ASSIGNMENT((#{use}))")
        self.add(f"STYLED_ITEM('color',(#{psa}),#{item})")

    # Units, contexts and products.

    def context(self):
        if self.ctx is not None:
            return self.ctx
        if self.unit == "mm":
            length = self.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))")
        elif self.unit == "m":
            length = self.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.))")
        elif self.unit == "mm_deg":
            length = self.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))")
        elif self.unit == "inch":
            mm = self.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))")
            dim = self.add("DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)")
            m = self.add(f"LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{mm})")
            length = self.add(f"(CONVERSION_BASED_UNIT('INCH',#{m}) LENGTH_UNIT() NAMED_UNIT(#{dim}))")
        else:
            raise ValueError(self.unit)
        ang = self.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))")
        if self.unit == "mm_deg":
            dim = self.add("DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.)")
            m = self.add(f"PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE({math.pi / 180!r}),#{ang})")
            ang = self.add(f"(CONVERSION_BASED_UNIT('DEGREE',#{m}) NAMED_UNIT(#{dim}) PLANE_ANGLE_UNIT())")
        sol = self.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())")
        self.ctx = self.add(
            f"(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{ang},#{sol})) REPRESENTATION_CONTEXT('Context','3D'))"
        )
        return self.ctx

    def product(self, name, items, frame=None):
        """A product with a shape representation of the items. `frame` is the (origin, z, x)
        of the axis that the placements refer to. Returns (product definition,
        representation, shape representation items placement axis)."""
        ctx = self.context()
        app = self.add("APPLICATION_CONTEXT('automotive design')")
        pc = self.add(f"PRODUCT_CONTEXT('',#{app},'mechanical')")
        prod = self.add(f"PRODUCT('{name}','{name}','',(#{pc}))")
        pf = self.add(f"PRODUCT_DEFINITION_FORMATION('','',#{prod})")
        dc = self.add(f"PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')")
        pd = self.add(f"PRODUCT_DEFINITION('design','',#{pf},#{dc})")
        pds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{pd})")
        origin = self.axis(*frame) if frame else self.axis()
        its = [origin] + list(items)
        rep = self.add(
            "ADVANCED_BREP_SHAPE_REPRESENTATION('" + name + "',(" + ",".join(f"#{i}" for i in its) + f"),#{ctx})"
        )
        self.add(f"SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})")
        return pd, rep, origin

    def assembly(self, name, parts):
        """An assembly of (child product definition, child rep, child origin axis, placement
        (origin, z, x)) tuples."""
        ctx = self.context()
        app = self.add("APPLICATION_CONTEXT('automotive design')")
        pc = self.add(f"PRODUCT_CONTEXT('',#{app},'mechanical')")
        prod = self.add(f"PRODUCT('{name}','{name}','',(#{pc}))")
        pf = self.add(f"PRODUCT_DEFINITION_FORMATION('','',#{prod})")
        dc = self.add(f"PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')")
        pd = self.add(f"PRODUCT_DEFINITION('design','',#{pf},#{dc})")
        pds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{pd})")
        origin = self.axis()
        rep = self.add(f"SHAPE_REPRESENTATION('{name}',(#{origin}),#{ctx})")
        self.add(f"SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})")
        for child_pd, child_rep, child_origin, place, *flags in parts:
            nauo = self.add(f"NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{pd},#{child_pd},$)")
            if place is None:
                # A usage that no placement belongs to.
                continue
            npds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{nauo})")
            target = self.axis(*place)
            if flags and flags[0] == "reverse":
                # The parent is the first representation of the relationship.
                idt = self.add(f"ITEM_DEFINED_TRANSFORMATION('','',#{target},#{child_origin})")
                first, second = rep, child_rep
            else:
                idt = self.add(f"ITEM_DEFINED_TRANSFORMATION('','',#{child_origin},#{target})")
                first, second = child_rep, rep
            rel = self.add(
                f"(REPRESENTATION_RELATIONSHIP('','',#{first},#{second}) "
                f"REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt}) SHAPE_REPRESENTATION_RELATIONSHIP())"
            )
            self.add(f"CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rel},#{npds})")
        return pd, rep, origin


    # More shapes, for the tests of the less common paths.

    def inverted_box(self, w, d, h, name="inverted"):
        """A box whose faces point inward (the corners run clockwise seen from outside)."""
        v = [((i & 1) * w, ((i >> 1) & 1) * d, ((i >> 2) & 1) * h) for i in range(8)]
        faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
        return self.polyhedron(v, [list(reversed(f)) for f in faces], name)

    def box_with_void(self, w, d, h, m, name="hollow"):
        """A box with a box-shaped hole in the middle: a solid with one void."""
        def shell_of(x0, y0, z0, a, b, c):
            v = [(x0 + (i & 1) * a, y0 + ((i >> 1) & 1) * b, z0 + ((i >> 2) & 1) * c) for i in range(8)]
            faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
            cache = {}
            out = []
            for f in faces:
                corners = [v[i] for i in f]
                nrm = (0, 0, 0)
                for i in range(len(corners)):
                    cc = cross(corners[i], corners[(i + 1) % len(corners)])
                    nrm = tuple(x + y for x, y in zip(nrm, cc))
                surf = self.plane(corners[0], nrm, sub(corners[1], corners[0]))
                loop = self.loop(self.poly_face_edges(corners, cache))
                out.append(self.face([self.bound(loop)], surf))
            return self.shell(out)
        outer = shell_of(0, 0, 0, w, d, h)
        inner = shell_of(m, m, m, w - 2 * m, d - 2 * m, h - 2 * m)
        oriented = self.add(f"ORIENTED_CLOSED_SHELL('',*,#{inner},.F.)")
        return self.add(f"BREP_WITH_VOIDS('{name}',#{outer},(#{oriented}))")

    def cylinder_two_loops(self, r, h, name="no_seam"):
        """A cylinder whose side face has two loops (the two circles) and no seam edge."""
        p0, p1 = (r, 0, 0), (r, 0, h)
        v0, v1 = self.vertex(p0), self.vertex(p1)
        a0, a1 = self.axis(), self.axis((0, 0, h))
        e0, e1 = self.edge(v0, v0, self.circle(a0, r)), self.edge(v1, v1, self.circle(a1, r))
        side_surf = self.add(f"CYLINDRICAL_SURFACE('',#{a0},{num(r)})")
        side = self.face(
            [self.bound(self.loop([self.oriented(e0)])), self.bound(self.loop([self.oriented(e1, False)]), outer=False)],
            side_surf,
        )
        top = self.face([self.bound(self.loop([self.oriented(e1)]))], self.add(f"PLANE('',#{a1})"))
        bot_ax = self.axis((0, 0, 0), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(e0, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, top, bot]), name)

    def cone_with_apex(self, r, h, name="cone", degrees=False):
        """A cone: the side face has one loop (the base circle) and the apex is a pole."""
        p0 = (r, 0, 0)
        v0 = self.vertex(p0)
        a0 = self.axis()
        e0 = self.edge(v0, v0, self.circle(a0, r))
        half = math.atan2(r, h)
        # The cone narrows upward: a negative half angle with the radius r at z = 0.
        side = self.face(
            [self.bound(self.loop([self.oriented(e0)]))],
            self.add(f"CONICAL_SURFACE('',#{a0},{num(r)},{num(-(math.degrees(half) if degrees else half))})"),
        )
        bot_ax = self.axis((0, 0, 0), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(e0, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, bot]), name)

    def oriented_face_model(self, name="faces"):
        """A face based surface model: one connected face set holding an oriented face of a
        polygon on a FACE_SURFACE, and a face of polygon loops with no surface."""
        pts = [(0, 0, 0), (10, 0, 0), (10, 10, 0), (0, 10, 0)]
        ids = [self.point(p) for p in pts]
        pl = self.add("POLY_LOOP('',(" + ",".join(f"#{i}" for i in ids) + "))")
        b = self.add(f"FACE_OUTER_BOUND('',#{pl},.T.)")
        plane = self.plane((0, 0, 0), (0, 0, 1), (1, 0, 0))
        f1 = self.add(f"FACE_SURFACE('',(#{b}),#{plane},.T.)")
        of = self.add(f"ORIENTED_FACE('',*,#{f1},.F.)")
        pts2 = [(20, 0, 0), (30, 0, 0), (30, 10, 0)]
        ids2 = [self.point(p) for p in pts2]
        pl2 = self.add("POLY_LOOP('',(" + ",".join(f"#{i}" for i in ids2) + "))")
        b2 = self.add(f"FACE_BOUND('',#{pl2},.T.)")
        f2 = self.add(f"FACE('',(#{b2}))")
        cfs = self.add(f"CONNECTED_FACE_SET('',(#{of},#{f2}))")
        return self.add(f"FACE_BASED_SURFACE_MODEL('{name}',(#{cfs}))")

    def patch_variants(self, name="patches"):
        """Patches with the other kinds of B-spline surfaces and curves: Bezier, quasi uniform and
        uniform, a rectangular trimmed surface, and one surface type that is not read."""
        def net(n, m, z=0.0):
            return [[self.point((i * 10.0, j * 10.0, z + 0.5 * i * j)) for j in range(m)] for i in range(n)]
        def rows(ids):
            return "(" + ",".join("(" + ",".join(f"#{i}" for i in r) + ")" for r in ids) + ")"
        out = []
        bez = self.add(f"BEZIER_SURFACE('',2,2,{rows(net(3, 3))},.UNSPECIFIED.,.F.,.F.,.F.)")
        qu = self.add(f"QUASI_UNIFORM_SURFACE('',2,2,{rows(net(4, 4))},.UNSPECIFIED.,.F.,.F.,.F.)")
        un = self.add(f"UNIFORM_SURFACE('',2,2,{rows(net(4, 4))},.UNSPECIFIED.,.F.,.F.,.F.)")
        plane = self.plane((0, 0, 0), (0, 0, 1), (1, 0, 0))
        rt = self.add(f"RECTANGULAR_TRIMMED_SURFACE('',#{plane},0.,20.,0.,20.,.T.,.T.)")
        off = self.add(f"OFFSET_SURFACE('',#{plane},1.,.F.)")
        for surf in (bez, qu, un, rt, off):
            out.append(self.patch_face(surf, [(0, 0, 0), (20, 0, 0), (20, 20, 0), (0, 20, 0)]))
        sh = self.add("OPEN_SHELL('',(" + ",".join(f"#{f}" for f in out) + "))")
        return self.add(f"SHELL_BASED_SURFACE_MODEL('{name}',(#{sh}))")

    def patch_face(self, surf, corners):
        cache = {}
        loop = self.loop(self.poly_face_edges(corners, cache))
        return self.face([self.bound(loop)], surf)

    def curve_zoo(self, name="zoo"):
        """A geometric set with the less common curves: a composite curve of a line and an arc, a
        surface curve, Bezier, quasi uniform and uniform B-spline curves, trims by point, an
        ellipse, a bare line (no ends) and a hyperbola (not read)."""
        o = self.point((0, 0, 0))
        a = self.point((10, 0, 0))
        b = self.point((10, 10, 0))
        ids = []
        line = self.line((0, 0, 0), (10, 0, 0))
        seg1 = self.add(f"TRIMMED_CURVE('',#{line},(#{o}),(#{a}),.T.,.CARTESIAN.)")
        ax = self.axis((10, 5, 0), (0, 0, 1), (0, -1, 0))
        circ = self.circle(ax, 5.0)
        seg2 = self.add(f"TRIMMED_CURVE('',#{circ},(#{a}),(#{b}),.T.,.CARTESIAN.)")
        cs1 = self.add(f"COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#{seg1})")
        cs2 = self.add(f"COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#{seg2})")
        ids.append(self.add(f"COMPOSITE_CURVE('',(#{cs1},#{cs2}),.F.)"))
        ids.append(self.add(f"SURFACE_CURVE('',#{seg1},(),.CURVE_3D.)"))
        pts = [self.point((0, 0, 0)), self.point((5, 5, 0)), self.point((10, 0, 0)), self.point((15, 5, 2))]
        pl = ",".join(f"#{i}" for i in pts)
        ids.append(self.add(f"BEZIER_CURVE('',3,({pl}),.UNSPECIFIED.,.F.,.F.)"))
        ids.append(self.add(f"QUASI_UNIFORM_CURVE('',2,({pl}),.UNSPECIFIED.,.F.,.F.)"))
        ids.append(self.add(f"UNIFORM_CURVE('',2,({pl}),.UNSPECIFIED.,.F.,.F.)"))
        eax = self.axis((0, 0, 30))
        ell = self.add(f"ELLIPSE('',#{eax},8.,4.)")
        ids.append(self.add(f"TRIMMED_CURVE('',#{ell},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(3.0)),.F.,.PARAMETER.)"))
        ids.append(self.add(f"TRIMMED_CURVE('',#{circ},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.5)),.T.,.UNSPECIFIED.)"))
        ids.append(line)  # a line has no ends: not drawn
        hyp = self.add(f"HYPERBOLA('',#{eax},3.,2.)")
        ids.append(hyp)
        return self.add(f"GEOMETRIC_SET('{name}',(" + ",".join(f"#{i}" for i in ids) + "))")

    def edge_zoo(self, name="edges"):
        """A face whose edges are a trimmed circle, a polyline, a B-spline curve and a composite
        curve, on a plane."""
        pts = [(0, 0, 0), (20, 0, 0), (20, 10, 0), (10, 20, 0), (0, 10, 0)]
        vs = [self.vertex(p) for p in pts]
        # An arc from (20, 0) to (20, 10) around (20, 5)... as a trimmed circle edge.
        ax = self.axis((20, 5, 0), (0, 0, 1), (1, 0, 0))
        circ = self.circle(ax, 5.0)
        arc = self.add(f"TRIMMED_CURVE('',#{circ},(#{self.point(pts[1])}),(#{self.point(pts[2])}),.T.,.CARTESIAN.)")
        e1 = self.edge(vs[0], vs[1], self.line(pts[0], pts[1]))
        e2 = self.edge(vs[1], vs[2], arc)
        # A polyline edge from (20, 10) over (15, 15) to (10, 20).
        mid = self.point((15, 15, 0))
        poly = self.add(f"POLYLINE('',(#{self.point(pts[2])},#{mid},#{self.point(pts[3])}))")
        e3 = self.edge(vs[2], vs[3], poly)
        # A B-spline edge from (10, 20) to (0, 10).
        cps = [self.point(pts[3]), self.point((5, 18, 0)), self.point(pts[4])]
        spl = self.add(
            f"B_SPLINE_CURVE_WITH_KNOTS('',2,(" + ",".join(f"#{i}" for i in cps) + "),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.)"
        )
        e4 = self.edge(vs[3], vs[4], spl)
        # A composite curve edge from (0, 10) back to (0, 0), against its direction.
        l1 = self.add(f"TRIMMED_CURVE('',#{self.line((0, 0, 0), (0, 10, 0))},(#{self.point((0, 0, 0))}),(#{self.point((0, 10, 0))}),.T.,.CARTESIAN.)")
        c1 = self.add(f"COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#{l1})")
        comp = self.add(f"COMPOSITE_CURVE('',(#{c1}),.F.)")
        e5 = self.edge(vs[0], vs[4], comp)
        loop = self.loop([self.oriented(e1), self.oriented(e2), self.oriented(e3), self.oriented(e4), self.oriented(e5, False)])
        plane = self.plane((0, 0, 0), (0, 0, 1), (1, 0, 0))
        f = self.face([self.bound(loop)], plane)
        sh = self.shell([f], closed=False)
        return self.add(f"SHELL_BASED_SURFACE_MODEL('{name}',(#{sh}))")

    def tessellated_variants(self):
        """Triangulated items with a point index, strips and fans, and a shell of them. One
        triangle names a point that does not exist."""
        pts = [(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0), (5.0, 5.0, 8.0)]
        cl = self.add("COORDINATES_LIST('',5,(" + ",".join(vec(p) for p in pts) + "))")
        # With a point index: the index maps 1..4 to the points 5, 1, 2, 3 (so (2,3,4) is 1,2,3).
        tf = self.add(f"TRIANGULATED_FACE('',#{cl},5,(),$,(5,1,2,3),((2,3,4),(2,4,1),(1,1,9)))")
        ts = self.add(f"TRIANGULATED_SURFACE_SET('',#{cl},5,(),(),((1,2,5),(2,3,5)))")
        cf = self.add(f"COMPLEX_TRIANGULATED_FACE('',#{cl},5,(),$,(),((1,2,5,3,4)),((5,1,2,3)))")
        shell = self.add(f"TESSELLATED_SHELL('',(#{tf},#{ts},#{cf}),$)")
        bad = self.add(f"TRIANGULATED_FACE('',#{cl},5,(),$,(),((1,2,3),(3,4,1)))")
        wire = self.add("TESSELLATED_CURVE_SET('',#" + str(cl) + ",((1,2)))")
        solid = self.add(f"TESSELLATED_SOLID('mixed',(#{bad},#{wire}),$)")
        return shell, solid

    def style_variants(self, items):
        """Colors: predefined names, a transparent one, and a style on a face."""
        c = self.add("DRAUGHTING_PRE_DEFINED_COLOUR('green')")
        r = self.add(f"SURFACE_STYLE_RENDERING(.NORMAL_SHADING.,#{c})")
        tr = self.add("SURFACE_STYLE_TRANSPARENT(0.25)")
        rr = self.add(f"SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.NORMAL_SHADING.,#{c},(#{tr}))")
        side = self.add(f"SURFACE_SIDE_STYLE('',(#{r},#{rr}))")
        use = self.add(f"SURFACE_STYLE_USAGE(.BOTH.,#{side})")
        psa = self.add(f"PRESENTATION_STYLE_ASSIGNMENT((#{use}))")
        for it in items:
            self.add(f"STYLED_ITEM('',(#{psa}),#{it})")
        for name, rgb in (("red", 0), ("blue", 0), ("yellow", 0), ("cyan", 0), ("magenta", 0), ("black", 0), ("white", 0), ("unknown", 0)):
            c2 = self.add(f"DRAUGHTING_PRE_DEFINED_COLOUR('{name}')")
            r2 = self.add(f"SURFACE_STYLE_RENDERING(.NORMAL_SHADING.,#{c2})")
            s2 = self.add(f"SURFACE_SIDE_STYLE('',(#{r2}))")
            u2 = self.add(f"SURFACE_STYLE_USAGE(.BOTH.,#{s2})")
            p2 = self.add(f"PRESENTATION_STYLE_ASSIGNMENT((#{u2}))")
            self.add(f"STYLED_ITEM('',(#{p2}),#{items[0]})")

    def mapped_assembly(self, name="Plate", count=2):
        """A product of one box and an assembly that maps its representation twice."""
        box = self.box(10, 10, 5, name="plate")
        pd, rep, origin = self.product(name, [box])
        ctx = self.context()
        app = self.add("APPLICATION_CONTEXT('automotive design')")
        pc = self.add(f"PRODUCT_CONTEXT('',#{app},'mechanical')")
        prod = self.add(f"PRODUCT('Frame','Frame','',(#{pc}))")
        pf = self.add(f"PRODUCT_DEFINITION_FORMATION('','',#{prod})")
        dc = self.add(f"PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')")
        apd = self.add(f"PRODUCT_DEFINITION('design','',#{pf},#{dc})")
        pds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{apd})")
        rmap = self.add(f"REPRESENTATION_MAP(#{origin},#{rep})")
        items = []
        for k in range(count):
            target = self.axis((50.0 * (k + 1), 0, 0), (0, 0, 1), (1, 0, 0) if k == 0 else (0, 1, 0))
            items.append(self.add(f"MAPPED_ITEM('',#{rmap},#{target})"))
        arep = self.add("SHAPE_REPRESENTATION('Frame',(" + ",".join(f"#{i}" for i in items) + f"),#{ctx})")
        self.add(f"SHAPE_DEFINITION_REPRESENTATION(#{pds},#{arep})")

    def related_representations(self, name="Related"):
        """A product whose shape representation holds no geometry. The geometry is in another
        representation that a plain relationship ties to it."""
        ctx = self.context()
        box = self.box(10, 20, 30, name="related box")
        app = self.add("APPLICATION_CONTEXT('automotive design')")
        pc = self.add(f"PRODUCT_CONTEXT('',#{app},'mechanical')")
        prod = self.add(f"PRODUCT('{name}','{name}','',(#{pc}))")
        pf = self.add(f"PRODUCT_DEFINITION_FORMATION('','',#{prod})")
        dc = self.add(f"PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')")
        pd = self.add(f"PRODUCT_DEFINITION('design','',#{pf},#{dc})")
        pds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{pd})")
        empty = self.add(f"SHAPE_REPRESENTATION('empty',(#{self.axis()}),#{ctx})")
        geo = self.add(f"ADVANCED_BREP_SHAPE_REPRESENTATION('geo',(#{self.axis()},#{box}),#{ctx})")
        self.add(f"SHAPE_DEFINITION_REPRESENTATION(#{pds},#{empty})")
        self.add(f"SHAPE_REPRESENTATION_RELATIONSHIP('','',#{empty},#{geo})")


    def sphere_zone(self, r, z, name="bead"):
        """The part of a ball between the planes z = -z and z = +z: a spherical face with two
        loops (the two circles) and no seam, and two disks."""
        rho = math.sqrt(r * r - z * z)
        pb, pt = (rho, 0, -z), (rho, 0, z)
        vb, vt = self.vertex(pb), self.vertex(pt)
        ab, at_ = self.axis((0, 0, -z)), self.axis((0, 0, z))
        eb, et = self.edge(vb, vb, self.circle(ab, rho)), self.edge(vt, vt, self.circle(at_, rho))
        sph = self.add(f"SPHERICAL_SURFACE('',#{self.axis()},{num(r)})")
        side = self.face(
            [self.bound(self.loop([self.oriented(eb)])), self.bound(self.loop([self.oriented(et, False)]), outer=False)],
            sph,
        )
        top = self.face([self.bound(self.loop([self.oriented(et)]))], self.add(f"PLANE('',#{at_})"))
        bot_ax = self.axis((0, 0, -z), (0, 0, -1), (1, 0, 0))
        bot = self.face([self.bound(self.loop([self.oriented(eb, False)]))], self.add(f"PLANE('',#{bot_ax})"))
        return self.solid(self.shell([side, top, bot]), name)

    def closed_edges(self, name="closed"):
        """Three planar faces whose single edge is a closed curve: a rational B-spline circle, a
        closed polyline and a trimmed B-spline circle that runs against its direction."""
        w = math.sqrt(0.5)
        r = 10.0
        def spline(z):
            ring = [(r, 0), (r, r), (0, r), (-r, r), (-r, 0), (-r, -r), (0, -r), (r, -r), (r, 0)]
            ids = ",".join(f"#{self.point((x, y, z))}" for x, y in ring)
            wts = ",".join(num(x) for x in [1, w, 1, w, 1, w, 1, w, 1])
            return self.add(
                f"(BOUNDED_CURVE() B_SPLINE_CURVE(2,({ids}),.UNSPECIFIED.,.T.,.F.) "
                f"B_SPLINE_CURVE_WITH_KNOTS((3,2,2,2,3),(0.,1.,2.,3.,4.),.UNSPECIFIED.) CURVE() "
                f"GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({wts})) REPRESENTATION_ITEM(''))"
            )
        faces = []
        # 1. The rational circle at z = 0.
        v = self.vertex((r, 0, 0))
        e = self.edge(v, v, spline(0.0))
        faces.append(self.face([self.bound(self.loop([self.oriented(e)]))], self.plane((0, 0, 0), (0, 0, 1), (1, 0, 0))))
        # 2. A closed polyline square at z = 20.
        sq = [(0, 0, 20), (10, 0, 20), (10, 10, 20), (0, 10, 20), (0, 0, 20)]
        poly = self.add("POLYLINE('',(" + ",".join(f"#{self.point(p)}" for p in sq) + "))")
        v2 = self.vertex((0, 0, 20))
        e2 = self.edge(v2, v2, poly)
        faces.append(self.face([self.bound(self.loop([self.oriented(e2)]))], self.plane((0, 0, 20), (0, 0, 1), (1, 0, 0))))
        # 3. A trimmed copy of the rational circle at z = 40, edge sense against the curve.
        v3 = self.vertex((r, 0, 40))
        tr = self.add(f"TRIMMED_CURVE('',#{spline(40.0)},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(4.)),.T.,.PARAMETER.)")
        e3 = self.edge(v3, v3, tr, same=False)
        faces.append(self.face([self.bound(self.loop([self.oriented(e3, False)]))], self.plane((0, 0, 40), (0, 0, 1), (1, 0, 0))))
        sh = self.shell(faces, closed=False)
        return self.add(f"SHELL_BASED_SURFACE_MODEL('{name}',(#{sh}))")

    def text(self):
        head = (
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Kadakas test file'),'2;1');\n"
            "FILE_NAME('test.step','2026-01-01T00:00:00',('Kadakas'),(''),'make_step_files.py','','');\n"
            f"FILE_SCHEMA(('{self.schema}'));\nENDSEC;\nDATA;\n"
        )
        return head + "\n".join(self.lines) + "\nENDSEC;\nEND-ISO-10303-21;\n"


def single(maker, name, unit="mm", schema="AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }", color=None):
    s = Step(schema=schema, unit=unit)
    item = maker(s)
    if color is not None:
        s.color(item, color)
    s.product(name, [item])
    return s


def fixtures():
    out = {}
    out["box.step"] = single(lambda s: s.box(20, 30, 40), "Box", color=(1.0, 0.0, 0.0))
    out["box_inch.step"] = single(lambda s: s.box(1, 2, 3), "Inch box", unit="inch")
    out["box_metre.step"] = single(lambda s: s.box(0.02, 0.03, 0.04), "Metre box", unit="m", schema="CONFIG_CONTROL_DESIGN")
    out["cylinder.step"] = single(lambda s: s.cylinder(10, 25), "Cylinder")
    out["sphere.step"] = single(lambda s: s.sphere_full(12), "Sphere")
    out["hemisphere.step"] = single(lambda s: s.sphere_cap_solid(12), "Hemisphere")
    out["torus.step"] = single(lambda s: s.torus(20, 5), "Torus")
    out["frustum.step"] = single(lambda s: s.frustum_by_cone(12, 6, 20), "Frustum")
    out["revolved.step"] = single(lambda s: s.frustum_by_revolution(12, 6, 20), "Revolved")
    out["extrusion.step"] = single(lambda s: s.elliptic_extrusion(15, 8, 10), "Extrusion")
    out["plate_hole.step"] = single(lambda s: s.plate_with_hole(60, 40, 8, 6), "Plate")
    out["patch_rational.step"] = single(lambda s: s.quarter_cylinder_patch(10, 20), "Quarter pipe")
    out["patch_flat.step"] = single(lambda s: s.flat_bspline_patch(30, 20), "Flat patch")
    out["patch_bump.step"] = single(lambda s: s.bumpy_patch(), "Bump")
    out["faceted.step"] = single(lambda s: s.faceted_tetra(), "Tetra", schema="CONFIG_CONTROL_DESIGN")
    out["tessellated.step"] = single(
        lambda s: s.tessellated_box(10, 20, 30), "Tessellated", schema="AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"
    )
    out["curves.step"] = single(lambda s: s.curve_set(), "Curves")

    # Files for the less common paths.
    out["inverted.step"] = single(lambda s: s.inverted_box(10, 20, 30), "Inverted")
    out["hollow.step"] = single(lambda s: s.box_with_void(40, 40, 40, 10), "Hollow")
    out["no_seam.step"] = single(lambda s: s.cylinder_two_loops(10, 20), "No seam")
    out["cone.step"] = single(lambda s: s.cone_with_apex(10, 25), "Cone")
    out["oriented_faces.step"] = single(lambda s: s.oriented_face_model(), "Faces")
    out["patch_variants.step"] = single(lambda s: s.patch_variants(), "Patches")
    out["curve_zoo.step"] = single(lambda s: s.curve_zoo(), "Zoo")
    out["edge_zoo.step"] = single(lambda s: s.edge_zoo(), "Edges")
    s = Step(unit="mm")
    shell, solid = s.tessellated_variants()
    s.product("Tessellated variants", [shell, solid])
    out["tessellated_variants.step"] = s
    s = Step(unit="mm_deg")
    box = s.box(10, 10, 10, name="colored")
    s.style_variants([box])
    cone = s.cone_with_apex(10, 25, name="cone in degrees", degrees=True)
    ax = s.axis((0, 0, 40))
    quarter = s.add(f"TRIMMED_CURVE('',#{s.circle(ax, 10.0)},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.)")
    curves = s.add(f"GEOMETRIC_CURVE_SET('arc in degrees',(#{quarter}))")
    s.product("Styled", [box, cone, curves])
    out["styles.step"] = s
    s = Step()
    s.mapped_assembly()
    out["mapped.step"] = s
    s = Step()
    s.related_representations()
    out["related.step"] = s
    # No product structure at all and no units: bare solids.
    s = Step()
    s.box(10, 10, 10, name="loose box")
    s.cylinder(5, 20, at=(30, 0, 0), name="loose cylinder")
    out["loose.step"] = s
    out["partial_box.step"] = single(lambda s: s.box(20, 30, 40, at=(10000, 0, 0), bad_face=1), "Partial box")
    out["bead.step"] = single(lambda s: s.sphere_zone(12, 6), "Bead")
    out["closed_edges.step"] = single(lambda s: s.closed_edges(), "Closed")
    # An assembly: a leg product used twice, with two placements.
    s = Step()
    leg = s.box(10, 10, 50, name="leg")
    s.color(leg, (0.0, 0.0, 1.0))
    pd, rep, origin = s.product("Leg", [leg])
    s.assembly("Table", [(pd, rep, origin, ((100, 0, 0), (0, 0, 1), (1, 0, 0))), (pd, rep, origin, ((0, 200, 0), (0, 0, 1), (0, 1, 0)))])
    out["assembly.step"] = s
    # A child whose own frame is not the origin (item 1 is moved and turned), used with the
    # child as the first representation and then with the parent as the first one.
    s = Step()
    cube = s.box(10, 10, 10, name="cube")
    pd, rep, origin = s.product("Cube", [cube], frame=((2, 0, 0), (0, 0, 1), (0, 1, 0)))
    s.assembly(
        "Frames",
        [
            (pd, rep, origin, ((100, 0, 0), (0, 0, 1), (1, 0, 0))),
            (pd, rep, origin, ((200, 0, 0), (0, 0, 1), (1, 0, 0)), "reverse"),
        ],
    )
    out["assembly_frames.step"] = s
    # One usage has a placement and one has none.
    s = Step()
    leg = s.box(10, 10, 50, name="leg")
    pd, rep, origin = s.product("Leg", [leg])
    s.assembly("Table", [(pd, rep, origin, ((100, 0, 0), (0, 0, 1), (1, 0, 0))), (pd, rep, origin, None)])
    out["assembly_unplaced.step"] = s
    return out


def main():
    folder = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent
    for name, s in fixtures().items():
        (folder / name).write_text(s.text())
    print(f"{len(fixtures())} files written to {folder}")


if __name__ == "__main__":
    main()
