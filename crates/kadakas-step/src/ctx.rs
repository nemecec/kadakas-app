//! The state of one import: the file, which entities were used or skipped, the options, the
//! limits and the counters of the report.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use kadakas_meshio::MeshError;

use crate::part21::{Entity, StepFile, Value};
use crate::{StepLimits, StepOptions};
use kadakas_core::exact::{Curve, Surface};

/// Why a piece of the file is not read. `Skip` loses that piece only. `Abort` stops the import
/// (a limit that the file passes).
#[derive(Debug)]
pub enum Fail {
    Skip(String),
    Abort(MeshError),
}

impl From<MeshError> for Fail {
    fn from(e: MeshError) -> Fail {
        Fail::Abort(e)
    }
}

pub type R<T> = Result<T, Fail>;

pub fn skip<T>(why: impl Into<String>) -> R<T> {
    Err(Fail::Skip(why.into()))
}

/// The state of an entity in the accounting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unseen,
    Used,
    Skipped,
}

/// What was tessellated, by the kind of surface.
#[derive(Clone, Copy, Debug, Default)]
pub struct TessStat {
    pub faces: u64,
    pub triangles: u64,
}

pub struct Ctx<'a> {
    pub file: &'a StepFile,
    state: Vec<State>,
    pub skips: BTreeMap<(String, String), u64>,
    pub notes: Vec<String>,
    notes_seen: HashSet<String>,
    pub tessellated: BTreeMap<String, TessStat>,
    pub opts: &'a StepOptions,
    pub limits: &'a StepLimits,
    /// The chord tolerance in file units of the representation that is being read.
    pub tol: f64,
    /// The angle step in radians.
    pub angle: f64,
    /// Radians in one plane angle unit of the representation that is being read.
    pub angle_unit: f64,
    pub curves: HashMap<u64, Arc<Curve>>,
    /// Colors of styled items by the number of the item (a solid, a shell or a face).
    pub colors: HashMap<u64, [u8; 4]>,
    pub surfaces: HashMap<u64, Arc<Surface>>,
    pub depth: usize,
    /// Millimeters per file unit of the representations read, and whether the file named them.
    pub unit_scales: Vec<f64>,
    pub unit_from_file: bool,
    /// Triangles and vertices made so far, for the limits.
    pub triangles: u64,
    pub vertices: u64,
}

const MAX_NOTES: usize = 100;

impl<'a> Ctx<'a> {
    pub fn new(file: &'a StepFile, opts: &'a StepOptions, limits: &'a StepLimits) -> Ctx<'a> {
        Ctx {
            file,
            state: vec![State::Unseen; file.entities.len()],
            skips: BTreeMap::new(),
            notes: Vec::new(),
            notes_seen: HashSet::new(),
            tessellated: BTreeMap::new(),
            opts,
            limits,
            tol: 0.1,
            angle: 0.3,
            angle_unit: 1.0,
            curves: HashMap::new(),
            colors: HashMap::new(),
            surfaces: HashMap::new(),
            depth: 0,
            unit_scales: Vec::new(),
            unit_from_file: true,
            triangles: 0,
            vertices: 0,
        }
    }

    fn position(&self, id: u64) -> Option<usize> {
        self.file.entities.binary_search_by_key(&id, |e| e.id).ok()
    }

    /// The entity with this number, marked as used.
    pub fn ent(&mut self, id: u64) -> R<&'a Entity> {
        let file = self.file;
        match self.position(id) {
            Some(i) => {
                if self.state[i] == State::Unseen {
                    self.state[i] = State::Used;
                }
                Ok(&file.entities[i])
            }
            None => skip(format!("the reference #{id} points to no entity")),
        }
    }

    /// The entity with this number, without marking it.
    pub fn peek(&self, id: u64) -> Option<&'a Entity> {
        self.position(id).map(|i| &self.file.entities[i])
    }

    /// Marks an entity as used.
    pub fn use_id(&mut self, id: u64) {
        if let Some(i) = self.position(id)
            && self.state[i] == State::Unseen
        {
            self.state[i] = State::Used;
        }
    }

    /// Counts a piece of the file that is not read, with the reason. The entity is no longer
    /// counted as used.
    pub fn skip_entity(&mut self, e: &Entity, why: &str) {
        if let Some(i) = self.position(e.id) {
            if self.state[i] == State::Skipped {
                return;
            }
            self.state[i] = State::Skipped;
        }
        *self
            .skips
            .entry((type_name(e), why.to_string()))
            .or_default() += 1;
    }

    /// Same, by number.
    pub fn skip_id(&mut self, id: u64, why: &str) {
        if let Some(e) = self.peek(id) {
            self.skip_entity(e, why);
        }
    }

    pub fn note(&mut self, text: impl Into<String>) {
        let t = text.into();
        if self.notes.len() < MAX_NOTES && self.notes_seen.insert(t.clone()) {
            self.notes.push(t);
        }
    }

    pub fn tessellated_face(&mut self, kind: &str, triangles: u64) {
        self.tessellated_faces(kind, 1, triangles);
    }

    pub fn tessellated_faces(&mut self, kind: &str, faces: u64, triangles: u64) {
        let s = self.tessellated.entry(kind.to_string()).or_default();
        s.faces += faces;
        s.triangles += triangles;
    }

    /// The entities that are used, and those that nobody reached (by type).
    pub fn accounting(&self) -> (u64, BTreeMap<String, u64>, BTreeMap<String, u64>) {
        let mut used = BTreeMap::new();
        let mut unseen = BTreeMap::new();
        let mut used_n = 0;
        for (e, s) in self.file.entities.iter().zip(&self.state) {
            match s {
                State::Used => {
                    used_n += 1;
                    *used.entry(type_name(e)).or_default() += 1;
                }
                State::Unseen => *unseen.entry(type_name(e)).or_default() += 1,
                State::Skipped => {}
            }
        }
        (used_n, used, unseen)
    }

    /// Adds triangles to the count and checks the limits.
    pub fn count(&mut self, vertices: u64, triangles: u64) -> R<()> {
        self.vertices += vertices;
        self.triangles += triangles;
        let l = &self.opts.limits;
        if self.triangles > l.max_triangles {
            return Err(Fail::Abort(MeshError::TooManyTriangles {
                count: self.triangles,
                limit: l.max_triangles,
            }));
        }
        if self.vertices > l.max_vertices {
            return Err(Fail::Abort(MeshError::TooManyVertices {
                count: self.vertices,
                limit: l.max_vertices,
            }));
        }
        let need = kadakas_meshio::Limits::memory_of(self.vertices, self.triangles);
        if need > l.max_memory {
            return Err(Fail::Abort(MeshError::TooMuchMemory {
                bytes: need,
                limit: l.max_memory,
            }));
        }
        Ok(())
    }
}

/// The name of an entity for the report. A complex instance lists its parts.
pub fn type_name(e: &Entity) -> String {
    if e.name == crate::part21::COMPLEX {
        let parts: Vec<&str> = e
            .attrs
            .iter()
            .filter_map(|v| match v {
                Value::Typed(n, _) => Some(n.as_str()),
                _ => None,
            })
            .collect();
        format!("({})", parts.join("+"))
    } else {
        e.name.clone()
    }
}

// Reading values.

/// A real number (an integer also counts, a typed number like `LENGTH_MEASURE(2.)` too).
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Real(x) if x.is_finite() => Some(*x),
        Value::Int(i) => Some(*i as f64),
        Value::Typed(_, inner) => num(inner),
        _ => None,
    }
}

pub fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Typed(_, inner) => int(inner),
        _ => None,
    }
}

pub fn reference(v: &Value) -> Option<u64> {
    v.as_ref_id()
}

pub fn list(v: &Value) -> Option<&[Value]> {
    match v {
        Value::List(items) => Some(items),
        _ => None,
    }
}

/// `.T.` or `.F.`. Anything else (`.U.`, `$`) is `None`.
pub fn flag(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

pub fn text(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

pub fn enum_name(v: &Value) -> Option<&str> {
    match v {
        Value::Enum(s) => Some(s),
        _ => None,
    }
}

/// The attribute `i` of the entity.
pub fn attr(e: &Entity, i: usize) -> R<&Value> {
    e.attrs.get(i).map_or_else(
        || skip(format!("{} has no attribute {}", type_name(e), i + 1)),
        Ok,
    )
}

/// The references of a list attribute.
pub fn refs(v: &Value) -> Vec<u64> {
    list(v)
        .map(|l| l.iter().filter_map(reference).collect())
        .unwrap_or_default()
}

/// A list of numbers.
pub fn reals(v: &Value) -> Option<Vec<f64>> {
    list(v)?.iter().map(num).collect()
}
