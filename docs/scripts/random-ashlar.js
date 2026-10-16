// Random ashlar: courses of equal height, in each course units of three lengths in a random
// order. A generator program for a pattern of the kind "generator" (PAT-09): it makes the unit
// shapes. Run it with script.eval (see docs/scripting-javascript.md), then write what it
// returns into a pattern; the document stores the result and runs no code.
//
// args.region.box = [x0, y0, x1, y1]: the area to cover (mm), for example the box of a hardscape.
// args.params = { course: 200,              height of a course (mm)
//                 lengths: [300, 450, 600], the three unit lengths (mm)
//                 seed: 1 }                 the same seed gives the same wall
//
// The shapes are the cells at the centers of the joints, not clipped. Kadakas moves them inward
// by half the gap and cuts them to the region and its holes.
const p = args.params || {};
const course = p.course || 200;
const lengths = p.lengths || [300, 450, 600];
let state = (p.seed === undefined ? 1 : p.seed) >>> 0;

// A small random generator with a seed of our own (xorshift32), so the result does not depend on
// anything but the parameters.
function next() {
  state ^= state << 13; state >>>= 0;
  state ^= state >>> 17;
  state ^= state << 5; state >>>= 0;
  return state / 4294967296;
}

const [x0, y0, x1, y1] = args.region.box;
const rows = Math.ceil((y1 - y0) / course);
const rects = [];
const kinds = [];
for (let r = 0; r < rows; r++) {
  const cy = y0 + (r + 0.5) * course;
  // Start a little to the left of the region by a random part of a unit, so the joints of
  // neighbouring courses do not line up.
  let x = x0 - next() * lengths[0];
  while (x < x1) {
    const k = Math.min(lengths.length - 1, Math.floor(next() * lengths.length));
    const len = lengths[k];
    rects.push(x + len / 2, cy, len, course, 0);
    kinds.push(k);
    x += len;
  }
}
return {
  types: lengths.map((l) => `${l}x${course}`),
  rects: { kind: new Uint32Array(kinds), data: new Float64Array(rects) },
};
