// Pure helpers for the in-place editor: chip insertion and unsaved drafts. Node can test them.
(function (root) {
  'use strict';

  // Insert `chip` at the cursor (or over the selection). Returns the new text and cursor.
  function insertAtCursor(text, start, end, chip) {
    start = Math.max(0, Math.min(start, text.length));
    end = Math.max(start, Math.min(end, text.length));
    return { text: text.slice(0, start) + chip + text.slice(end), cursor: start + chip.length };
  }

  function isChanged(text, original) { return text.replace(/^\n+|\n+$/g, '') !== original; }

  var PREFIX = 'l10n-draft:';
  function draftKey(target, id) { return PREFIX + target + ':' + id; }

  function safe(fn, fallback) { try { return fn(); } catch (e) { return fallback; } }

  // A draft keeps the text, the original text it started from and a time.
  function saveDraft(storage, target, id, text, original) {
    return safe(function () {
      if (!isChanged(text, original)) { storage.removeItem(draftKey(target, id)); return false; }
      storage.setItem(draftKey(target, id), JSON.stringify({ text: text, original: original, time: Date.now() }));
      return true;
    }, false);
  }
  function loadDraft(storage, target, id, currentRaw) {
    return safe(function () {
      var raw = storage.getItem(draftKey(target, id));
      if (!raw) return null;
      var d = JSON.parse(raw);
      if (!d || typeof d.text !== 'string' || d.original !== currentRaw) return null;   // the file changed
      return d;
    }, null);
  }
  function clearDraft(storage, target, id) { safe(function () { storage.removeItem(draftKey(target, id)); }); }

  // The newest draft of a target whose original still equals the text in the file.
  // `current` maps message id to its current raw text.
  function newestDraft(storage, target, current) {
    return safe(function () {
      var best = null, prefix = PREFIX + target + ':';
      for (var i = 0; i < storage.length; i++) {
        var k = storage.key(i);
        if (!k || k.indexOf(prefix) !== 0) continue;
        var id = k.slice(prefix.length);
        if (!(id in current)) continue;
        var d = loadDraft(storage, target, id, current[id]);
        if (d && (!best || d.time > best.time)) best = { id: id, text: d.text, original: d.original, time: d.time };
      }
      return best;
    }, null);
  }

  var api = { insertAtCursor: insertAtCursor, isChanged: isChanged, draftKey: draftKey, saveDraft: saveDraft,
    loadDraft: loadDraft, clearDraft: clearDraft, newestDraft: newestDraft };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Editing = api;
})(typeof window !== 'undefined' ? window : this);
