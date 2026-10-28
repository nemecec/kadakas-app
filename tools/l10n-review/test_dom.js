'use strict';
// Run: node tools/l10n-review/test_dom.js
const assert = require('assert');
const Dom = require('./static/dom.js');

// A fake document that records everything an element receives.
function fakeDoc() {
  return {
    createElement(tag) {
      const e = { tag, attrs: {}, props: {}, listeners: [], children: [], style: {}, dataset: {}, className: '',
        setAttribute(k, v) { this.attrs[k] = v; }, addEventListener(n, f) { this.listeners.push([n, f]); },
        appendChild(c) { this.children.push(c); return c; } };
      return new Proxy(e, { set(t, k, v) { if (!(k in t) || ['className'].includes(k)) t[k] = v; else t[k] = v; if (!['className', 'attrs', 'props', 'listeners', 'children', 'style', 'dataset'].includes(k)) t.props[k] = v; return true; } });
    },
    createTextNode(data) { return { nodeType: 3, data }; },
  };
}
const errors = [];
const origError = console.error;
console.error = (m) => errors.push(m);
const h = Dom.makeH(fakeDoc());

// hostile names and values are rejected
const e1 = h('a', { href: 'javascript:alert(1)', src: 'x', srcdoc: '<script>1</script>', style: 'background:url(javascript:1)',
  formaction: 'javascript:1', onclick: 'alert(1)', onerror: 'alert(1)', 'xlink:href': 'javascript:1', action: 'x', css: { background: 'url(x)' } }, 'hi');
assert.deepStrictEqual(e1.attrs, {});
assert.deepStrictEqual(e1.props, {});
assert.deepStrictEqual(e1.style, {});
assert.deepStrictEqual(e1.listeners, []);
assert.ok(errors.length >= 9, 'each rejected name is reported: ' + errors.length);
assert.ok(errors.some(m => /href/.test(m)) && errors.some(m => /srcdoc/.test(m)) && errors.some(m => /onclick/.test(m)));

// events only as functions
const f = () => {};
const e2 = h('button', { onclick: f, onmousedown: 'steal()' });
assert.deepStrictEqual(e2.listeners, [['click', f]]);

// hostile TEXT stays text
const evil = '<img src=x onerror=alert(1)></div><script>alert(1)</script>';
const e3 = h('div', {}, [evil, h('span', {}, evil)]);
assert.deepStrictEqual(e3.children[0], { nodeType: 3, data: evil });
assert.deepStrictEqual(e3.children[1].children[0], { nodeType: 3, data: evil });
assert.strictEqual(e3.children[1].tag, 'span');   // never parsed into other elements
assert.strictEqual(e3.children.length, 2);

// whitelisted attributes with hostile values are inert strings in properties or fixed attributes
const e4 = h('input', { title: evil, value: evil, placeholder: evil, id: evil, 'data-id': evil, 'aria-label': evil, class: evil, type: 'text',
  'aria-pressed': true, rows: '3', disabled: true, checked: false, spellcheck: 'true', colspan: '4', tabindex: '0' });
assert.strictEqual(e4.props.title, evil);
assert.strictEqual(e4.props.value, evil);
assert.strictEqual(e4.props.placeholder, evil);
assert.strictEqual(e4.props.id, evil);
assert.strictEqual(e4.dataset.id, evil);
assert.strictEqual(e4.attrs['aria-label'], evil);
assert.strictEqual(e4.attrs['aria-pressed'], 'true');
assert.strictEqual(e4.className, evil);
assert.strictEqual(e4.props.rows, 3);
assert.strictEqual(e4.props.disabled, true);
assert.strictEqual(e4.props.spellcheck, true);
assert.strictEqual(e4.props.colSpan, 4);
assert.strictEqual(e4.props.tabIndex, 0);
assert.ok(!('checked' in e4.props));   // false means not set
assert.deepStrictEqual(Object.keys(e4.attrs).sort(), ['aria-label', 'aria-pressed']);

// data-* names must be plain, other aria names are not allowed
errors.length = 0;
const e5 = h('div', { 'data-a b': '1', 'data-': '1', 'DATA-x': '1', 'aria-describedby': 'x', 'data-s': '5', 'data-multi-word': '2' });
assert.deepStrictEqual(e5.dataset, { s: '5', multiWord: '2' });
assert.deepStrictEqual(e5.attrs, {});
assert.strictEqual(errors.length, 4);

// a css object sets whitelisted properties one by one
errors.length = 0;
const e6 = h('div', { css: { whiteSpace: 'pre-wrap', background: 'url(javascript:1)', color: 'red' } });
assert.deepStrictEqual(e6.style, { whiteSpace: 'pre-wrap', color: 'red' });
assert.strictEqual(errors.length, 1);

// null and false values are skipped, kids may be missing
const e7 = h('div', { title: null, hidden: false });
assert.deepStrictEqual(e7.props, {});
assert.deepStrictEqual(e7.children, []);

// the page code has none of the usual sinks
const fs = require('fs');
for (const name of ['app.js', 'editing.js', 'offsets.js', 'dom.js']) {
  const src = fs.readFileSync(__dirname + '/static/' + name, 'utf8');
  for (const bad of [/\.innerHTML/, /\.outerHTML/, /insertAdjacentHTML/, /document\.write/, /\beval\(/, /new Function/, /window\.open/, /setTimeout\(\s*['"`]/, /\.setAttribute\(\s*[a-z]+\s*,/i]) {
    const found = src.split('\n').filter(l => bad.test(l) && !/^\s*\/\//.test(l));
    assert.deepStrictEqual(found, [], name + ' has ' + bad + ': ' + found.join(' | '));
  }
}
console.error = origError;
console.log('dom ok');
