'use strict';
// Run: node tools/l10n-review/test_editing.js
const assert = require('assert');
const E = require('./static/editing.js');

function fakeStorage() {
  const m = new Map();
  return { get length() { return m.size; }, key: i => [...m.keys()][i] ?? null,
    getItem: k => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: k => m.delete(k) };
}

// chip insertion at a cursor and over a selection
assert.deepStrictEqual(E.insertAtCursor('Tere maailm', 5, 5, '{ $name }'), { text: 'Tere { $name }maailm', cursor: 14 });
assert.deepStrictEqual(E.insertAtCursor('abc', 1, 2, '{ " " }'), { text: 'a{ " " }c', cursor: 8 });
assert.deepStrictEqual(E.insertAtCursor('abc', 99, 5, 'X'), { text: 'abcX', cursor: 4 });
assert.deepStrictEqual(E.insertAtCursor('abc', 3, 1, 'X'), { text: 'abcX', cursor: 4 });
assert.deepStrictEqual(E.insertAtCursor('', 0, 0, '{ -brand }'), { text: '{ -brand }', cursor: 10 });

// changed means different from the original, ignoring edge line breaks
assert.strictEqual(E.isChanged('a', 'a'), false);
assert.strictEqual(E.isChanged('a\n', 'a'), false);
assert.strictEqual(E.isChanged('b', 'a'), true);

// drafts: saved only when changed, restored only when the file text is unchanged
const st = fakeStorage();
assert.strictEqual(E.saveDraft(st, 'et', 'f.ftl::k', 'same', 'same'), false);
assert.strictEqual(st.length, 0);
assert.strictEqual(E.saveDraft(st, 'et', 'f.ftl::k', 'new', 'old'), true);
assert.strictEqual(E.loadDraft(st, 'et', 'f.ftl::k', 'old').text, 'new');
assert.strictEqual(E.loadDraft(st, 'et', 'f.ftl::k', 'changed in file'), null);
assert.strictEqual(E.loadDraft(st, 'de', 'f.ftl::k', 'old'), null);
// going back to the original removes the draft
E.saveDraft(st, 'et', 'f.ftl::k', 'old', 'old');
assert.strictEqual(st.length, 0);

// newest draft over several messages, stale ones skipped
const real = Date.now; let t = 1000; Date.now = () => t;
E.saveDraft(st, 'et', 'a::1', 'x1', 'o1'); t = 2000;
E.saveDraft(st, 'et', 'b::2', 'x2', 'o2'); t = 3000;
E.saveDraft(st, 'et', 'c::3', 'x3', 'o3');
Date.now = real;
assert.strictEqual(E.newestDraft(st, 'et', { 'a::1': 'o1', 'b::2': 'o2', 'c::3': 'CHANGED' }).id, 'b::2');
assert.strictEqual(E.newestDraft(st, 'et', { 'a::1': 'o1', 'b::2': 'o2', 'c::3': 'o3' }).id, 'c::3');
assert.strictEqual(E.newestDraft(st, 'et', {}), null);
assert.strictEqual(E.newestDraft(st, 'de', { 'a::1': 'o1' }), null);
E.clearDraft(st, 'et', 'c::3');
assert.strictEqual(E.loadDraft(st, 'et', 'c::3', 'o3'), null);

// a broken storage never throws
const bad = { getItem() { throw new Error('no'); }, setItem() { throw new Error('no'); }, removeItem() { throw new Error('no'); }, length: 1, key() { throw new Error('no'); } };
assert.strictEqual(E.saveDraft(bad, 'et', 'x', 'a', 'b'), false);
assert.strictEqual(E.loadDraft(bad, 'et', 'x', 'b'), null);
assert.strictEqual(E.newestDraft(bad, 'et', { x: 'b' }), null);
E.clearDraft(bad, 'et', 'x');
console.log('editing ok');
