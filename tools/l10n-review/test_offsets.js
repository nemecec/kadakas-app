'use strict';
// Run: node tools/l10n-review/test_offsets.js
const assert = require('assert');
const O = require('./static/offsets.js');

// A tiny fake DOM: root with spans, each with one text node.
function build(text, highlights) {
  const root = { nodeType: 1, dataset: { len: String(text.length) }, childNodes: [], parentNode: null };
  O.segments(text, highlights).forEach(s => {
    const el = { nodeType: 1, dataset: { s: String(s.start), len: String(s.end - s.start) }, parentNode: root, childNodes: [] };
    const tn = { nodeType: 3, parentNode: el, textContent: s.text };
    el.childNodes.push(tn);
    root.childNodes.push(el);
  });
  return root;
}

// segments cover the text without gaps
let segs = O.segments('Alles on need taastekoopiad', [{ start: 9, end: 13 }, { start: 0, end: 5 }]);
assert.deepStrictEqual(segs.map(s => s.text), ['Alles', ' on ', 'need', ' taastekoopiad']);
assert.deepStrictEqual(segs.map(s => s.hl), [true, false, true, false]);
assert.strictEqual(segs.map(s => s.text).join(''), 'Alles on need taastekoopiad');
assert.deepStrictEqual(O.segments('', []).map(s => s.text), ['']);

// overlapping and out of range highlights are cleaned
assert.deepStrictEqual(O.cleanHighlights('abcdef', [{ start: 0, end: 4 }, { start: 2, end: 5 }, { start: 5, end: 99 }, { start: 3, end: 3 }]),
  [{ start: 0, end: 4 }, { start: 5, end: 6 }]);

// normalize: swap, trim, clamp
assert.deepStrictEqual(O.normalizeRange(' kaks  sõna ', 0, 12), { start: 1, end: 11 });
assert.deepStrictEqual(O.normalizeRange('abc', 3, 1), { start: 1, end: 3 });
assert.strictEqual(O.normalizeRange('a   b', 1, 4), null);

// point mapping inside text nodes of plain and highlighted pieces
const text = 'Alles on need taastekoopiad';
const root = build(text, [{ start: 0, end: 5 }]);
const first = root.childNodes[0], second = root.childNodes[1];
assert.strictEqual(O.pointToOffset(first.childNodes[0], 3, root), 3);
assert.strictEqual(O.pointToOffset(second.childNodes[0], 4, root), 9);   // "need" starts at 9 in text
assert.strictEqual(O.pointToOffset(second.childNodes[0], 0, root), 5);
// element points
assert.strictEqual(O.pointToOffset(root, 0, root), 0);
assert.strictEqual(O.pointToOffset(root, 1, root), 5);
assert.strictEqual(O.pointToOffset(root, 2, root), text.length);
assert.strictEqual(O.pointToOffset(second, 0, root), 5);
assert.strictEqual(O.pointToOffset(second, 1, root), text.length);
assert.strictEqual(O.pointToOffset({ nodeType: 3, parentNode: null }, 1, root), null);

// a selection across two pieces, with surrounding spaces trimmed
const sel = { rangeCount: 1, isCollapsed: false, getRangeAt: () => ({ startContainer: first.childNodes[0], startOffset: 3, endContainer: second.childNodes[0], endOffset: 4 }) };
assert.deepStrictEqual(O.selectionToRange(root, sel, text), { start: 3, end: 8 });
assert.strictEqual(text.slice(3, 8), 'es on');   // the trailing space is trimmed
assert.strictEqual(O.selectionToRange(root, { rangeCount: 0 }, text), null);
assert.strictEqual(O.selectionToRange(root, { rangeCount: 1, isCollapsed: true }, text), null);

// astral characters count as two units, like the server expects (UTF-16)
const emoji = 'a\u{1F600}b tere';
assert.strictEqual(emoji.length, 9);
const r2 = build(emoji, []);
assert.strictEqual(O.pointToOffset(r2.childNodes[0].childNodes[0], 4, r2), 4);
assert.strictEqual(emoji.slice(5, 9), 'tere');

// word diff marks only the new words
const d = O.diffWords('Jooksutuse aruanne 3', 'Kokkujooksmise aruanne 3');
assert.deepStrictEqual(d.filter(p => p.changed).map(p => p.text), ['Kokkujooksmise']);
assert.strictEqual(d.map(p => p.text).join(''), 'Kokkujooksmise aruanne 3');
assert.deepStrictEqual(O.diffWords('a b', 'a b').filter(p => p.changed), []);
console.log('offsets ok');
