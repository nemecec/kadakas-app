// Radial fan: rings of wedge-shaped units around the origin of the pattern, like a fan of
// cobbles round a fountain. A generator for a pattern of the kind "generator" (PAT-09).
//
// params: { ring: 500,       width of a ring (mm)
//           unit: 300,       about how long a wedge is along the middle of its ring (mm)
//           count: 0,        units in every ring; 0 = choose from `unit` for each ring
//           step: 5 }        degrees of an arc piece (arcs are cut into straight pieces)
//
// The fan starts at args.origin and turns with args.angle, so the pattern origin and angle of
// the object set where the fan is and where its first joint points. Two unit types alternate
// from ring to ring. The shapes are not clipped; Kadakas cuts them to the region.
const p = args.params || {};
const ring = p.ring || 500;
const unit = p.unit || 300;
const step = p.step || 5;
const [cx, cy] = args.origin;
const turn = (args.angle * Math.PI) / 180;
const [x0, y0, x1, y1] = args.region.box;
// The fan reaches the farthest corner of the box.
const reach = Math.max(
  Math.hypot(x0 - cx, y0 - cy), Math.hypot(x1 - cx, y0 - cy),
  Math.hypot(x0 - cx, y1 - cy), Math.hypot(x1 - cx, y1 - cy));
const rings = Math.ceil(reach / ring);

const points = [];
const start = [0];
const kinds = [];
for (let r = 0; r < rings; r++) {
  const r0 = r * ring, r1 = (r + 1) * ring;
  const n = p.count > 0 ? p.count : Math.max(3, Math.round((2 * Math.PI * (r0 + r1) / 2) / unit));
  for (let i = 0; i < n; i++) {
    const a0 = turn + (2 * Math.PI * i) / n, a1 = turn + (2 * Math.PI * (i + 1)) / n;
    const pieces = Math.max(1, Math.ceil(((a1 - a0) * 180) / Math.PI / step));
    // Outer arc from a0 to a1, then the inner arc back (a ring that starts at the center has a
    // point there instead).
    for (let k = 0; k <= pieces; k++) {
      const a = a0 + ((a1 - a0) * k) / pieces;
      points.push(cx + r1 * Math.cos(a), cy + r1 * Math.sin(a));
    }
    if (r0 === 0) {
      points.push(cx, cy);
    } else {
      for (let k = pieces; k >= 0; k--) {
        const a = a0 + ((a1 - a0) * k) / pieces;
        points.push(cx + r0 * Math.cos(a), cy + r0 * Math.sin(a));
      }
    }
    start.push(points.length / 2);
    kinds.push(r % 2);
  }
}
return {
  types: ["even ring", "odd ring"],
  polygons: {
    kind: new Uint32Array(kinds),
    start: new Uint32Array(start),
    points: new Float64Array(points),
  },
};
