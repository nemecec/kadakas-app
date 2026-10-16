//! The generator hook (PAT-09): a pattern of the kind `generator` names a script resource. The
//! layout engine asks for the unit shapes of a region, the script makes them, and the engine does
//! the rest (clipping, holes, whole and cut, counts, check, export).
//!
//! The script gets `args`:
//!
//! ```text
//! { params,                 // the parameters of the pattern (generator.params)
//!   origin: [x, y],         // where the pattern starts (mm)
//!   angle,                  // degrees, counterclockwise
//!   gap,                    // the width of a joint: the shapes are drawn at the centers of the joints
//!   max_units,              // the most shapes that it may return
//!   region: { box: [x0, y0, x1, y1], area,
//!             outer: Float64Array (stride 2), holes: [Float64Array, ...],      // the first shape
//!             shapes: [{ outer, holes }, ...] } }                              // all shapes
//! ```
//!
//! and returns the unit shapes, not clipped, as typed arrays:
//!
//! ```text
//! { types: ["300x200", ...],                                  // names of the unit types
//!   rects: { kind: Uint32Array, data: Float64Array },         // stride 5: cx, cy, length, width, angle (degrees)
//!   polygons: { kind: Uint32Array, start: Uint32Array,        // start has n + 1 entries
//!               points: Float64Array } }                      // x, y pairs of all shapes
//! ```
//!
//! A generator is a pure function of its input: no calls to the API (`kadakas.call` fails), no
//! clock, a fixed `Math.random`. The host keeps the answer for the same input, so a pattern is made
//! again only when the region, the parameters, the origin, the angle, the gap or the text of the
//! script change.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use glam::DVec2;
use kadakas_core::layout::generator::{
    Cell, Generated, Request, UnitGenerator, install, installed,
};
use kadakas_script_js::{CallFailure, HostError, Output, ScriptHost, ScriptLimits};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::EngineSlot;
use crate::Session;

/// Most answers the cache keeps, and most cells in all of them.
const CACHE_ENTRIES: usize = 16;
const CACHE_CELLS: usize = 400_000;

/// A script of the document, as the generator sees it.
struct Entry {
    id: String,
    name: String,
    code: Arc<str>,
    sha: String,
    runnable: bool,
}

#[derive(Default)]
struct Cache {
    entries: VecDeque<(String, Arc<Generated>)>,
    cells: usize,
}

impl Cache {
    fn get(&mut self, key: &str) -> Option<Arc<Generated>> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        let e = self.entries.remove(at)?;
        let v = e.1.clone();
        self.entries.push_back(e);
        Some(v)
    }

    fn put(&mut self, key: String, g: Arc<Generated>) {
        self.cells += g.cells.len();
        self.entries.push_back((key, g));
        while self.entries.len() > CACHE_ENTRIES
            || (self.cells > CACHE_CELLS && self.entries.len() > 1)
        {
            if let Some((_, old)) = self.entries.pop_front() {
                self.cells -= old.cells.len();
            }
        }
    }
}

/// The generator of one state of the document.
struct JsGenerator {
    scripts: Vec<Entry>,
    engine: Arc<EngineSlot>,
    limits: ScriptLimits,
    cache: Arc<Mutex<Cache>>,
}

/// The generator for one state of the document: the revision it was made for, the limits it has,
/// and the generator itself.
type Current = (u64, ScriptLimits, Arc<dyn UnitGenerator>);

/// What the session keeps: the cache of answers and the generator that is installed.
#[derive(Default)]
pub(crate) struct GeneratorState {
    cache: Arc<Mutex<Cache>>,
    current: Mutex<Option<Current>>,
}

impl GeneratorState {
    /// The scripts or their trust changed: build the generator again at the next sync.
    pub(crate) fn invalidate(&self) {
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl Session {
    /// Installs the generator of this document for the current thread, so that the layout of a
    /// generator pattern can run its script. `handle` does this for every call; a host that
    /// draws patterns itself (the app) calls it before it draws. It is cheap when nothing changed.
    pub fn sync_generators(&mut self) {
        let revision = self.editor.revision();
        let limits = super::limits_of(self);
        let state = &self.script_js.generator;
        let mut cur = state.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((rev, l, g)) = cur.as_ref()
            && *rev == revision
            && *l == limits
        {
            if !installed().is_some_and(|i| Arc::ptr_eq(&i, g)) {
                install(Some(g.clone()));
            }
            return;
        }
        let scripts: Vec<Entry> = self
            .editor
            .doc()
            .resources
            .iter()
            .filter_map(|r| match &r.body {
                kadakas_core::resource::ResourceBody::Script(c) => {
                    let sha = c.sha256();
                    Some(Entry {
                        id: r.id.to_string(),
                        name: r.name.clone(),
                        code: Arc::from(c.code.as_str()),
                        runnable: super::trusted(self, &sha) || super::agents_allowed(self),
                        sha,
                    })
                }
                _ => None,
            })
            .collect();
        let g: Arc<dyn UnitGenerator> = Arc::new(JsGenerator {
            scripts,
            engine: self.script_js.engine.clone(),
            limits,
            cache: state.cache.clone(),
        });
        install(Some(g.clone()));
        *cur = Some((revision, limits, g));
    }
}

/// A script cannot call the API from a generator.
struct NoCalls;

impl ScriptHost for NoCalls {
    fn call(&mut self, method: &str, _p: Value, _t: bool) -> Result<Value, HostError> {
        Err(HostError::Failure(CallFailure {
            code: crate::errors::CODE_POLICY,
            message: format!(
                "a generator cannot call {method}: it makes units from its input only"
            ),
            data: None,
        }))
    }
}

fn f64_bytes(points: &[DVec2]) -> Vec<u8> {
    points
        .iter()
        .flat_map(|p| [p.x, p.y])
        .flat_map(f64::to_le_bytes)
        .collect()
}

impl UnitGenerator for JsGenerator {
    fn generate(&self, req: &Request<'_>) -> Result<Arc<Generated>, String> {
        let wanted = req.spec.script.trim();
        let entry = self
            .scripts
            .iter()
            .find(|e| e.id == wanted)
            .or_else(|| self.scripts.iter().find(|e| e.name.eq_ignore_ascii_case(wanted)))
            .ok_or_else(|| {
                format!(
                    "the pattern needs the script '{wanted}', and this document has none with that name. Create it with script_resource.create"
                )
            })?;
        if !entry.runnable {
            return Err(format!(
                "the script '{}' is not trusted. A person must trust its exact text (SHA-256 {}) in the app, in the Scripts window",
                entry.name, entry.sha
            ));
        }
        // The input: the region as typed arrays.
        let mut buffers: Vec<Vec<u8>> = Vec::new();
        let mut marker = |points: &[DVec2]| {
            buffers.push(f64_bytes(points));
            json!({"$kt": "f64", "$ki": buffers.len() - 1, "$kn": 2})
        };
        let shapes: Vec<Value> = req
            .region
            .shapes
            .iter()
            .map(|sh| {
                let outer = marker(&sh[0]);
                let holes: Vec<Value> = sh[1..].iter().map(|h| marker(h)).collect();
                json!({"outer": outer, "holes": holes})
            })
            .collect();
        let first = shapes.first().cloned().unwrap_or(Value::Null);
        let args = json!({
            "params": req.spec.params,
            "origin": [req.origin.x, req.origin.y],
            "angle": req.angle,
            "gap": req.gap,
            "max_units": req.max_units,
            "region": {
                "box": [req.region.lo.x, req.region.lo.y, req.region.hi.x, req.region.hi.y],
                "area": req.region.area,
                "outer": first["outer"],
                "holes": first["holes"],
                "shapes": shapes,
            },
        });
        let mut h = Sha256::new();
        h.update(entry.sha.as_bytes());
        h.update(args.to_string().as_bytes());
        for b in &buffers {
            h.update(b);
        }
        let key = format!("{:x}", h.finalize());
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return Ok(hit);
        }
        let engine = self.engine.get(self.limits)?;
        let out = engine
            .run(
                &kadakas_script_js::Request {
                    code: &entry.code,
                    args: &args,
                    buffers: &buffers,
                    seed: 0,
                },
                &mut NoCalls,
            )
            .map_err(|f| format!("the generator script '{}' failed: {}", entry.name, f.error))?;
        let made = Arc::new(parse(&out, req.max_units).map_err(|e| {
            format!(
                "the generator script '{}' gave a bad answer: {e}",
                entry.name
            )
        })?);
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(key, made.clone());
        Ok(made)
    }
}

/// A list of numbers from the answer: a typed array (a marker and its bytes) or a plain list.
fn numbers(v: &Value, out: &Output, what: &str) -> Result<Vec<f64>, String> {
    let mut v = v.clone();
    kadakas_script_js::typed::resolve(&mut v, &out.buffers)?;
    // A marker with a stride is a list of tuples: flatten it.
    fn flat(v: &Value, acc: &mut Vec<f64>) -> bool {
        match v {
            Value::Number(n) => {
                acc.push(n.as_f64().unwrap_or(f64::NAN));
                true
            }
            Value::Array(a) => a.iter().all(|x| flat(x, acc)),
            _ => false,
        }
    }
    let mut acc = Vec::new();
    if !flat(&v, &mut acc) {
        return Err(format!(
            "'{what}' must be a typed array or a list of numbers"
        ));
    }
    Ok(acc)
}

fn whole_numbers(v: &Value, out: &Output, what: &str) -> Result<Vec<usize>, String> {
    numbers(v, out, what)?
        .into_iter()
        .map(|n| {
            if n >= 0.0 && n.fract() == 0.0 && n < 4e9 {
                Ok(n as usize)
            } else {
                Err(format!(
                    "'{what}' holds a number that is not a whole number from 0"
                ))
            }
        })
        .collect()
}

/// The unit shapes from the answer of a generator.
fn parse(out: &Output, max_units: usize) -> Result<Generated, String> {
    let obj = out
        .value
        .as_object()
        .ok_or("the script must return an object {types, rects?, polygons?}")?;
    let kinds: Vec<String> = match obj.get("types") {
        None | Some(Value::Null) => vec!["unit".to_string()],
        Some(Value::Array(a)) => a
            .iter()
            .map(|n| {
                n.as_str()
                    .map(|t| {
                        t.chars()
                            .filter(|c| !c.is_control())
                            .take(kadakas_core::layout::generator::MAX_KIND_NAME)
                            .collect::<String>()
                            .trim()
                            .to_string()
                    })
                    .filter(|t| !t.is_empty())
                    .ok_or("every name in 'types' must be non-empty text")
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("'types' must be a list of names".into()),
    };
    let mut cells: Vec<Cell> = Vec::new();
    if let Some(r) = obj.get("rects").filter(|v| !v.is_null()) {
        let data = numbers(
            r.get("data").ok_or("'rects' needs 'data'")?,
            out,
            "rects.data",
        )?;
        if data.len() % 5 != 0 {
            return Err(format!(
                "rects.data has {} numbers, but a rectangle takes 5 (cx, cy, length, width, angle in degrees)",
                data.len()
            ));
        }
        let n = data.len() / 5;
        if n > max_units {
            return Err(format!(
                "the generator made {n} rectangles, more than the limit of {max_units}"
            ));
        }
        let kind = match r.get("kind").filter(|v| !v.is_null()) {
            Some(k) => whole_numbers(k, out, "rects.kind")?,
            None => vec![0; n],
        };
        if kind.len() != n {
            return Err(format!(
                "rects.kind has {} entries for {n} rectangles",
                kind.len()
            ));
        }
        for (i, d) in data.as_chunks::<5>().0.iter().enumerate() {
            let (c, l, w) = (DVec2::new(d[0], d[1]), d[2], d[3]);
            if !(l > 0.0 && w > 0.0) {
                return Err(format!("rectangle {i} needs a positive length and width"));
            }
            let u = DVec2::from_angle(d[4].to_radians()) * (l / 2.0);
            let v = DVec2::from_angle(d[4].to_radians()).perp() * (w / 2.0);
            cells.push(Cell {
                kind: kind[i],
                ring: vec![c - u - v, c + u - v, c + u + v, c - u + v],
            });
        }
    }
    if let Some(p) = obj.get("polygons").filter(|v| !v.is_null()) {
        let pts = numbers(
            p.get("points").ok_or("'polygons' needs 'points'")?,
            out,
            "polygons.points",
        )?;
        let start = whole_numbers(
            p.get("start").ok_or("'polygons' needs 'start'")?,
            out,
            "polygons.start",
        )?;
        if pts.len() % 2 != 0 {
            return Err("polygons.points holds x and y pairs".into());
        }
        let npts = pts.len() / 2;
        if start.len() < 2
            || start.windows(2).any(|w| w[0] > w[1])
            || *start.last().unwrap_or(&0) != npts
            || start[0] != 0
        {
            return Err(format!(
                "polygons.start must have n + 1 entries that rise from 0 to the number of points ({npts})"
            ));
        }
        let n = start.len() - 1;
        if cells.len() + n > max_units {
            return Err(format!(
                "the generator made {} units, more than the limit of {max_units}",
                cells.len() + n
            ));
        }
        let kind = match p.get("kind").filter(|v| !v.is_null()) {
            Some(k) => whole_numbers(k, out, "polygons.kind")?,
            None => vec![0; n],
        };
        if kind.len() != n {
            return Err(format!(
                "polygons.kind has {} entries for {n} polygons",
                kind.len()
            ));
        }
        for i in 0..n {
            cells.push(Cell {
                kind: kind[i],
                ring: (start[i]..start[i + 1])
                    .map(|k| DVec2::new(pts[2 * k], pts[2 * k + 1]))
                    .collect(),
            });
        }
    }
    Ok(Generated { kinds, cells })
}
