'use strict';
(function () {
  var O = window.Offsets, Ed = window.Editing;
  var S = { target: new URLSearchParams(location.search).get('target') || '', data: null, byId: {}, sel: null,
    shown: 300, filtered: [], tab: 'list', queue: null, jobErrors: {}, open: {} };
  var $ = function (id) { return document.getElementById(id); };

  function h(tag, attrs, kids) {
    var e = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === 'class') e.className = attrs[k];
      else if (k.slice(0, 2) === 'on') e.addEventListener(k.slice(2), attrs[k]);
      else if (attrs[k] !== false && attrs[k] != null) e.setAttribute(k, attrs[k] === true ? '' : attrs[k]);
    });
    [].concat(kids == null ? [] : kids).forEach(function (c) {
      e.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    });
    return e;
  }
  function toast(msg, undo) {
    var t = $('toast'); t.textContent = msg; t.hidden = false;
    if (undo) { t.appendChild(h('button', { class: 'link', style: 'color:inherit', onclick: function () { t.hidden = true; undo(); } }, 'Undo')); }
    clearTimeout(toast.t); toast.t = setTimeout(function () { t.hidden = true; }, undo ? 8000 : 3500);
  }
  var TOKEN = (document.querySelector('meta[name="l10n-token"]') || {}).content || '';
  function request(path, body) {
    var q = 'target=' + encodeURIComponent(S.target);
    var url = '/api/' + path + (path.indexOf('?') < 0 ? '?' : '&') + q;
    var headers = { 'X-L10n-Token': TOKEN };
    var opt = { headers: headers };
    if (body !== undefined) { opt.method = 'POST'; headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
    return fetch(url, opt);
  }
  function api(path, body) {
    return request(path, body).then(function (r) { return r.json().then(function (j) { if (!r.ok) throw new Error(j.error || r.status); return j; }); });
  }
  function fail(e) { toast(e.message || String(e)); }

  // Never rebuild the panel while the owner types in it.
  function editing() {
    var a = document.activeElement;
    return !!(a && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName) && a.closest && a.closest('#detail-cell, .editor'));
  }
  function renderSafe() {
    if (editing()) { S.pendingRender = true; return; }
    S.pendingRender = false; counts(); renderRows(); fillDetail();
  }
  document.addEventListener('focusout', function () { setTimeout(function () { if (S.pendingRender && !editing()) renderSafe(); }, 0); });

  // ---- loading
  function load() {
    return api('list').then(function (d) {
      S.data = d; S.target = d.target; S.byId = {};
      d.messages.forEach(function (m) { S.byId[m.id] = m; });
      var ts = $('target'); ts.textContent = '';
      d.locales.forEach(function (l) { ts.appendChild(h('option', { value: l, selected: l === d.target }, l)); });
      $('th-target').textContent = d.target;
      fillFilters(); renderAll();
    }).catch(fail);
  }
  function fillFilters() {
    var f = $('f-file'), cur = f.value;
    f.textContent = ''; f.appendChild(h('option', { value: '' }, 'all files'));
    S.data.files.forEach(function (x) { f.appendChild(h('option', { value: x.name }, x.name)); });
    f.value = cur;
    var s = $('f-status'), cs = s.value;
    s.textContent = ''; s.appendChild(h('option', { value: '' }, 'all'));
    S.data.statuses.forEach(function (x) { s.appendChild(h('option', { value: x }, x)); });
    s.value = cs;
  }
  function counts() {
    var c = {}; S.data.messages.forEach(function (m) { c[m.status] = (c[m.status] || 0) + 1; });
    S.data.counts = c;
    $('counts').textContent = S.data.total + ' messages, ' + (c.flagged || 0) + ' flagged, ' + (c.suggested || 0) +
      ' suggested, ' + (c.accepted || 0) + ' accepted, ' + (c.fixed || 0) + ' fixed';
  }
  function apply(it) {  // merge a fresh item from the server
    var old = S.byId[it.id];
    if (old) { Object.keys(it).forEach(function (k) { old[k] = it[k]; }); }
    return old || it;
  }

  // ---- list
  function filter() {
    var file = $('f-file').value, st = $('f-status').value, q = $('f-text').value.trim().toLowerCase(), df = $('f-diff').checked;
    S.filtered = S.data.messages.filter(function (m) {
      if (file && m.file !== file) return false;
      if (st && m.status !== st) return false;
      if (df && !m.suggestions.some(function (s) { return s.state !== 'rejected' && m.tr && s.text !== m.tr.raw; })) return false;
      if (q) {
        var t = ((m.en ? m.en.raw : '') + '\n' + (m.tr ? m.tr.raw : '') + '\n' + m.key).toLowerCase();
        if (t.indexOf(q) < 0) return false;
      }
      return true;
    });
  }
  function renderAll() {
    counts();
    if (S.tab === 'list') renderRows();
    else if (S.tab === 'terms') renderTerms();
    else if (S.tab === 'glossary') renderGlossary();
    else renderParity();
  }
  function renderRows() {
    filter();
    var ta0 = S.edit && S.edit.ta, had = !!(ta0 && document.activeElement === ta0), ss = ta0 ? ta0.selectionStart : 0, se = ta0 ? ta0.selectionEnd : 0;
    var tb = document.querySelector('#rows tbody'); tb.textContent = '';
    S.filtered.slice(0, S.shown).forEach(function (m) { tb.appendChild(rowFor(m)); if (S.sel === m.id) tb.appendChild(detailFor(m)); });
    $('more-rows').hidden = S.filtered.length <= S.shown;
    if (had && S.edit.ta.isConnected) { S.edit.ta.focus(); S.edit.ta.setSelectionRange(ss, se); }
  }
  function trNode(m) {
    var text = m.tr ? m.tr.raw : '';
    var box = h('div', { class: 'trtext', 'data-len': String(text.length), 'data-id': m.id, ondblclick: function () { openEdit(m.id); } });
    if (!m.tr) { box.textContent = '(missing)'; return box; }
    O.segments(text, m.highlights).forEach(function (s) {
      var el = h(s.hl ? 'mark' : 'span', { 'data-s': String(s.start), 'data-len': String(s.end - s.start) }, s.text);
      if (s.hl) { el.title = 'Click to remove'; el.addEventListener('click', function (ev) {
          ev.stopPropagation();
          if (!window.getSelection().isCollapsed) return;   // the click ends a drag-select
          post('highlight', { id: m.id, remove: true, start: s.start, end: s.end }).then(function () {
            toast('Highlight removed', function () { post('highlight', { id: m.id, start: s.start, end: s.end }); });
          });
        }); }
      box.appendChild(el);
    });
    return box;
  }
  function rowFor(m) {
    var tr = h('tr', { class: 'msg' + (S.sel === m.id ? ' sel' : ''), 'data-id': m.id, onclick: function () { if (S.sel === m.id || !window.getSelection().isCollapsed) return; select(m.id, false); } }, [
      h('td', { class: 'k' }, [h('div', { class: 'f' }, m.file), h('div', {}, m.key), m.parity !== 'ok' ? h('span', { class: 'badge bad' }, m.parity) : '', m.problems.length ? h('span', { class: 'badge bad' }, m.problems.join(', ')) : '',
        m.tr ? h('div', {}, h('button', { class: 'link', title: 'Edit the text in place (key e)', onclick: function (ev) { ev.stopPropagation(); openEdit(m.id); } }, 'Edit')) : '']),
      h('td', { class: 'en' }, m.en ? m.en.raw : '(no English)'),
      h('td', { class: 'tr' }, S.edit && S.edit.id === m.id ? S.edit.el : trNode(m)),
      h('td', { class: 'st' }, h('span', { class: 'badge ' + m.status }, m.status))]);
    return tr;
  }
  function select(id, scroll) {
    S.sel = S.sel === id && !scroll ? S.sel : id;
    renderRows();
    var el = document.querySelector('tr.msg[data-id="' + CSS.escape(id) + '"]');
    if (el && scroll) el.scrollIntoView({ block: 'center' });
    api('item?id=' + encodeURIComponent(id)).then(function (it) { apply(it); fillDetail(); }).catch(fail);
    loadUsage(id);
  }
  function post(path, body) {
    return api(path, body).then(function (it) { if (it && it.id) apply(it); renderSafe(); return it; }).catch(fail);
  }

  // ---- in-place editor
  function store() {
    try { return window.localStorage || null; } catch (e) { return null; }
  }
  var NOSTORE = { length: 0, key: function () { return null; }, getItem: function () { return null; }, setItem: function () {}, removeItem: function () {} };
  function st() { return store() || NOSTORE; }
  function openEdit(id) {
    var m = S.byId[id];
    if (!m || !m.tr) return toast('Nothing to edit');
    if (S.edit && S.edit.id === id) { if (S.edit.ta) S.edit.ta.focus(); return; }
    S.edit = null;   // the draft of another edit stays in localStorage
    api('item?id=' + encodeURIComponent(id)).then(function (it) {
      apply(it);
      var d = Ed.loadDraft(st(), S.target, id, it.tr.raw);
      startEdit(S.byId[id], d ? d.text : it.tr.raw, !!d);
    }).catch(fail);
  }
  function startEdit(m, text, restored) {
    S.sel = m.id;
    S.edit = { id: m.id, orig: m.tr.raw, hash: m.tr.hash, text: text, seq: 0 };
    S.edit.el = buildEditor(m, text);
    renderRows(); fillDetail();
    S.edit.ta.focus();
    var row = document.querySelector('tr.msg[data-id="' + CSS.escape(m.id) + '"]'); if (row) row.scrollIntoView({ block: 'center' });
    validateNow();
    if (restored) toast('Restored your unsaved edit');
  }
  function buildEditor(m, text) {
    var ta = h('textarea', { class: 'ed-ta', lang: S.target, spellcheck: 'true', rows: String(Math.max(2, text.split('\n').length + 1)) });
    ta.value = text;
    var msg = h('div', { class: 'ed-msg muted' }, '');
    var bSug = h('button', { onclick: function () { saveEdit(false); } }, 'Save as suggestion');
    var bAcc = h('button', { class: 'primary', onclick: function () { saveEdit(true); } }, 'Save and accept');
    var chips = h('div', { class: 'chips' }, [h('span', { class: 'muted' }, 'Insert: ')].concat((m.chips || []).map(function (c) {
      return h('button', { class: 'chip', type: 'button', title: 'Insert at the cursor', onmousedown: function (e) { e.preventDefault(); }, onclick: function () { insertChip(c); } }, c);
    })));
    var words = (m.highlights || []).map(function (x) { return x.text; });
    var box = h('div', { class: 'editor', onclick: function (e) { e.stopPropagation(); } }, [
      h('div', { class: 'ed-en' }, [h('div', { class: 'muted' }, 'English'), h('div', { class: 'ed-en-text' }, m.en ? m.en.raw : '')]),
      words.length ? wordsLine(words) : '',
      ta, chips, msg,
      h('div', { class: 'actions' }, [bSug, bAcc, h('button', { onclick: cancelEdit }, 'Cancel'), h('span', { class: 'muted' }, 'Ctrl/Cmd+Enter saves and accepts, Esc cancels')])]);
    S.edit.ta = ta; S.edit.msg = msg; S.edit.bSug = bSug; S.edit.bAcc = bAcc;
    ta.addEventListener('input', onEditInput);
    ta.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); saveEdit(true); }
      else if (e.key === 'Escape') { e.preventDefault(); cancelEdit(); }
      e.stopPropagation();
    });
    return box;
  }
  function wordsLine(words) {
    var kids = [h('span', { class: 'muted' }, 'Flagged words: ')];
    words.forEach(function (w) { kids.push(h('mark', {}, w), ' '); });
    return h('div', { class: 'ed-words' }, kids);
  }
  function onEditInput() {
    var e = S.edit; if (!e) return;
    e.text = e.ta.value;
    Ed.saveDraft(st(), S.target, e.id, e.text, e.orig);
    e.ta.rows = Math.max(2, e.text.split('\n').length + 1);
    clearTimeout(onEditInput.t); onEditInput.t = setTimeout(validateNow, 300);
  }
  function showEditMsg(text, cls) { if (S.edit) { S.edit.msg.textContent = text; S.edit.msg.className = 'ed-msg ' + cls; } }
  function validateNow() {
    var e = S.edit; if (!e) return;
    var seq = ++e.seq, text = e.ta.value;
    if (!Ed.isChanged(text, e.orig)) { showEditMsg('No change yet.', 'muted'); e.bSug.disabled = e.bAcc.disabled = true; return; }
    e.bSug.disabled = e.bAcc.disabled = false;
    api('validate', { id: e.id, text: text }).then(function (r) {
      if (!S.edit || S.edit.seq !== seq) return;
      if (r.error) showEditMsg('Invalid: ' + r.error, 'err'); else showEditMsg('Valid. Variables, placeables and structure fit.', 'ok');
    }).catch(function (er) { showEditMsg(er.message, 'err'); });
  }
  function insertChip(chip) {
    var e = S.edit; if (!e) return;
    var r = Ed.insertAtCursor(e.ta.value, e.ta.selectionStart, e.ta.selectionEnd, chip);
    e.ta.value = r.text; e.ta.focus(); e.ta.setSelectionRange(r.cursor, r.cursor);
    onEditInput();
  }
  function saveEdit(accept) {
    var e = S.edit; if (!e) return;
    var text = e.ta.value;
    if (!Ed.isChanged(text, e.orig)) { toast('No change, nothing saved'); closeEdit(true); return; }
    api('owner-edit', { id: e.id, text: text, base_hash: e.hash, accept: accept }).then(function (it) {
      Ed.clearDraft(st(), S.target, e.id);
      S.edit = null; apply(it); counts(); renderRows(); fillDetail();
      toast(accept ? 'Saved and accepted' : 'Saved as a suggestion');
    }).catch(function (er) { showEditMsg(er.message, 'err'); });
  }
  function closeEdit(clear) {
    if (S.edit && clear) Ed.clearDraft(st(), S.target, S.edit.id);
    S.edit = null; counts(); renderRows(); fillDetail();
  }
  function cancelEdit() {
    var e = S.edit; if (!e) return;
    if (Ed.isChanged(e.ta.value, e.orig) && !confirm('Discard your edit?')) { e.ta.focus(); return; }
    closeEdit(true);
  }
  function restoreDraft() {
    var cur = {};
    S.data.messages.forEach(function (m) { if (m.tr) cur[m.id] = m.tr.raw; });
    var d = Ed.newestDraft(st(), S.target, cur);
    if (d && !S.edit) {
      api('item?id=' + encodeURIComponent(d.id)).then(function (it) { apply(it); startEdit(S.byId[d.id], d.text, true); }).catch(fail);
    }
  }

  // ---- detail panel
  function detailFor(m) {
    return h('tr', { class: 'detail' }, h('td', { colspan: '4', id: 'detail-cell' }));
  }
  function fillDetail() {
    var cell = $('detail-cell'); var m = S.byId[S.sel];
    if (!cell || !m) return;
    var oldCm = cell.querySelector('textarea.cm'), keepFocus = document.activeElement === oldCm, pos = oldCm ? oldCm.selectionStart : 0;
    cell.textContent = '';
    cell.appendChild(contextBlock(m));
    cell.appendChild(actionsBlock(m));
    cell.appendChild(cardsBlock(m));
    if (keepFocus) { var n = cell.querySelector('textarea.cm'); n.focus(); n.setSelectionRange(pos, pos); }
  }
  function fileGroup(m) { return m.group_en || ''; }
  function contextBlock(m) {
    var c = h('div', { class: 'ctx' });
    var l = h('div'), r = h('div');
    l.appendChild(h('h4', {}, 'File'));
    l.appendChild(h('div', {}, m.file + (fileGroup(m) ? ': ' + fileGroup(m) : '')));
    if (m.en && m.en.comment) { l.appendChild(h('h4', {}, 'Comment above the message')); l.appendChild(h('div', {}, m.en.comment)); }
    if (m.en && m.en.variables.length) {
      l.appendChild(h('h4', {}, 'Variables (example value)'));
      l.appendChild(h('div', {}, m.en.variables.map(function (v) { return '$' + v + ' = ' + m.en.examples[v]; }).join(',  ')));
    }
    if (m.en && m.en.selector) {
      l.appendChild(h('h4', {}, 'Variants of ' + m.en.selector));
      l.appendChild(h('div', {}, m.en.variants.map(function (v) { return (v.default ? '*' : '') + '[' + v.key + '] ' + v.text; }).join('\n')));
      l.lastChild.style.whiteSpace = 'pre-wrap';
    }
    l.appendChild(h('h4', {}, 'Neighbouring keys'));
    var det = h('details', S.open.neigh ? { open: true } : {}, [h('summary', {}, (m.neighbours || []).length + ' keys')]);
    det.addEventListener('toggle', function () { S.open.neigh = det.open; });
    (m.neighbours || []).forEach(function (n) {
      det.appendChild(h('div', { class: 'muted', style: n.here ? 'font-weight:bold' : '' }, [n.key + ': ' + n.en.replace(/\n/g, ' ') + '  |  ' + n.tr.replace(/\n/g, ' ')]));
    });
    l.appendChild(det);
    r.appendChild(h('h4', {}, 'Where it is used'));
    r.appendChild(h('div', { id: 'usage-box', class: 'muted' }, 'Searching...'));
    c.appendChild(l); c.appendChild(r);
    return c;
  }
  function loadUsage(id) {
    api('usage?id=' + encodeURIComponent(id)).then(function (u) {
      var box = $('usage-box'); if (!box || S.sel !== id) return;
      box.textContent = '';
      if (!u.uses.length) { box.textContent = 'No use found in crates/**/*.rs.'; return; }
      u.uses.forEach(function (x) {
        box.appendChild(h('div', {}, x.path + ':' + x.line + (x.dynamic ? '  (name built at run time, may match)' : '')));
        box.appendChild(h('pre', { class: 'snip' }, x.snippet));
      });
      if (u.total > u.uses.length) box.appendChild(h('div', {}, '... and ' + (u.total - u.uses.length) + ' more'));
    }).catch(function () {});
  }
  function actionsBlock(m) {
    var wrap = h('div');
    wrap.appendChild(h('textarea', { class: 'cm', placeholder: 'Comment (what is wrong?)', oninput: function (e) { saveComment(m.id, e.target.value); }, onchange: function (e) { saveComment(m.id, e.target.value, true); } }, m.comment));
    var busy = S.queue && S.queue.status === 'running';
    var a = h('div', { class: 'actions' }, [
      h('button', { onclick: weird, title: 'Select words in the target text first (key w)' }, 'Weird'),
      h('button', { disabled: m.status !== 'unreviewed' && m.status !== 'flagged', onclick: toggleFlag }, m.status === 'unreviewed' ? 'Flag' : 'Unflag'),
      h('button', { class: 'primary', disabled: busy || !m.tr, onclick: function () { suggest([m.id], false); } }, 'Suggest'),
      h('button', { disabled: busy || !m.suggestions.length, onclick: function () { suggest([m.id], true); } }, 'More options'),
      h('button', { class: 'link', onclick: copyPrompt }, 'Copy prompt'),
      h('span', { class: 'muted' }, 'Runs claude -p on this machine and uses your Claude usage.')]);
    wrap.appendChild(a);
    return wrap;
  }
  function saveComment(id, value, now) {
    clearTimeout(saveComment.t);
    var m = S.byId[id]; if (m) m.comment = value;
    var send = function () { api('comment', { id: id, comment: value }).then(function (it) { var o = S.byId[id]; if (o) { o.status = it.status; } counts(); }).catch(fail); };
    if (now) send(); else saveComment.t = setTimeout(send, 600);
  }
  function toggleFlag() {
    var m = S.byId[S.sel]; if (!m) return;
    if (m.status === 'unreviewed') post('status', { id: m.id, status: 'flagged' });
    else if (m.status === 'flagged') post('status', { id: m.id, status: 'unreviewed' });
    else toast('The status is ' + m.status + '. Flag and unflag only switch between unreviewed and flagged.');
  }
  function weird() {
    var m = S.byId[S.sel]; if (!m || !m.tr) return toast('Select a row first');
    var box = document.querySelector('tr.msg[data-id="' + CSS.escape(m.id) + '"] .trtext');
    var r = O.selectionToRange(box, window.getSelection(), m.tr.raw);
    if (!r) return toast('Select words in the target text of this row first');
    window.getSelection().removeAllRanges();
    post('highlight', { id: m.id, start: r.start, end: r.end });
  }
  function copyPrompt() {
    var m = S.byId[S.sel];
    api('prompt?id=' + encodeURIComponent(m.id)).then(function (p) {
      var done = function () { toast('Prompt copied'); };
      if (navigator.clipboard && window.isSecureContext !== false) navigator.clipboard.writeText(p.prompt).then(done, function () { fallbackCopy(p.prompt); });
      else fallbackCopy(p.prompt);
    }).catch(fail);
  }
  function fallbackCopy(text) {
    var ta = h('textarea', {}, text); document.body.appendChild(ta); ta.select();
    try { document.execCommand('copy'); toast('Prompt copied'); } catch (e) { toast('Copy failed'); }
    ta.remove();
  }

  // ---- suggestion cards
  function cardsBlock(m) {
    var wrap = h('div', { class: 'cards' });
    var q = S.queue;
    if (q && q.status === 'running' && q.current === m.id) {
      wrap.appendChild(h('div', { class: 'card' }, [h('span', { class: 'spin' }), h('span', { id: 'spin-text' }, ' Asking Claude (' + q.model + '), ' + q.item_elapsed + ' s')]));
    }
    var je = S.jobErrors[m.id];
    if (je) {
      wrap.appendChild(h('div', { class: 'errbox' }, je.message + (je.code != null ? '\nExit code: ' + je.code : '') + (je.stderr ? '\n' + je.stderr : '') + '\nCopy prompt below still works.'));
    }
    m.suggestions.forEach(function (s) { wrap.appendChild(card(m, s)); });
    var own = h('div', { class: 'actions' }, [h('input', { id: 'own-sug', size: '60', placeholder: 'Type or paste your own suggestion' }),
      h('button', { onclick: function () { var v = $('own-sug').value; if (v.trim()) post('suggestion', { id: m.id, op: 'add', text: v }); } }, 'Add')]);
    wrap.appendChild(own);
    return wrap;
  }
  function card(m, s) {
    var cur = m.tr ? m.tr.raw : '';
    var txt = h('div', { class: 'txt' });
    O.diffWords(cur, s.text).forEach(function (p) { txt.appendChild(p.changed ? h('span', { class: 'chg' }, p.text) : document.createTextNode(p.text)); });
    var c = h('div', { class: 'card ' + s.state + (s.error ? ' invalid' : '') });
    c.appendChild(txt);
    if (s.why) c.appendChild(h('div', { class: 'why' }, s.why + '  (' + s.source + ')'));
    else c.appendChild(h('div', { class: 'why' }, s.source));
    if (s.error) c.appendChild(h('div', { class: 'err' }, 'Invalid: ' + s.error));
    var btns = h('div', { class: 'actions' });
    if (s.state === 'accepted') btns.appendChild(h('span', { class: 'badge accepted' }, 'accepted'));
    if (s.state === 'applied') btns.appendChild(h('span', { class: 'badge fixed' }, 'applied'));
    if (s.state === 'pending' || s.state === 'rejected') {
      btns.appendChild(h('button', { class: 'primary', disabled: !!s.error, onclick: function () { post('suggestion', { id: m.id, sid: s.id, op: 'accept' }); } }, 'Use this'));
    }
    if (s.state !== 'applied') {
      btns.appendChild(h('button', { onclick: function () { editCard(m, s, c, txt, btns); } }, 'Edit'));
    }
    if (s.state === 'pending' || s.state === 'accepted') btns.appendChild(h('button', { onclick: function () { post('suggestion', { id: m.id, sid: s.id, op: 'reject' }); } }, 'Reject'));
    if (s.state === 'rejected') btns.appendChild(h('button', { class: 'link', onclick: function () { post('suggestion', { id: m.id, sid: s.id, op: 'delete' }); } }, 'Delete'));
    c.appendChild(btns);
    return c;
  }
  function editCard(m, s, c, txt, btns) {
    var ta = h('textarea', { class: 'cm' }, s.text);
    txt.replaceWith(ta);
    btns.textContent = '';
    btns.appendChild(h('button', { class: 'primary', onclick: function () {
      api('suggestion', { id: m.id, sid: s.id, op: 'edit', text: ta.value }).then(function (it) {
        apply(it);
        var ns = it.suggestions.filter(function (x) { return x.id === s.id; })[0];
        if (ns && !ns.error) return post('suggestion', { id: m.id, sid: s.id, op: 'accept' });
        fillDetail(); toast('Saved, but the text is invalid: ' + (ns && ns.error));
      }).catch(fail);
    } }, 'Save and use'));
    btns.appendChild(h('button', { onclick: fillDetail }, 'Cancel'));
    ta.focus();
  }

  // ---- queue
  function suggest(ids, more) {
    delete S.jobErrors[ids[0]];
    api('queue', { ids: ids, model: $('model').value, more: more }).then(function (j) { S.queue = j; pollQueue(); fillDetail(); }).catch(fail);
  }
  function pollQueue() {
    clearTimeout(pollQueue.t);
    api('queue').then(function (j) {
      var prev = S.queue; S.queue = j;
      var running = j.status === 'running';
      $('btn-cancel').hidden = !running; $('queue-progress').hidden = !running && j.status !== 'done';
      $('queue-progress').max = j.total || 1; $('queue-progress').value = j.done || 0;
      $('queue-status').textContent = running ? 'Asking ' + (j.done + 1) + ' of ' + j.total + ' (' + j.model + ', ' + j.elapsed + ' s)' : (j.status === 'idle' ? '' : j.status);
      $('btn-all').disabled = running;
      var errNew = false;
      Object.keys(j.errors || {}).forEach(function (k) { if (!S.jobErrors[k]) errNew = true; S.jobErrors[k] = j.errors[k]; });
      S.seenFinished = S.seenFinished || {};
      var todo = (j.finished || []).filter(function (id) { return !S.seenFinished[id + ':' + j.started]; });
      var changed = todo.length > 0 || errNew || !prev || prev.current !== j.current || prev.status !== j.status || prev.started !== j.started;
      var chain = Promise.resolve();
      todo.forEach(function (id) { S.seenFinished[id + ':' + j.started] = 1; chain = chain.then(function () { return api('item?id=' + encodeURIComponent(id)).then(apply); }); });
      chain.then(function () {
        if (changed) renderSafe();
        else { var sp = document.getElementById('spin-text'); if (sp) sp.textContent = ' Asking Claude (' + j.model + '), ' + j.item_elapsed + ' s'; }
      });
      if (running) pollQueue.t = setTimeout(pollQueue, 1000);
    }).catch(fail);
  }
  $('btn-all').addEventListener('click', function () {
    var ids = S.data.messages.filter(function (m) { return m.status === 'flagged' && m.tr; }).map(function (m) { return m.id; });
    if (!ids.length) return toast('No flagged messages');
    if (!confirm('Ask Claude for ' + ids.length + ' flagged messages, one after another? This uses your Claude usage.')) return;
    S.queue = null; S.jobErrors = {}; S.seenFinished = {};
    api('queue', { ids: ids, model: $('model').value }).then(function (j) { S.queue = j; pollQueue(); }).catch(fail);
  });
  $('btn-cancel').addEventListener('click', function () { api('queue/cancel', {}).then(function () { pollQueue(); }); });

  // ---- terms, glossary, parity
  function renderTerms() {
    var box = $('tab-terms'); box.textContent = 'Working...';
    api('terms').then(function (t) {
      box.textContent = '';
      box.appendChild(h('p', { class: 'muted' }, 'Heuristic. It counts words that occur together, so some hits are noise. Target words are cut to 5 letters to ignore endings.'));
      box.appendChild(h('h3', {}, 'Glossary check'));
      t.glossary.forEach(function (g) {
        var row = h('div', {}, g.en + ' -> ' + g.tr + ': ' + g.messages + ' messages, ' + g.missing_count + ' without the fixed term');
        box.appendChild(row);
        g.missing.slice(0, 8).forEach(function (id) { box.appendChild(h('div', { class: 'muted' }, h('a', { href: '#', onclick: function (e) { e.preventDefault(); jump(id); } }, id))); });
      });
      box.appendChild(h('h3', {}, 'English words with several target renderings'));
      var tb = h('table'); tb.appendChild(h('tr', {}, [h('th', {}, 'English word'), h('th', {}, 'Messages'), h('th', {}, 'Renderings (stem: messages)')]));
      t.inconsistent.forEach(function (r) {
        tb.appendChild(h('tr', {}, [h('td', {}, r.word), h('td', {}, String(r.messages)),
          h('td', {}, r.renderings.map(function (x) { return x.stem + '...: ' + x.count; }).join('   ') + '   ')]));
        var last = tb.lastChild.lastChild;
        r.renderings.forEach(function (x) { x.ids.slice(0, 2).forEach(function (id) { last.appendChild(h('a', { href: '#', class: 'muted', onclick: function (e) { e.preventDefault(); jump(id); } }, id + ' ')); }); });
      });
      box.appendChild(tb);
    }).catch(fail);
  }
  function jump(id) { setTab('list'); $('f-file').value = ''; $('f-status').value = ''; $('f-text').value = ''; var i = S.data.messages.findIndex(function (m) { return m.id === id; }); S.shown = Math.max(S.shown, i + 20); S.sel = id; renderRows(); select(id, true); }
  function renderGlossary() {
    var box = $('tab-glossary'); box.textContent = '';
    var items = S.data.glossary.map(function (g) { return { en: g.en, tr: g.tr, note: g.note }; });
    box.appendChild(h('p', { class: 'muted' }, 'Fixed term pairs. Every Claude prompt includes them.'));
    var tb = h('table', { class: 'gl' });
    tb.appendChild(h('tr', {}, [h('th', {}, 'English'), h('th', {}, S.data.target), h('th', {}, 'Note'), h('th')]));
    function draw() {
      [].slice.call(tb.querySelectorAll('tr.it')).forEach(function (r) { r.remove(); });
      items.forEach(function (it, i) {
        tb.appendChild(h('tr', { class: 'it' }, [['en', 'tr', 'note'].map(function (k) { return h('td', {}, h('input', { value: it[k], oninput: function (e) { it[k] = e.target.value; } })); }),
          h('td', {}, h('button', { onclick: function () { items.splice(i, 1); draw(); } }, 'Remove'))].reduce(function (a, b) { return a.concat(b); }, [])));
      });
    }
    draw(); box.appendChild(tb);
    box.appendChild(h('div', { class: 'actions' }, [h('button', { onclick: function () { items.push({ en: '', tr: '', note: '' }); draw(); } }, 'Add term'),
      h('button', { class: 'primary', onclick: function () { api('glossary', { items: items }).then(function (r) { S.data.glossary = r.glossary; toast('Glossary saved'); renderGlossary(); }).catch(fail); } }, 'Save glossary')]));
  }
  function renderParity() {
    var box = $('tab-parity'); box.textContent = ''; var p = S.data.parity;
    box.appendChild(h('p', {}, p.missing.length + ' keys missing in ' + S.data.target + ', ' + p.extra.length + ' extra. Files missing: ' + (p.missing_files.join(', ') || 'none') + '. Extra files: ' + (p.extra_files.join(', ') || 'none') + '.'));
    p.missing.concat(p.extra).forEach(function (x) { box.appendChild(h('div', {}, x.file + '  ' + x.key)); });
  }

  // ---- apply and export
  $('btn-apply').addEventListener('click', function () {
    api('apply/preview', {}).then(function (pv) {
      var d = $('dlg'); d.textContent = '';
      d.appendChild(h('h3', {}, 'Apply accepted suggestions'));
      if (!pv.changes.length) d.appendChild(h('p', {}, 'Nothing accepted yet.'));
      pv.changes.forEach(function (c) {
        d.appendChild(h('div', {}, c.file + ':' + c.line + '  ' + c.key));
        if (c.error) d.appendChild(h('div', { class: 'errbox' }, 'Refused: ' + c.error));
        else d.appendChild(h('div', { class: 'diff' }, c.old_lines.map(function (l) { return h('span', { class: 'del' }, '- ' + l); }).concat(c.new_lines.map(function (l) { return h('span', { class: 'add' }, '+ ' + l); }))));
      });
      var ok = pv.changes.filter(function (c) { return !c.error; }).length;
      var chk = h('input', { type: 'checkbox' });
      var risky = pv.dirty_unknown || (pv.dirty && pv.dirty.length);
      if (risky) d.appendChild(h('p', { class: 'errbox' }, [(pv.dirty_unknown ? 'Git cannot tell whether the target files have uncommitted changes. ' : 'The target files have uncommitted changes: ' + pv.dirty.join(', ') + '. '), h('label', {}, [chk, ' I know, apply anyway'])]));
      d.appendChild(h('div', { class: 'actions' }, [
        h('button', { class: 'primary', disabled: !ok, onclick: function () {
          if (risky && !chk.checked) return toast('Tick the box to apply over uncommitted changes');
          api('apply', { confirm_dirty: chk.checked, expect: pv.digest }).then(function (r) { d.close(); if (r.refused) toast(r.refused === 'changed' ? 'The files or suggestions changed since the preview. Open Apply again.' : 'Refused: ' + r.refused); else toast((r.applied || []).length + ' messages written'); load(); }).catch(fail);
        } }, 'Apply ' + ok + ' changes'), h('button', { onclick: function () { d.close(); } }, 'Close')]));
      d.showModal();
    }).catch(fail);
  });
  $('btn-export').addEventListener('click', function () {
    request('export').then(function (r) { return r.blob(); }).then(function (b) {
      var a = h('a', { href: URL.createObjectURL(b), download: 'review-' + S.target + '.md' });
      document.body.appendChild(a); a.click(); a.remove();
    }).catch(fail);
  });

  // ---- wiring
  function setTab(t) {
    S.tab = t;
    ['list', 'terms', 'glossary', 'parity'].forEach(function (n) { $('tab-' + n).hidden = n !== t; });
    [].forEach.call(document.querySelectorAll('#tabs button'), function (b) { b.classList.toggle('on', b.dataset.tab === t); });
    $('filters').hidden = t !== 'list';
    renderAll();
  }
  [].forEach.call(document.querySelectorAll('#tabs button'), function (b) { b.addEventListener('click', function () { setTab(b.dataset.tab); }); });
  ['f-file', 'f-status', 'f-diff'].forEach(function (id) { $(id).addEventListener('change', function () { S.shown = 300; renderRows(); }); });
  $('f-text').addEventListener('input', function () { S.shown = 300; renderRows(); });
  $('more-rows').addEventListener('click', function () { S.shown += 300; renderRows(); });
  $('target').addEventListener('change', function (e) { S.target = e.target.value; S.sel = null; S.edit = null; load().then(restoreDraft); });
  document.addEventListener('keydown', function (e) {
    var tag = (e.target.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea' || tag === 'select' || e.metaKey || e.ctrlKey || e.altKey || S.tab !== 'list' || $('dlg').open) return;
    var i = S.filtered.findIndex(function (m) { return m.id === S.sel; });
    if (e.key === 'j' || e.key === 'k') {
      var n = Math.min(S.filtered.length - 1, Math.max(0, i + (e.key === 'j' ? 1 : -1)));
      if (S.filtered[n]) { if (n >= S.shown) S.shown = n + 20; S.sel = null; select(S.filtered[n].id, true); }
    } else if (e.key === 'f') toggleFlag();
    else if (e.key === 'c') { var t = document.querySelector('textarea.cm'); if (t) { e.preventDefault(); t.focus(); } }
    else if (e.key === 'a') { var m = S.byId[S.sel]; if (m && m.tr && ($('no-ask').checked || confirm('Ask Claude for this message? This uses your Claude usage.'))) suggest([m.id], false); }
    else if (e.key === 'w') weird();
    else if (e.key === 'e') { if (S.sel) { e.preventDefault(); openEdit(S.sel); } }
    else return;
  });
  load().then(function () { pollQueue(); restoreDraft(); });
})();
