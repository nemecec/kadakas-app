'use strict';
(function () {
  var O = window.Offsets, Ed = window.Editing;
  var S = { target: '',   // set from the server answer, never from the address bar
     data: null, byId: {}, sel: null,
    shown: 300, filtered: [], tab: 'list', prev: {}, sampleVer: 0, queue: null, jobErrors: {}, open: {} };
  var $ = function (id) { return document.getElementById(id); };

  var h = window.Dom.makeH(document);
  function toast(msg, undo) {
    var t = $('toast'); t.textContent = msg; t.hidden = false;
    if (undo) { t.appendChild(h('button', { class: 'link inherit', onclick: function () { t.hidden = true; undo(); } }, 'Undo')); }
    clearTimeout(toast.t); toast.t = setTimeout(function () { t.hidden = true; }, undo ? 8000 : 3500);
  }
  var TOKEN = (document.querySelector('meta[name="l10n-token"]') || {}).content || '';
  function request(path, body, keepalive) {
    var q = 'target=' + encodeURIComponent(S.target);
    var url = '/api/' + path + (path.indexOf('?') < 0 ? '?' : '&') + q;
    var headers = { 'X-L10n-Token': TOKEN };
    var opt = { headers: headers };
    if (body !== undefined) { opt.method = 'POST'; headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
    if (keepalive) opt.keepalive = true;
    return fetch(url, opt);
  }
  // Every request that changes the state file is tracked by the Saved indicator.
  var STATE_PATHS = ['status', 'comment', 'highlight', 'suggestion', 'owner-edit', 'glossary', 'apply', 'bulk-ok', 'bulk-undo', 'samples', 'samples/reset'];
  var SAVE = { pending: 0, failed: [], last: '', typing: false };
  function clock() { var d = new Date(), p = function (n) { return (n < 10 ? '0' : '') + n; }; return p(d.getHours()) + ':' + p(d.getMinutes()) + ':' + p(d.getSeconds()); }
  function showSave() {
    var el = $('save-ind'); if (!el) return;
    el.textContent = ''; el.className = 'save-ind';
    if (SAVE.failed.length) {
      el.classList.add('bad');
      el.appendChild(document.createTextNode('NOT SAVED: ' + SAVE.failed[SAVE.failed.length - 1].err + ' '));
      el.appendChild(h('button', { onclick: retrySave }, 'Retry'));
    } else if (SAVE.pending > 0 || SAVE.typing) el.textContent = 'Saving...';
    else if (SAVE.last) el.textContent = 'Saved ' + SAVE.last;
    else el.textContent = 'Nothing changed yet';
  }
  function retrySave() {
    var items = SAVE.failed; SAVE.failed = []; showSave();
    items.forEach(function (f) { api(f.path, f.body).catch(fail); });
  }
  function api(path, body) {
    var tracked = body !== undefined && STATE_PATHS.indexOf(path) >= 0;
    if (tracked) { SAVE.pending++; showSave(); }
    var done = function () { if (tracked) { SAVE.pending--; } };
    return request(path, body).then(function (r) {
      return r.json().then(function (j) {
        if (!r.ok) { var e = new Error(j.error || r.status); e.status = r.status; throw e; }
        done(); if (tracked) { SAVE.last = clock(); showSave(); }
        return j;
      });
    }).catch(function (e) {
      if (tracked) {
        done();
        // A refusal (4xx) is a mistake of the owner, not a failed save. A network error or 5xx is a failed save.
        if (!e.status || e.status >= 500) SAVE.failed.push({ path: path, body: body, err: e.message || 'network error' });
        showSave();
      }
      throw e;
    });
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
    s.appendChild(h('option', { value: 'attention' }, 'needs attention'));
    s.value = cs;
  }
  var ATTENTION = ['flagged', 'suggested', 'accepted'];
  function counts() {
    var c = {}, per = {};
    S.data.messages.forEach(function (m) {
      c[m.status] = (c[m.status] || 0) + 1;
      var p = per[m.file] || (per[m.file] = { un: 0, total: 0 });
      p.total++; if (m.status === 'unreviewed') p.un++;
    });
    S.data.counts = c;
    var total = S.data.total, done = total - (c.unreviewed || 0);
    $('progress').textContent = 'Reviewed ' + done + ' of ' + total + ' (' + (total ? Math.round(100 * done / total) : 0) + '%)';
    $('counts').textContent = (c.ok || 0) + ' ok, ' + (c.flagged || 0) + ' flagged, ' + (c.suggested || 0) +
      ' suggested, ' + (c.accepted || 0) + ' accepted, ' + (c.fixed || 0) + ' fixed';
    [].forEach.call($('f-file').options, function (o) {
      if (!o.value) { o.textContent = 'all files (' + (c.unreviewed || 0) + ' / ' + total + ')'; return; }
      var p = per[o.value] || { un: 0, total: 0 };
      o.textContent = o.value + ' (' + p.un + ' / ' + p.total + ')';
    });
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
      if (st === 'attention') { if (ATTENTION.indexOf(m.status) < 0) return false; }
      else if (st && m.status !== st) return false;
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
    fetchPreviews();
  }
  function trNode(m) {
    var text = m.tr ? m.tr.raw : '';
    var box = h('div', { class: 'trtext', 'data-len': String(text.length), 'data-id': m.id });
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
  function svgIcon(paths, width) {
    var ns = 'http://www.w3.org/2000/svg';
    var svg = document.createElementNS(ns, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('width', '14'); svg.setAttribute('height', '14');
    svg.setAttribute('fill', 'none'); svg.setAttribute('stroke', 'currentColor'); svg.setAttribute('stroke-width', width || '2');
    svg.setAttribute('stroke-linecap', 'round'); svg.setAttribute('stroke-linejoin', 'round'); svg.setAttribute('aria-hidden', 'true');
    paths.forEach(function (d) { var p = document.createElementNS(ns, 'path'); p.setAttribute('d', d); svg.appendChild(p); });
    return svg;
  }
  function pencilButton(m, loc) {
    var b = h('button', { class: 'pencil', type: 'button', 'aria-label': loc === 'en' ? 'Edit English' : 'Edit',
      title: loc === 'en' ? 'Edit the English source text (E)' : 'Edit this text (e)',
      onclick: function (ev) { ev.stopPropagation(); openEdit(m.id, loc); } });
    b.appendChild(svgIcon(['M12 20h9', 'M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z']));
    return b;
  }
  function okButton(m) {
    var on = m.status === 'ok';
    var b = h('button', { class: 'okbtn' + (on ? ' on' : ''), type: 'button', 'aria-label': 'Mark OK', 'aria-pressed': on ? 'true' : 'false',
      title: on ? 'Marked OK. Click to undo (o)' : 'Looks good (o)', onclick: function (ev) { ev.stopPropagation(); S.sel = m.id; toggleOk(); } });
    b.appendChild(svgIcon(['M4 12.5l5 5L20 6.5'], '2.4'));
    b.appendChild(document.createTextNode(' OK'));
    return b;
  }
  function flagButton(m) {
    var can = m.status === 'unreviewed' || m.status === 'ok' || m.status === 'flagged';
    return h('button', { class: 'rowbtn', type: 'button', disabled: !can, title: can ? 'Flag or unflag (f)' : 'The status is ' + m.status,
      onclick: function (ev) { ev.stopPropagation(); S.sel = m.id; toggleFlag(); } }, m.status === 'flagged' ? 'Unflag' : 'Flag');
  }
  function chosenSuggestion(m) { return m.suggestions.filter(function (x) { return x.id === m.chosen; })[0] || null; }
  function acceptButton(m) {
    var c = chosenSuggestion(m);
    return h('button', { class: 'rowbtn acc', type: 'button', disabled: !c,
      title: c ? 'Accept the chosen suggestion for Apply: ' + c.text.slice(0, 60) : 'Add or choose a valid suggestion first',
      onclick: function (ev) { ev.stopPropagation(); S.sel = m.id; post('suggestion', { id: m.id, sid: c.id, op: 'accept' }); } }, 'Accept');
  }
  function statusCell(m) {
    var open = m.suggestions.filter(function (x) { return x.state === 'pending'; }).length;
    var kids = [h('span', { class: 'badge ' + m.status }, m.status)];
    if (m.status === 'ok' && open) kids.push(' ', h('span', { class: 'badge suggested', title: 'Open suggestions on an OK message' }, open + ' open'));
    return [h('div', {}, kids), h('div', { class: 'rowacts' }, [okButton(m), flagButton(m), acceptButton(m)])];
  }
  // Preview lines below a text, filled from the cache. The server renders them with sample values.
  function previewNode(m, which) {
    var box = h('div', { class: 'preview pv-' + which, 'data-id': m.id });
    paintInto(box, m, which);
    return box;
  }
  function paintInto(box, m, which) {
    box.textContent = '';
    var c = S.prev[m.id]; var data = c && c.sig === prevSig(m) ? c.data[which] : null;
    if (!data) return;
    box.appendChild(h('div', { class: 'muted pv-title' }, 'Preview'));
    data.lines.forEach(function (l) {
      var line = h('div', { class: 'pv-line' + (l.errors.length ? ' bad' : '') }, [l.label ? h('span', { class: 'pv-label' }, l.label + ' → ') : '', l.text]);
      if (l.errors.length) line.title = l.errors.join('; ');
      box.appendChild(line);
    });
    if (data.truncated) box.appendChild(h('div', { class: 'muted' }, 'More combinations are not shown.'));
  }
  function prevSig(m) { return (m.en ? m.en.hash : '') + ':' + (m.tr ? m.tr.hash : '') + ':' + S.sampleVer; }
  function paintPreviews(ids) {
    ids.forEach(function (id) {
      var m = S.byId[id]; if (!m) return;
      [].forEach.call(document.querySelectorAll('.preview[data-id="' + CSS.escape(id) + '"]'), function (box) { paintInto(box, m, box.classList.contains('pv-en') ? 'en' : 'tr'); });
    });
  }
  function fetchPreviews() {
    var want = S.filtered.slice(0, S.shown).filter(function (m) {
      var c = S.prev[m.id];
      return !(c && c.sig === prevSig(m)) && ((m.en && m.en.raw.indexOf('{') >= 0) || (m.tr && m.tr.raw.indexOf('{') >= 0));
    }).map(function (m) { return m.id; });
    for (var i = 0; i < want.length; i += 150) {
      (function (ids) {
        api('preview', { ids: ids }).then(function (r) {
          ids.forEach(function (id) { var m = S.byId[id]; if (m) S.prev[id] = { sig: prevSig(m), data: r.previews[id] || {} }; });
          S.data.samples = r.samples; paintPreviews(ids);
        }).catch(function () {});
      })(want.slice(i, i + 150));
    }
  }
  function enCell(m) {
    var wrap = h('div', { class: 'trwrap' }, [h('div', { class: 'entext' }, m.en ? m.en.raw : '(no English)')]);
    if (m.en) wrap.appendChild(h('div', { class: 'rowbtns' }, [pencilButton(m, 'en')]));
    if (m.en) wrap.appendChild(previewNode(m, 'en'));
    return wrap;
  }
  function changeNode(m) {
    var c = m.en_change; if (!c) return '';
    var diff = h('span');
    O.diffWords(c.old, c.new).forEach(function (p) { diff.appendChild(p.changed ? h('span', { class: 'chg' }, p.text) : document.createTextNode(p.text)); });
    return h('div', { class: 'enchange' }, [h('div', { class: 'muted' }, 'English changed on ' + c.date + '. Check this text.'), h('div', { class: 'entext' }, ['Before: ' + c.old]), h('div', { class: 'entext' }, ['Now: ', diff])]);
  }
  function trCell(m) {
    var wrap = h('div', { class: 'trwrap' }, [trNode(m)]);
    if (m.tr) wrap.appendChild(h('div', { class: 'rowbtns' }, [pencilButton(m, 'tr')]));
    var kids = [wrap];
    if (m.en_change) kids.push(changeNode(m));
    if (m.tr) kids.push(previewNode(m, 'tr'));
    return h('div', {}, kids);
  }
  function rowFor(m) {
    var editEn = S.edit && S.edit.id === m.id && S.edit.loc === 'en', editTr = S.edit && S.edit.id === m.id && S.edit.loc === 'tr';
    var tr = h('tr', { class: 'msg' + (S.sel === m.id ? ' sel' : ''), 'data-id': m.id, onclick: function () { if (S.sel === m.id || !window.getSelection().isCollapsed) return; select(m.id, false); } }, [
      h('td', { class: 'k' }, [h('div', { class: 'f' }, m.file), h('div', {}, m.key), m.parity !== 'ok' ? h('span', { class: 'badge bad' }, m.parity) : '', m.problems.length ? h('span', { class: 'badge bad' }, m.problems.join(', ')) : '']),
      h('td', { class: 'en' }, editEn ? S.edit.el : enCell(m)),
      h('td', { class: 'tr' }, editTr ? S.edit.el : trCell(m)),
      h('td', { class: 'st' }, statusCell(m))]);
    return tr;
  }
  function select(id, scroll) {
    flushComment();
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
  function editKey(id, loc) { return id + '|' + loc; }
  function openEdit(id, loc) {
    loc = loc || 'tr';
    var m = S.byId[id];
    if (!m || !(loc === 'en' ? m.en : m.tr)) return toast('Nothing to edit');
    if (S.edit && S.edit.id === id && S.edit.loc === loc) { if (S.edit.ta) S.edit.ta.focus(); return; }
    S.edit = null;   // the draft of another edit stays in localStorage
    api('item?id=' + encodeURIComponent(id)).then(function (it) {
      apply(it);
      var cur = loc === 'en' ? it.en : it.tr;
      var d = Ed.loadDraft(st(), S.target, editKey(id, loc), cur.raw);
      startEdit(S.byId[id], loc, d ? d.text : cur.raw, !!d);
    }).catch(fail);
  }
  function startEdit(m, loc, text, restored) {
    S.sel = m.id;
    var cur = loc === 'en' ? m.en : m.tr;
    S.edit = { id: m.id, loc: loc, locale: loc === 'en' ? S.data.source : S.target, key: editKey(m.id, loc), orig: cur.raw, hash: cur.hash, text: text, seq: 0 };
    S.edit.el = buildEditor(m, loc, text);
    renderRows(); fillDetail();
    S.edit.ta.focus();
    var row = document.querySelector('tr.msg[data-id="' + CSS.escape(m.id) + '"]'); if (row) row.scrollIntoView({ block: 'center' });
    validateNow();
    if (restored) toast('Restored your unsaved edit');
  }
  function buildEditor(m, loc, text) {
    var ta = h('textarea', { class: 'ed-ta', lang: loc === 'en' ? 'en' : S.target, spellcheck: 'true', rows: String(Math.max(2, text.split('\n').length + 1)) });
    ta.value = text;
    var msg = h('div', { class: 'ed-msg muted' }, '');
    var warn = h('div', { class: 'ed-warn' }, '');
    var pv = h('div', { class: 'preview ed-preview' }, '');
    var bSug = h('button', { onclick: function () { saveEdit(false); } }, 'Save as suggestion');
    var bAcc = h('button', { class: 'primary', onclick: function () { saveEdit(true); } }, 'Save and accept');
    var chipList = (loc === 'en' ? m.chips_en : m.chips) || [];
    var chips = h('div', { class: 'chips' }, [h('span', { class: 'muted' }, 'Insert: ')].concat(chipList.map(function (c) {
      return h('button', { class: 'chip', type: 'button', title: 'Insert at the cursor', onmousedown: function (e) { e.preventDefault(); }, onclick: function () { insertChip(c); } }, c);
    })));
    var words = loc === 'tr' ? (m.highlights || []).map(function (x) { return x.text; }) : [];
    var ctx = loc === 'en'
      ? h('div', { class: 'ed-en' }, [h('div', { class: 'muted' }, 'Editing the English source. The ' + S.target + ' text for context:'), h('div', { class: 'ed-en-text' }, m.tr ? m.tr.raw : '(missing)'),
          h('div', { class: 'muted' }, 'A new variable is refused, because the code does not pass it. Removing one is allowed. Then the ' + S.target + ' text is flagged for you.')])
      : h('div', { class: 'ed-en' }, [h('div', { class: 'muted' }, 'English'), h('div', { class: 'ed-en-text' }, m.en ? m.en.raw : '')]);
    var box = h('div', { class: 'editor', onclick: function (e) { e.stopPropagation(); } }, [
      ctx, words.length ? wordsLine(words) : '', ta, chips, msg, warn, pv,
      h('div', { class: 'actions' }, [bSug, bAcc, h('button', { onclick: cancelEdit }, 'Cancel'), h('span', { class: 'muted' }, 'Ctrl/Cmd+Enter saves and accepts, Esc cancels')])]);
    S.edit.ta = ta; S.edit.msg = msg; S.edit.warn = warn; S.edit.pv = pv; S.edit.bSug = bSug; S.edit.bAcc = bAcc;
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
    Ed.saveDraft(st(), S.target, e.key, e.text, e.orig);
    e.ta.rows = Math.max(2, e.text.split('\n').length + 1);
    clearTimeout(onEditInput.t); onEditInput.t = setTimeout(validateNow, 300);
  }
  function showEditMsg(text, cls) { if (S.edit) { S.edit.msg.textContent = text; S.edit.msg.className = 'ed-msg ' + cls; } }
  function validateNow() {
    var e = S.edit; if (!e) return;
    var seq = ++e.seq, text = e.ta.value;
    if (!Ed.isChanged(text, e.orig)) { showEditMsg('No change yet.', 'muted'); e.warn.textContent = ''; e.bSug.disabled = e.bAcc.disabled = true; }
    else {
      e.bSug.disabled = e.bAcc.disabled = false;
      api('validate', { id: e.id, text: text, locale: e.locale }).then(function (r) {
        if (!S.edit || S.edit.seq !== seq) return;
        if (r.error) showEditMsg('Invalid: ' + r.error, 'err'); else showEditMsg('Valid. Variables, placeables and structure fit.', 'ok');
        e.warn.textContent = (r.warnings || []).join(' ');
      }).catch(function (er) { showEditMsg(er.message, 'err'); });
    }
    if (text.indexOf('{') >= 0) {
      api('preview-text', { id: e.id, text: text, locale: e.locale }).then(function (r) {
        if (!S.edit || S.edit.seq !== seq) return;
        e.pv.textContent = '';
        e.pv.appendChild(h('div', { class: 'muted pv-title' }, 'Preview'));
        r.lines.forEach(function (l) { e.pv.appendChild(h('div', { class: 'pv-line' + (l.errors.length ? ' bad' : '') }, [l.label ? h('span', { class: 'pv-label' }, l.label + ' → ') : '', l.text])); });
      }).catch(function () {});
    } else if (e.pv) e.pv.textContent = '';
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
    api('owner-edit', { id: e.id, text: text, locale: e.locale, base_hash: e.hash, accept: accept }).then(function (it) {
      Ed.clearDraft(st(), S.target, e.key);
      S.edit = null; apply(it); counts(); renderRows(); fillDetail();
      toast(accept ? 'Saved and accepted' : 'Saved as a suggestion');
    }).catch(function (er) { showEditMsg(er.message, 'err'); });
  }
  function closeEdit(clear) {
    if (S.edit && clear) Ed.clearDraft(st(), S.target, S.edit.key);
    S.edit = null; counts(); renderRows(); fillDetail();
  }
  function cancelEdit() {
    var e = S.edit; if (!e) return;
    if (Ed.isChanged(e.ta.value, e.orig) && !confirm('Discard your edit?')) { e.ta.focus(); return; }
    closeEdit(true);
  }
  function restoreDraft() {
    var cur = {};
    S.data.messages.forEach(function (m) { if (m.tr) cur[editKey(m.id, 'tr')] = m.tr.raw; if (m.en) cur[editKey(m.id, 'en')] = m.en.raw; });
    var d = Ed.newestDraft(st(), S.target, cur);
    if (d && !S.edit) {
      var i = d.id.lastIndexOf('|'), id = d.id.slice(0, i), loc = d.id.slice(i + 1);
      api('item?id=' + encodeURIComponent(id)).then(function (it) { apply(it); startEdit(S.byId[id], loc, d.text, true); }).catch(fail);
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
  // Remembered open or closed state of the collapsible sections, per browser.
  function uiGet(name) { try { return window.localStorage.getItem('l10n-ui:' + name) === '1'; } catch (e) { return false; } }
  function uiSet(name, on) { try { window.localStorage.setItem('l10n-ui:' + name, on ? '1' : '0'); } catch (e) {} }
  function section(name, summary, kids) {
    var d = h('details', uiGet(name) ? { open: true, 'data-sec': name } : { 'data-sec': name }, [h('summary', {}, summary)].concat(kids || []));
    d.addEventListener('toggle', function () { uiSet(name, d.open); });
    return d;
  }
  function contextBlock(m) {
    var c = h('div', { class: 'ctx' });
    var l = h('div'), r = h('div');
    l.appendChild(h('h4', {}, 'File'));
    l.appendChild(h('div', {}, m.file + (fileGroup(m) ? ': ' + fileGroup(m) : '')));
    if (m.en && m.en.comment) { l.appendChild(h('h4', {}, 'Comment above the message')); l.appendChild(h('div', {}, m.en.comment)); }
    if (m.en && m.en.selector) {
      l.appendChild(h('h4', {}, 'Variants of ' + m.en.selector));
      l.appendChild(h('div', { class: 'prewrap' }, m.en.variants.map(function (v) { return (v.default ? '*' : '') + '[' + v.key + '] ' + v.text; }).join('\n')));
    }
    var rows = m.sample_rows || [];
    if (rows.length) {
      l.appendChild(h('h4', {}, 'Samples for the preview'));
      var line = h('div', { class: 'samples' });
      rows.forEach(function (row) {
        line.appendChild(h('label', {}, [row.name + ' ', h('input', { size: String(Math.max(4, (row.value || row.default).length)), value: row.value, placeholder: row.default,
          title: 'Sample value for $' + row.name + '. It counts for all messages. Empty means the default.',
          onchange: function (e) { setSample(row.name, e.target.value); } })]));
      });
      line.appendChild(h('button', { class: 'link', onclick: function () { api('samples/reset', {}).then(function (r2) { S.data.samples = r2.samples; sampleChanged(); select(S.sel, true); }).catch(fail); } }, 'Reset samples'));
      l.appendChild(line);
    }
    var nb = (m.neighbours || []).map(function (n) {
      return h('div', { class: n.here ? 'muted bold' : 'muted' }, [n.key + ': ' + n.en.replace(/\n/g, ' ') + '  |  ' + n.tr.replace(/\n/g, ' ')]);
    });
    r.appendChild(section('neigh', 'Neighbours (' + Math.max(0, (m.neighbours || []).length - 1) + ')', nb));
    r.appendChild(section('usage', 'Used in ... places', [h('div', { id: 'usage-box', class: 'muted' }, 'Searching...')]));
    c.appendChild(l); c.appendChild(r);
    return c;
  }
  function setSample(name, value) {
    api('samples', { name: name, value: value }).then(function (r) { S.data.samples = r.samples; sampleChanged(); }).catch(fail);
  }
  function sampleChanged() { S.sampleVer++; S.prev = {}; renderSafe(); }
  function loadUsage(id) {
    api('usage?id=' + encodeURIComponent(id)).then(function (u) {
      var box = $('usage-box'); if (!box || S.sel !== id) return;
      var sum = box.parentNode.querySelector('summary');
      sum.textContent = u.total ? 'Used in ' + u.total + (u.total === 1 ? ' place' : ' places') : 'Not used in the Rust sources';
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
      h('button', { class: 'primary', disabled: busy || !m.tr, onclick: function () { suggest([m.id], false); } }, 'Suggest'),
      h('button', { disabled: busy || !m.suggestions.length, onclick: function () { suggest([m.id], true); } }, 'More options'),
      h('button', { disabled: busy || !m.en, title: 'Ask Claude for a better English source text', onclick: function () { suggest([m.id], false, S.data.source); } }, 'Suggest English'),
      h('button', { class: 'link', onclick: copyPrompt }, 'Copy prompt'),
      h('span', { class: 'muted' }, 'Runs claude -p on this machine and uses your Claude usage.')]);
    wrap.appendChild(a);
    return wrap;
  }
  function saveComment(id, value, now) {
    clearTimeout(saveComment.t);
    var m = S.byId[id]; if (m) m.comment = value;
    saveComment.pending = { id: id, value: value };
    SAVE.typing = true; showSave();
    var send = function () {
      saveComment.pending = null; SAVE.typing = false;
      api('comment', { id: id, comment: value }).then(function (it) { var o = S.byId[id]; if (o) { o.status = it.status; } counts(); }).catch(fail);
    };
    if (now) send(); else saveComment.t = setTimeout(send, 600);
  }
  // Send a waiting comment at once: on row change, tab change and when the page goes away.
  function flushComment(keepalive) {
    var p = saveComment.pending; if (!p) return;
    clearTimeout(saveComment.t); saveComment.pending = null; SAVE.typing = false;
    if (keepalive) { request('comment', { id: p.id, comment: p.value }, true).catch(function () {}); return; }
    api('comment', { id: p.id, comment: p.value }).catch(fail);
  }
  window.addEventListener('pagehide', function () { flushComment(true); });
  document.addEventListener('visibilitychange', function () { if (document.visibilityState === 'hidden') flushComment(true); });

  function toggleFlag() {
    var m = S.byId[S.sel]; if (!m) return;
    if (m.status === 'unreviewed' || m.status === 'ok') post('status', { id: m.id, status: 'flagged' });
    else if (m.status === 'flagged') post('status', { id: m.id, status: 'unreviewed' });
    else toast('The status is ' + m.status + '. Flag and unflag switch between unreviewed, ok and flagged only.');
  }
  // The next (dir 1) or previous (dir -1) unreviewed row of the shown list, after row `fromId`.
  function nextUnreviewed(dir, fromId) {
    var i = S.filtered.findIndex(function (m) { return m.id === fromId; });
    for (var k = i < 0 ? (dir > 0 ? 0 : S.filtered.length - 1) : i + dir; k >= 0 && k < S.filtered.length; k += dir) {
      if (S.filtered[k].status === 'unreviewed') return S.filtered[k].id;
    }
    return null;
  }
  function goTo(id) {
    var n = S.filtered.findIndex(function (m) { return m.id === id; });
    if (n < 0) return;
    if (n >= S.shown) S.shown = n + 20;
    S.sel = null; select(id, true);
  }
  function toggleOk() {
    var m = S.byId[S.sel]; if (!m) return;
    if (m.status === 'ok') {
      post('status', { id: m.id, status: 'unreviewed' }).then(function (it) {
        if (it) toast('Back to unreviewed', function () { post('status', { id: m.id, status: 'ok' }); });
      });
      return;
    }
    if (m.status === 'accepted' || m.status === 'fixed') return toast('The status is ' + m.status + '. It cannot be marked OK.');
    var prev = m.status, next = nextUnreviewed(1, m.id);
    post('status', { id: m.id, status: 'ok' }).then(function (it) {
      if (!it) return;
      toast('Marked OK', function () { post('status', { id: m.id, status: prev }); });
      if (next) goTo(next);
    });
  }
  function markAllShownOk() {
    filter();
    var ids = S.filtered.filter(function (m) { return m.status === 'unreviewed'; }).map(function (m) { return m.id; });
    if (!ids.length) return toast('No unreviewed messages are shown');
    if (!confirm('Mark ' + ids.length + ' shown messages as OK?')) return;
    api('bulk-ok', { ids: ids }).then(function (r) {
      r.changed.forEach(function (id) { if (S.byId[id]) S.byId[id].status = 'ok'; });
      renderSafe();
      toast('Marked ' + r.count + ' as OK', function () {
        api('bulk-undo', { ids: r.changed }).then(function (u) {
          u.changed.forEach(function (id) { if (S.byId[id]) S.byId[id].status = 'unreviewed'; });
          renderSafe();
        }).catch(fail);
      });
    }).catch(fail);
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
    var isEn = s.locale === S.data.source;
    var cur = (isEn ? m.en : m.tr) ? (isEn ? m.en.raw : m.tr.raw) : '';
    var txt = h('div', { class: 'txt' });
    O.diffWords(cur, s.text).forEach(function (p) { txt.appendChild(p.changed ? h('span', { class: 'chg' }, p.text) : document.createTextNode(p.text)); });
    var chosen = m.chosen === s.id && s.state === 'pending';
    var c = h('div', { class: 'card ' + s.state + (s.error ? ' invalid' : '') + (chosen ? ' chosen' : '') });
    var head = h('div', { class: 'cardhead' }, [h('span', { class: 'badge' }, isEn ? 'English' : S.target)]);
    if (chosen) head.appendChild(h('span', { class: 'badge accepted', title: 'The Accept button of the row takes this one' }, 'chosen'));
    if (s.stale_en) head.appendChild(h('span', { class: 'badge bad', title: 'The English text changed after this suggestion was made. Apply refuses it.' }, 'made for older English'));
    if (s.stale_base) head.appendChild(h('span', { class: 'badge bad', title: 'The text in the file changed after this suggestion was made. Apply refuses it.' }, 'text changed since'));
    c.appendChild(head);
    c.appendChild(txt);
    c.appendChild(h('div', { class: 'why' }, (s.why ? s.why + '  (' : '(') + s.source + ')'));
    if (s.error) c.appendChild(h('div', { class: 'err' }, 'Invalid: ' + s.error));
    var btns = h('div', { class: 'actions' });
    if (s.state === 'accepted') {
      btns.appendChild(h('span', { class: 'badge accepted' }, 'accepted'));
      btns.appendChild(h('button', { class: 'link', onclick: function () { post('suggestion', { id: m.id, sid: s.id, op: 'pending' }); } }, 'Withdraw'));
    }
    if (s.state === 'applied') btns.appendChild(h('span', { class: 'badge fixed' }, 'applied'));
    if (s.state === 'pending' && !chosen) {
      btns.appendChild(h('button', { disabled: !!s.error, title: s.error ? 'Invalid' : 'Make this the candidate for the Accept button', onclick: function () { post('suggestion', { id: m.id, sid: s.id, op: 'choose' }); } }, 'Choose'));
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
        if (ns && !ns.error) return post('suggestion', { id: m.id, sid: s.id, op: 'choose' });
        fillDetail(); toast('Saved, but the text is invalid: ' + (ns && ns.error));
      }).catch(fail);
    } }, 'Save'));
    btns.appendChild(h('button', { onclick: fillDetail }, 'Cancel'));
    ta.focus();
  }

  // ---- queue
  function suggest(ids, more, locale) {
    delete S.jobErrors[ids[0]];
    api('queue', { ids: ids, model: $('model').value, more: more, locale: locale || S.target }).then(function (j) { S.queue = j; pollQueue(); fillDetail(); }).catch(fail);
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
  $('btn-bulk-ok').addEventListener('click', markAllShownOk);
  $('btn-help').addEventListener('click', function () { $('help').showModal(); });
  $('help-close').addEventListener('click', function () { $('help').close(); });
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
        g.missing.slice(0, 8).forEach(function (id) { box.appendChild(h('div', { class: 'muted' }, h('button', { class: 'link', type: 'button', onclick: function () { jump(id); } }, id))); });
      });
      box.appendChild(h('h3', {}, 'English words with several target renderings'));
      var tb = h('table'); tb.appendChild(h('tr', {}, [h('th', {}, 'English word'), h('th', {}, 'Messages'), h('th', {}, 'Renderings (stem: messages)')]));
      t.inconsistent.forEach(function (r) {
        tb.appendChild(h('tr', {}, [h('td', {}, r.word), h('td', {}, String(r.messages)),
          h('td', {}, r.renderings.map(function (x) { return x.stem + '...: ' + x.count; }).join('   ') + '   ')]));
        var last = tb.lastChild.lastChild;
        r.renderings.forEach(function (x) { x.ids.slice(0, 2).forEach(function (id) { last.appendChild(h('button', { class: 'link', type: 'button', onclick: function () { jump(id); } }, id + ' ')); }); });
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
        d.appendChild(h('div', {}, c.locale + '/' + c.file + ':' + c.line + '  ' + c.key));
        (c.warnings || []).forEach(function (w) { d.appendChild(h('div', { class: 'ed-warn' }, 'Warning: ' + w)); });
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
      var a = document.createElement('a');
      a.href = URL.createObjectURL(b);   // a blob URL made here, never a string from the data
      a.download = 'review-' + S.target.replace(/[^A-Za-z0-9_-]/g, '') + '.md';
      document.body.appendChild(a); a.click(); a.remove();
    }).catch(fail);
  });

  // ---- wiring
  function setTab(t) {
    flushComment();
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
  $('target').addEventListener('change', function (e) { flushComment(); S.target = e.target.value; S.sel = null; S.edit = null; load().then(restoreDraft); });
  document.addEventListener('keydown', function (e) {
    var tag = (e.target.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea' || tag === 'select' || e.metaKey || e.ctrlKey || e.altKey || $('dlg').open || $('help').open) return;
    if (e.key === '?') { e.preventDefault(); $('help').showModal(); return; }
    if (S.tab !== 'list') return;
    var i = S.filtered.findIndex(function (m) { return m.id === S.sel; });
    if (e.key === 'j' || e.key === 'k') {
      var n = Math.min(S.filtered.length - 1, Math.max(0, i + (e.key === 'j' ? 1 : -1)));
      if (S.filtered[n]) { if (n >= S.shown) S.shown = n + 20; S.sel = null; select(S.filtered[n].id, true); }
    } else if (e.key === 'f') toggleFlag();
    else if (e.key === 'o') toggleOk();
    else if (e.key === 'u' || e.key === 'U') { var id = nextUnreviewed(e.key === 'u' ? 1 : -1, S.sel); if (id) goTo(id); else toast('No ' + (e.key === 'u' ? 'next' : 'previous') + ' unreviewed row in the shown list'); }
    else if (e.key === 'c') { var t = document.querySelector('textarea.cm'); if (t) { e.preventDefault(); t.focus(); } }
    else if (e.key === 'a') { var m = S.byId[S.sel]; if (m && m.tr && ($('no-ask').checked || confirm('Ask Claude for this message? This uses your Claude usage.'))) suggest([m.id], false); }
    else if (e.key === 'w') weird();
    else if (e.key === 'e') { if (S.sel) { e.preventDefault(); openEdit(S.sel, 'tr'); } }
    else if (e.key === 'E') { if (S.sel) { e.preventDefault(); openEdit(S.sel, 'en'); } }
    else return;
  });
  showSave();
  load().then(function () { pollQueue(); restoreDraft(); });
})();
