//! The visual compare (REV-03) for agents and for the app: the `compare` option of `render.png`,
//! `render.svg` and `sheet.render`, and the overlay and the documents that the app draws.
//!
//! The overlay model, its style and the tints are in `kadakas_review::overlay`. This module joins
//! them with the session:
//!
//! - [`CompareSpec`]: the `compare` option, checked.
//! - [`CompareCache`]: the states, overlays and one built document, so that a slider that steps
//!   through the versions (or an agent that renders a series) does not replay the history for
//!   every step. The cache is per session and bounded in count and bytes, and `drop_caches` frees
//!   it.
//! - [`Session::compare_overlay`], [`Session::compare_doc`]: what the app draws.
//! - `render`: the pictures (SVG, PNG, sheets).
//!
//! Only objects are compared in the picture (the overlay is about what you see). Layers, classes
//! and resources are in `version.diff`.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use kadakas_core::history::HistoryData;
use kadakas_core::model::Document;
use kadakas_review::overlay::{CompareOverlay, CompareStyle, Palette, Status, UnchangedMode};
use kadakas_review::{History, State};

use crate::errors::bad;
use crate::review_api::{self, Side};
use crate::{RpcError, Session};

mod render;

pub(crate) use render::PngRequest;

/// The most states that the compare cache keeps.
pub const STATE_CACHE_ENTRIES: usize = 6;
/// The most bytes of states that the compare cache keeps (a state that is bigger on its own is not
/// kept).
pub const STATE_CACHE_BYTES: usize = 192 * 1024 * 1024;
/// The most overlays that the compare cache keeps.
pub const OVERLAY_CACHE_ENTRIES: usize = 8;

/// How the two sides are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareMode {
    /// One picture: the new side with the changes tinted, and the ghosts of removed objects.
    Overlay,
    /// Two pictures next to each other: the old side and the new side.
    SideBySide,
}

impl CompareMode {
    pub fn name(self) -> &'static str {
        match self {
            CompareMode::Overlay => "overlay",
            CompareMode::SideBySide => "side_by_side",
        }
    }
}

/// The `compare` option of a render, checked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompareSpec {
    pub from: Side,
    pub to: Side,
    pub mode: CompareMode,
    /// True if the option named a mode (a sheet compare is side by side without one).
    pub mode_given: bool,
    pub style: CompareStyle,
}

const KEYS: &[&str] = &[
    "from",
    "to",
    "mode",
    "hide_unchanged",
    "unchanged",
    "palette",
    "dark",
    "ghosts",
    "previous",
];

fn flag(m: &Map<String, Value>, key: &str) -> Result<Option<bool>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("compare.{key} must be true or false"))),
    }
}

impl CompareSpec {
    /// Reads `compare: {from, to, mode?, hide_unchanged?, unchanged?, palette?, dark?, ghosts?,
    /// previous?}`. `None` if the option is missing or null.
    pub fn parse(v: Option<&Value>) -> Result<Option<CompareSpec>, RpcError> {
        let Some(v) = v.filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let m = v
            .as_object()
            .ok_or_else(|| bad("compare must be an object: {from, to, mode?, hide_unchanged?}"))?;
        if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(bad(format!(
                "unknown compare option '{k}'. Use {}",
                KEYS.join(", ")
            )));
        }
        let side = |key: &str| {
            if !m.contains_key(key) {
                return Err(bad(format!(
                    "compare.{key} is missing: a version ID or \"current\""
                )));
            }
            review_api::side(m, key)
        };
        let (from, to) = (side("from")?, side("to")?);
        let mode = match m.get("mode") {
            None | Some(Value::Null) => CompareMode::Overlay,
            Some(Value::String(t)) if t == "overlay" => CompareMode::Overlay,
            Some(Value::String(t)) if t == "side_by_side" => CompareMode::SideBySide,
            Some(_) => return Err(bad("compare.mode is overlay or side_by_side")),
        };
        let mut style = CompareStyle::default();
        if let Some(name) = m.get("palette").filter(|v| !v.is_null()) {
            style.palette = name
                .as_str()
                .and_then(Palette::parse)
                .ok_or_else(|| bad("compare.palette is standard or color_blind"))?;
        }
        if let Some(name) = m.get("unchanged").filter(|v| !v.is_null()) {
            style.unchanged = name
                .as_str()
                .and_then(UnchangedMode::parse)
                .ok_or_else(|| bad("compare.unchanged is show, dim or hide"))?;
        }
        // `hide_unchanged` is the short form; `true` wins over `unchanged`.
        match flag(m, "hide_unchanged")? {
            Some(true) => style.unchanged = UnchangedMode::Hide,
            Some(false) if style.unchanged == UnchangedMode::Hide => {
                style.unchanged = UnchangedMode::Dim;
            }
            _ => {}
        }
        style.dark = flag(m, "dark")?.unwrap_or(false);
        style.show_ghosts = flag(m, "ghosts")?.unwrap_or(true);
        style.show_previous = flag(m, "previous")?.unwrap_or(true);
        Ok(Some(CompareSpec {
            from,
            to,
            mode,
            mode_given: m.get("mode").is_some_and(|v| !v.is_null()),
            style,
        }))
    }
}

/// The document of the new side of a comparison: the open drawing, or a version built from its
/// state.
pub enum ShownDoc {
    Live,
    Built(Arc<Document>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StateKey {
    Version { serial: u64, id: u64 },
    Current { serial: u64, revision: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OverlayKey {
    from: StateKey,
    to: StateKey,
}

/// The states, overlays and the built document of the compare, bounded. See the module text.
#[derive(Default)]
pub struct CompareCache {
    /// Least recently used first.
    states: Vec<(StateKey, Arc<State>, usize)>,
    overlays: Vec<(OverlayKey, Arc<CompareOverlay>)>,
    built: Option<(StateKey, Arc<Document>)>,
    /// How many states were made (replayed from the history), and how many came from the cache.
    /// Tests count them.
    pub(crate) state_builds: u32,
    pub(crate) state_hits: u32,
    pub(crate) overlay_builds: u32,
    pub(crate) overlay_hits: u32,
    pub(crate) doc_builds: u32,
}

impl CompareCache {
    /// The memory the cache holds, in bytes (about).
    pub fn approx_bytes(&self) -> u64 {
        let states: usize = self.states.iter().map(|(_, _, b)| *b).sum();
        let overlays: usize = self.overlays.iter().map(|(_, o)| o.bytes()).sum();
        let doc = self
            .built
            .as_ref()
            .map_or(0, |(_, d)| crate::limits::document_bytes(d) as usize);
        (states + overlays + doc) as u64
    }

    pub fn clear(&mut self) {
        self.states.clear();
        self.overlays.clear();
        self.built = None;
    }

    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    fn state(&mut self, key: StateKey) -> Option<Arc<State>> {
        let i = self.states.iter().position(|(k, _, _)| *k == key)?;
        let entry = self.states.remove(i);
        let state = entry.1.clone();
        self.states.push(entry);
        self.state_hits += 1;
        Some(state)
    }

    fn put_state(&mut self, key: StateKey, state: Arc<State>) {
        let bytes = state.bytes();
        if bytes > STATE_CACHE_BYTES {
            return;
        }
        self.states.retain(|(k, _, _)| *k != key);
        self.states.push((key, state, bytes));
        // Drop the least recently used until the count and the bytes fit.
        while self.states.len() > STATE_CACHE_ENTRIES
            || self.states.iter().map(|(_, _, b)| *b).sum::<usize>() > STATE_CACHE_BYTES
        {
            self.states.remove(0);
        }
    }

    fn overlay(&mut self, key: OverlayKey) -> Option<Arc<CompareOverlay>> {
        let i = self.overlays.iter().position(|(k, _)| *k == key)?;
        let entry = self.overlays.remove(i);
        let overlay = entry.1.clone();
        self.overlays.push(entry);
        self.overlay_hits += 1;
        Some(overlay)
    }

    fn put_overlay(&mut self, key: OverlayKey, overlay: Arc<CompareOverlay>) {
        self.overlays.retain(|(k, _)| *k != key);
        self.overlays.push((key, overlay));
        while self.overlays.len() > OVERLAY_CACHE_ENTRIES {
            self.overlays.remove(0);
        }
    }
}

impl Session {
    fn history_serial(&self) -> u64 {
        self.editor
            .doc()
            .history
            .as_ref()
            .map_or(0, HistoryData::serial)
    }

    fn state_key(&self, side: Side) -> StateKey {
        let serial = self.history_serial();
        match side {
            Side::Version(id) => StateKey::Version { serial, id },
            Side::Current => StateKey::Current {
                serial,
                revision: self.editor.revision(),
            },
        }
    }

    /// The state of one side, from the cache if it is there.
    pub fn compare_state(&mut self, side: Side) -> Result<Arc<State>, RpcError> {
        let key = self.state_key(side);
        if let Some(state) = self.compare.state(key) {
            return Ok(state);
        }
        let h: Arc<History> = review_api::read(self);
        let state = match side {
            Side::Version(id) => h.state_of(id).map_err(review_api::limit_error)?,
            Side::Current => {
                let tip = review_api::tip(self, &h)?;
                State::of(self.editor.doc(), tip.as_deref())
            }
        };
        self.compare.state_builds += 1;
        let state = Arc::new(state);
        self.compare.put_state(key, state.clone());
        Ok(state)
    }

    /// The overlay between two sides: the objects that `to` has and `from` has not are added,
    /// the ones `from` has and `to` has not are removed. Kept in a small cache.
    pub fn compare_overlay(
        &mut self,
        from: Side,
        to: Side,
    ) -> Result<Arc<CompareOverlay>, RpcError> {
        let key = OverlayKey {
            from: self.state_key(from),
            to: self.state_key(to),
        };
        if let Some(o) = self.compare.overlay(key) {
            return Ok(o);
        }
        let (a, b) = (self.compare_state(from)?, self.compare_state(to)?);
        let overlay = Arc::new(CompareOverlay::from_states(&a, &b));
        self.compare.overlay_builds += 1;
        self.compare.put_overlay(key, overlay.clone());
        Ok(overlay)
    }

    /// The document of a side: the open drawing, or the version built from its state (one such
    /// document is kept).
    pub fn compare_doc(&mut self, side: Side) -> Result<ShownDoc, RpcError> {
        if side == Side::Current {
            return Ok(ShownDoc::Live);
        }
        let key = self.state_key(side);
        if let Some((k, d)) = &self.compare.built
            && *k == key
        {
            return Ok(ShownDoc::Built(d.clone()));
        }
        let state = self.compare_state(side)?;
        let mut doc = state.build(self.editor.doc()).doc;
        doc.history = None;
        kadakas_review::restore::without_stacking_keys(&mut doc);
        let doc = Arc::new(doc);
        self.compare.doc_builds += 1;
        self.compare.built = Some((key, doc.clone()));
        Ok(ShownDoc::Built(doc))
    }

    /// The facts about a comparison for an answer: the sides, the counts, the legend and what the
    /// picture does not show completely.
    pub fn compare_info(&mut self, spec: &CompareSpec, overlay: Option<&CompareOverlay>) -> Value {
        let h = review_api::read(self);
        let mut out = json!({
            "from": review_api::side_ref(&h, &spec.from),
            "to": review_api::side_ref(&h, &spec.to),
            "mode": spec.mode.name(),
            "style": {
                "palette": spec.style.palette.name(),
                "unchanged": spec.style.unchanged.name(),
                "dark": spec.style.dark,
            },
        });
        if let Some(o) = overlay {
            out["counts"] = json!({
                "added": o.counts().added,
                "removed": o.counts().removed,
                "changed": o.counts().changed,
            });
            out["summary"] = serde_json::to_value(o.summary()).unwrap_or(Value::Null);
            out["notes"] = serde_json::to_value(o.notes()).unwrap_or(Value::Null);
            out["legend"] = legend(&spec.style);
        }
        out
    }
}

/// The legend of a style: for each status the color, the line pattern and the mark.
pub fn legend(style: &CompareStyle) -> Value {
    Value::Array(
        Status::ALL
            .iter()
            .map(|s| {
                let t = style.tint(*s);
                json!({
                    "status": s.name(),
                    "color": t.rgb,
                    "pattern": format!("{:?}", t.pattern).to_lowercase(),
                    "symbol": t.symbol.to_string(),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;
