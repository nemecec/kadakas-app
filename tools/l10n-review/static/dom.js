// The only way the page builds elements. Text children become text nodes, never markup.
// Attributes: a fixed list of names, each set through a property or a fixed attribute name.
// Events: functions only. No style strings, no href, no src, no srcdoc, no on* strings.
(function (root) {
  'use strict';

  // Properties set from a string value.
  var STRING_PROPS = { id: 'id', title: 'title', value: 'value', placeholder: 'placeholder', type: 'type',
    name: 'name', lang: 'lang', role: 'role' };
  var NUMBER_PROPS = { rows: 'rows', cols: 'cols', size: 'size', tabindex: 'tabIndex', colspan: 'colSpan' };
  var BOOL_PROPS = { disabled: 'disabled', checked: 'checked', selected: 'selected', hidden: 'hidden',
    draggable: 'draggable', spellcheck: 'spellcheck', open: 'open' };
  // CSS properties that code may set, one by one, from the css object.
  var CSS_PROPS = { whiteSpace: 1, fontWeight: 1, color: 1, width: 1, display: 1 };
  var DATA_RE = /^data-[a-z][a-z0-9-]*$/;

  function reject(name) {
    if (typeof console !== 'undefined') console.error('dom.h: attribute not allowed: ' + name);
  }

  function makeH(doc) {
    function setAttr(e, k, v) {
      if (v === false || v == null) return;
      if (k === 'class') { e.className = String(v); return; }
      if (k === 'for') { e.htmlFor = String(v); return; }
      if (k === 'css') {
        if (typeof v !== 'object') return reject('css');
        Object.keys(v).forEach(function (p) {
          if (CSS_PROPS[p] === 1) e.style[p] = String(v[p]); else reject('css.' + p);
        });
        return;
      }
      if (k.slice(0, 2) === 'on') {
        if (typeof v === 'function') e.addEventListener(k.slice(2), v); else reject(k + ' (not a function)');
        return;
      }
      if (Object.prototype.hasOwnProperty.call(STRING_PROPS, k)) { e[STRING_PROPS[k]] = String(v); return; }
      if (Object.prototype.hasOwnProperty.call(NUMBER_PROPS, k)) { e[NUMBER_PROPS[k]] = Number(v); return; }
      if (Object.prototype.hasOwnProperty.call(BOOL_PROPS, k)) { e[BOOL_PROPS[k]] = v === true || v === 'true'; return; }
      if (DATA_RE.test(k)) {
        e.dataset[k.slice(5).replace(/-([a-z0-9])/g, function (m, c) { return c.toUpperCase(); })] = String(v);
        return;
      }
      if (k === 'aria-label') { e.setAttribute('aria-label', String(v)); return; }
      if (k === 'aria-pressed') { e.setAttribute('aria-pressed', v === true || v === 'true' ? 'true' : 'false'); return; }
      reject(k);
    }

    return function h(tag, attrs, kids) {
      var e = doc.createElement(tag);
      Object.keys(attrs || {}).forEach(function (k) { setAttr(e, k, attrs[k]); });
      [].concat(kids == null ? [] : kids).forEach(function (c) {
        e.appendChild(typeof c === 'string' ? doc.createTextNode(c) : c);
      });
      return e;
    };
  }

  var api = { makeH: makeH };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Dom = api;
})(typeof window !== 'undefined' ? window : this);
