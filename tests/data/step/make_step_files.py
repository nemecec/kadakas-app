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

    def polyhedron(self, verts, faces, name="solid"):
        """Faces are lists of corner indices, counterclockwise seen from outside."""
        cache = {}
        fs = []
        for f in faces:
            corners = [tuple(verts[i]) for i in f]
            nrm = (0, 0, 0)
            for i in range(len(corners)):
                c = cross(corners[i], corners[(i + 1) % len(corners)])
                nrm = tuple(x + y for x, y in zip(nrm, c))
            x = sub(corners[1], corners[0])
            surf = self.plane(corners[0], nrm, x)
            loop = self.loop(self.poly_face_edges(corners, cache))
            fs.append(self.face([self.bound(loop)], surf))
        return self.solid(self.shell(fs), name)

    def box(self, w, d, h, at=(0, 0, 0), name="box"):
        x0, y0, z0 = at
        v = [(x0 + (i & 1) * w, y0 + ((i >> 1) & 1) * d, z0 + ((i >> 2) & 1) * h) for i in range(8)]
        faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
        return self.polyhedron(v, faces, name)

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
        elif self.unit == "inch":
            mm = self.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))")
            dim = self.add("DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)")
            m = self.add(f"LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{mm})")
            length = self.add(f"(CONVERSION_BASED_UNIT('INCH',#{m}) LENGTH_UNIT() NAMED_UNIT(#{dim}))")
        else:
            raise ValueError(self.unit)
        ang = self.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))")
        sol = self.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())")
        self.ctx = self.add(
            f"(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{ang},#{sol})) REPRESENTATION_CONTEXT('Context','3D'))"
        )
        return self.ctx

    def product(self, name, items):
        """A product with a shape representation of the items. Returns (product definition,
        representation, shape representation items placement axis)."""
        ctx = self.context()
        app = self.add("APPLICATION_CONTEXT('automotive design')")
        pc = self.add(f"PRODUCT_CONTEXT('',#{app},'mechanical')")
        prod = self.add(f"PRODUCT('{name}','{name}','',(#{pc}))")
        pf = self.add(f"PRODUCT_DEFINITION_FORMATION('','',#{prod})")
        dc = self.add(f"PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')")
        pd = self.add(f"PRODUCT_DEFINITION('design','',#{pf},#{dc})")
        pds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{pd})")
        origin = self.axis()
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
        for child_pd, child_rep, child_origin, place in parts:
            nauo = self.add(f"NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{pd},#{child_pd},$)")
            npds = self.add(f"PRODUCT_DEFINITION_SHAPE('','',#{nauo})")
            target = self.axis(*place)
            idt = self.add(f"ITEM_DEFINED_TRANSFORMATION('','',#{child_origin},#{target})")
            rel = self.add(
                f"(REPRESENTATION_RELATIONSHIP('','',#{child_rep},#{rep}) "
                f"REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt}) SHAPE_REPRESENTATION_RELATIONSHIP())"
            )
            self.add(f"CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rel},#{npds})")
        return pd, rep, origin

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
    # An assembly: a leg product used twice, with two placements.
    s = Step()
    leg = s.box(10, 10, 50, name="leg")
    s.color(leg, (0.0, 0.0, 1.0))
    pd, rep, origin = s.product("Leg", [leg])
    s.assembly("Table", [(pd, rep, origin, ((100, 0, 0), (0, 0, 1), (1, 0, 0))), (pd, rep, origin, ((0, 200, 0), (0, 0, 1), (0, 1, 0)))])
    out["assembly.step"] = s
    return out


def main():
    folder = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent
    for name, s in fixtures().items():
        (folder / name).write_text(s.text())
    print(f"{len(fixtures())} files written to {folder}")


if __name__ == "__main__":
    main()
