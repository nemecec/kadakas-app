// Offset logic for the "Weird" highlights. No DOM needed for the pure parts, so node can test it.
// All offsets count UTF-16 code units of the plain text, like JavaScript strings do.
(function (root) {
  'use strict';

  // Sort highlights, clamp them to the text, drop overlaps and empty ones.
  function cleanHighlights(text, highlights) {
    var hs = (highlights || []).map(function (h) {
      return { start: Math.max(0, h.start), end: Math.min(text.length, h.end) };
    }).filter(function (h) { return h.end > h.start; });
    hs.sort(function (a, b) { return a.start - b.start || a.end - b.end; });
    var out = [], last = 0;
    hs.forEach(function (h) {
      if (h.start >= last) { out.push(h); last = h.end; }
    });
    return out;
  }

  // Cut the text into plain and highlighted pieces. Pieces cover the text without gaps.
  function segments(text, highlights) {
    var out = [], pos = 0;
    cleanHighlights(text, highlights).forEach(function (h) {
      if (h.start > pos) out.push({ start: pos, end: h.start, text: text.slice(pos, h.start), hl: false });
      out.push({ start: h.start, end: h.end, text: text.slice(h.start, h.end), hl: true });
      pos = h.end;
    });
    if (pos < text.length || out.length === 0) out.push({ start: pos, end: text.length, text: text.slice(pos), hl: false });
    return out;
  }

  // Order the ends, clamp, trim white space. Returns null when nothing is left.
  function normalizeRange(text, start, end) {
    if (start > end) { var t = start; start = end; end = t; }
    start = Math.max(0, start);
    end = Math.min(text.length, end);
    while (start < end && /\s/.test(text.charAt(start))) start++;
    while (end > start && /\s/.test(text.charAt(end - 1))) end--;
    return end > start ? { start: start, end: end } : null;
  }

  // Map a DOM point (node, offset) to a text offset. `rootEl` holds one element per piece,
  // each with data-s (start) and one text node. rootEl has data-len (text length).
  function pointToOffset(node, off, rootEl) {
    if (node === rootEl) {
      var kids = rootEl.childNodes;
      if (off >= kids.length) return Number(rootEl.dataset.len);
      return Number(kids[off].dataset.s);
    }
    var el = node;
    while (el && !(el.dataset && el.dataset.s !== undefined)) {
      if (el === rootEl) return null;
      el = el.parentNode;
    }
    if (!el) return null;
    var base = Number(el.dataset.s);
    if (node.nodeType === 3) return base + off;
    // an element point: off counts child nodes, 0 is the start, more is the end of the piece
    return off === 0 ? base : base + Number(el.dataset.len);
  }

  // Selection range (anchor/focus nodes) to {start,end}, or null.
  function selectionToRange(rootEl, sel, text) {
    if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return null;
    var r = sel.getRangeAt(0);
    var a = pointToOffset(r.startContainer, r.startOffset, rootEl);
    var b = pointToOffset(r.endContainer, r.endOffset, rootEl);
    if (a === null || b === null) return null;
    return normalizeRange(text, a, b);
  }

  // Word diff for suggestion cards: pieces of `neu` marked changed when not in `old`.
  function diffWords(old, neu) {
    var re = /\s+|[^\s]+/g;
    var a = old.match(re) || [], b = neu.match(re) || [];
    var n = a.length, m = b.length, i, j;
    var L = [];
    for (i = 0; i <= n; i++) { L.push(new Array(m + 1).fill(0)); }
    for (i = n - 1; i >= 0; i--) for (j = m - 1; j >= 0; j--)
      L[i][j] = a[i] === b[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    var out = [];
    i = 0; j = 0;
    while (j < m) {
      if (i < n && a[i] === b[j]) { out.push({ text: b[j], changed: false }); i++; j++; }
      else if (i < n && L[i + 1][j] >= L[i][j + 1]) i++;
      else { out.push({ text: b[j], changed: !/^\s+$/.test(b[j]) }); j++; }
    }
    return out;
  }

  var api = { cleanHighlights: cleanHighlights, segments: segments, normalizeRange: normalizeRange,
    pointToOffset: pointToOffset, selectionToRange: selectionToRange, diffWords: diffWords };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Offsets = api;
})(typeof window !== 'undefined' ? window : this);
