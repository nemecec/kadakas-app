//! A reader for large STEP physical files (ISO 10303-21): one pass to build an index, then lazy
//! parsing of single entities.
//!
//! [`step::parse`](crate::step::parse) reads a whole file into memory as parsed values. This reader
//! does not. [`StepIndex::from_reader`] scans the bytes once and records where each entity starts
//! and ends. [`StepIndex::entity`] parses one entity when a caller asks for it, and keeps the
//! result in a cache. A file of 1 GB with 10 million entities costs the file bytes plus about 48
//! bytes per entity, and only the entities that the importer visits are parsed.
//!
//! Safety rules, so that hostile or broken files never crash the reader:
//!
//! - The size of the file, the number of entities, the nesting depth and the length of a list have
//!   caps ([`Limits`]). A file over the size cap is refused. Over the entity cap, the scan stops
//!   and says so in [`StepIndex::notes`].
//! - The scanner has no recursion. The value parser recurses only up to the depth cap.
//! - A broken statement is skipped. The scan continues at the next `;`. A cut-off file keeps all
//!   the entities that are complete.
//! - Comments (`/* */`), strings with `''`, and complex entities (`#1=(A()B());`) are understood.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;

use crate::step::{Entity, Value, decode_string};

/// Caps for one file.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Largest file in bytes.
    pub max_bytes: usize,
    /// Most entities in the index.
    pub max_entities: usize,
    /// Deepest nesting of lists and typed values.
    pub max_depth: usize,
    /// Most items in one list.
    pub max_list: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 2 << 30,
            max_entities: 40_000_000,
            max_depth: 48,
            max_list: 20_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Rec {
    id: u64,
    name_start: usize,
    name_len: u16,
    /// Start of the `(` of the attributes.
    body_start: usize,
    /// Index of the `;`.
    end: usize,
}

/// An index over the entities of a STEP file.
pub struct StepIndex {
    data: Vec<u8>,
    recs: Vec<Rec>,
    by_id: HashMap<u64, u32>,
    header: String,
    /// Remarks about the file: cut-off data, skipped statements, reached caps.
    pub notes: Vec<String>,
    limits: Limits,
    cache: RefCell<HashMap<u64, Option<Rc<Entity>>>>,
}

/// Why a file cannot be read at all.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StepError {
    #[error("{0}")]
    Io(String),
    #[error("the file is larger than the limit of {0} bytes")]
    TooLarge(usize),
    #[error("not a STEP file: {0}")]
    Format(String),
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Skips spaces and `/* */` comments. An unclosed comment runs to the end.
fn skip_ws(d: &[u8], mut i: usize) -> usize {
    loop {
        while i < d.len() && is_space(d[i]) {
            i += 1;
        }
        if d[i.min(d.len())..].starts_with(b"/*") {
            match find(d, i + 2, b"*/") {
                Some(e) => i = e + 2,
                None => return d.len(),
            }
        } else {
            return i;
        }
    }
}

fn find(d: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if from >= d.len() {
        return None;
    }
    d[from..]
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

/// The index of the `;` that ends the statement that starts at `i`, skipping strings and comments.
fn statement_end(d: &[u8], mut i: usize) -> Option<usize> {
    while i < d.len() {
        match d[i] {
            b';' => return Some(i),
            b'\'' => {
                // A doubled quote toggles twice, so a plain toggle is right.
                i += 1;
                while i < d.len() && d[i] != b'\'' {
                    i += 1;
                }
            }
            b'/' if d.get(i + 1) == Some(&b'*') => {
                i = find(d, i + 2, b"*/")? + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

impl StepIndex {
    /// Reads the whole stream (up to the size cap) and indexes it.
    pub fn from_reader<R: Read>(r: R, limits: Limits) -> Result<StepIndex, StepError> {
        let mut data = Vec::new();
        let cap = limits.max_bytes as u64;
        r.take(cap + 1)
            .read_to_end(&mut data)
            .map_err(|e| StepError::Io(e.to_string()))?;
        if data.len() > limits.max_bytes {
            return Err(StepError::TooLarge(limits.max_bytes));
        }
        StepIndex::from_bytes(data, limits)
    }

    /// Indexes bytes that are in memory already.
    pub fn from_bytes(data: Vec<u8>, limits: Limits) -> Result<StepIndex, StepError> {
        if data.len() > limits.max_bytes {
            return Err(StepError::TooLarge(limits.max_bytes));
        }
        let mut idx = StepIndex {
            data,
            recs: Vec::new(),
            by_id: HashMap::new(),
            header: String::new(),
            notes: Vec::new(),
            limits,
            cache: RefCell::new(HashMap::new()),
        };
        idx.scan()?;
        Ok(idx)
    }

    fn scan(&mut self) -> Result<(), StepError> {
        let d = &self.data;
        let mut i = skip_ws(
            d,
            if d.starts_with(&[0xEF, 0xBB, 0xBF]) {
                3
            } else {
                0
            },
        );
        if !d[i..].starts_with(b"ISO-10303-21") {
            return Err(StepError::Format(
                "the file does not start with ISO-10303-21".into(),
            ));
        }
        i = match statement_end(d, i) {
            Some(e) => e + 1,
            None => return Err(StepError::Format("the file ends in the first line".into())),
        };
        // Header: statements up to `DATA;`.
        let header_start = i;
        let mut data_start = None;
        while i < d.len() {
            i = skip_ws(d, i);
            let Some(e) = statement_end(d, i) else { break };
            let head = &d[i..e];
            if head.starts_with(b"DATA") && head[4..].iter().all(|c| is_space(*c)) {
                data_start = Some(e + 1);
                break;
            }
            i = e + 1;
        }
        let Some(mut i) = data_start else {
            return Err(StepError::Format("no DATA section".into()));
        };
        self.header = String::from_utf8_lossy(&d[header_start..i.saturating_sub(5)]).into_owned();

        let mut bad = 0usize;
        let mut cut = false;
        let mut duplicate = 0usize;
        let mut recs: Vec<Rec> = Vec::new();
        let mut by_id: HashMap<u64, u32> = HashMap::new();
        loop {
            i = skip_ws(d, i);
            if i >= d.len() {
                cut = true;
                break;
            }
            if d[i..].starts_with(b"ENDSEC") {
                break;
            }
            if recs.len() >= self.limits.max_entities {
                self.notes.push(format!(
                    "the file has more than {} entities: the rest is ignored",
                    self.limits.max_entities
                ));
                break;
            }
            let Some(end) = statement_end(d, i) else {
                cut = true;
                break;
            };
            match Self::head(d, i, end) {
                Some(rec) => match by_id.entry(rec.id) {
                    std::collections::hash_map::Entry::Occupied(_) => duplicate += 1,
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(recs.len() as u32);
                        recs.push(rec);
                    }
                },
                None => bad += 1,
            }
            i = end + 1;
        }
        if cut {
            self.notes
                .push("the file is cut off: the last entity is not complete".into());
        }
        if bad > 0 {
            self.notes.push(format!(
                "{bad} statements were not entities and were skipped"
            ));
        }
        if duplicate > 0 {
            self.notes.push(format!(
                "{duplicate} entities repeat an ID: the first one is used"
            ));
        }
        self.recs = recs;
        self.by_id = by_id;
        Ok(())
    }

    /// Reads `#id = NAME (` of the statement `d[i..end]`.
    fn head(d: &[u8], mut i: usize, end: usize) -> Option<Rec> {
        if d.get(i) != Some(&b'#') {
            return None;
        }
        i += 1;
        let s = i;
        while i < end && d[i].is_ascii_digit() {
            i += 1;
        }
        let id: u64 = std::str::from_utf8(&d[s..i]).ok()?.parse().ok()?;
        i = skip_ws(d, i);
        if d.get(i) != Some(&b'=') || i >= end {
            return None;
        }
        i = skip_ws(d, i + 1);
        if i >= end {
            return None;
        }
        if d[i] == b'(' {
            return Some(Rec {
                id,
                name_start: i,
                name_len: 0,
                body_start: i,
                end,
            });
        }
        let ns = i;
        while i < end && is_word(d[i]) {
            i += 1;
        }
        if i == ns || i - ns > 200 {
            return None;
        }
        let name_len = (i - ns) as u16;
        i = skip_ws(d, i);
        if i >= end || d[i] != b'(' {
            return None;
        }
        Some(Rec {
            id,
            name_start: ns,
            name_len,
            body_start: i,
            end,
        })
    }

    /// The text of the header section.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// The schema names of `FILE_SCHEMA`, for example `IFC4`.
    pub fn schemas(&self) -> Vec<String> {
        let Some(at) = self.header.find("FILE_SCHEMA") else {
            return Vec::new();
        };
        let rest = &self.header[at..];
        let end = rest.find(';').unwrap_or(rest.len());
        let mut out = Vec::new();
        let mut parts = rest[..end].split('\'');
        parts.next();
        while let Some(name) = parts.next() {
            out.push(name.to_string());
            parts.next();
        }
        out
    }

    /// Number of entities in the index.
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// The upper-case name of an entity, without parsing its attributes. Complex entities give `""`.
    pub fn name_of(&self, id: u64) -> Option<&str> {
        let r = &self.recs[*self.by_id.get(&id)? as usize];
        Some(self.rec_name(r))
    }

    fn rec_name(&self, r: &Rec) -> &str {
        std::str::from_utf8(&self.data[r.name_start..r.name_start + r.name_len as usize])
            .unwrap_or("")
    }

    /// Every `(id, name)` in file order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &str)> {
        self.recs.iter().map(|r| (r.id, self.rec_name(r)))
    }

    /// The IDs of all entities with this upper-case name.
    pub fn ids_of(&self, name: &str) -> Vec<u64> {
        self.iter()
            .filter(|(_, n)| n.eq_ignore_ascii_case(name))
            .map(|(i, _)| i)
            .collect()
    }

    /// Parses an entity. `None` if there is none with this ID, or if its attributes are broken.
    pub fn entity(&self, id: u64) -> Option<Rc<Entity>> {
        if let Some(hit) = self.cache.borrow().get(&id) {
            return hit.clone();
        }
        let e = self.parse_entity(id).map(Rc::new);
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 3_000_000 {
            cache.clear();
        }
        cache.insert(id, e.clone());
        e
    }

    /// Drops the parsed entities, to free memory.
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }

    fn parse_entity(&self, id: u64) -> Option<Entity> {
        let r = self.recs[*self.by_id.get(&id)? as usize];
        let mut p = ValueParser {
            d: &self.data[..r.end],
            i: r.body_start,
            max_depth: self.limits.max_depth,
            max_list: self.limits.max_list,
        };
        if r.name_len == 0 {
            // Complex entity: `(NAME(args) NAME(args))`.
            p.i += 1;
            let mut parts = Vec::new();
            loop {
                p.ws();
                if p.peek()? == b')' {
                    break;
                }
                let name = p.word()?;
                p.ws();
                let args = p.value(1).ok()?;
                parts.push(Value::Typed(name, Box::new(args)));
            }
            return Some(Entity {
                id,
                name: "COMPLEX".into(),
                attrs: parts,
            });
        }
        let name = self.rec_name(&r).to_ascii_uppercase();
        let attrs = match p.value(0).ok()? {
            Value::List(items) => items,
            _ => return None,
        };
        Some(Entity { id, name, attrs })
    }
}

struct ValueParser<'a> {
    d: &'a [u8],
    i: usize,
    max_depth: usize,
    max_list: usize,
}

impl ValueParser<'_> {
    fn peek(&self) -> Option<u8> {
        self.d.get(self.i).copied()
    }

    fn ws(&mut self) {
        self.i = skip_ws(self.d, self.i);
    }

    fn word(&mut self) -> Option<String> {
        let s = self.i;
        while self.i < self.d.len() && is_word(self.d[self.i]) {
            self.i += 1;
        }
        (self.i > s).then(|| String::from_utf8_lossy(&self.d[s..self.i]).to_ascii_uppercase())
    }

    fn value(&mut self, depth: usize) -> Result<Value, ()> {
        if depth > self.max_depth {
            return Err(());
        }
        self.ws();
        let c = self.peek().ok_or(())?;
        match c {
            b'$' => {
                self.i += 1;
                Ok(Value::Null)
            }
            b'*' => {
                self.i += 1;
                Ok(Value::Derived)
            }
            b'#' => {
                self.i += 1;
                let s = self.i;
                while self.i < self.d.len() && self.d[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                std::str::from_utf8(&self.d[s..self.i])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .map(Value::Ref)
                    .ok_or(())
            }
            b'\'' => {
                self.i += 1;
                let s = self.i;
                loop {
                    match self.peek() {
                        None => return Err(()),
                        Some(b'\'') if self.d.get(self.i + 1) == Some(&b'\'') => self.i += 2,
                        Some(b'\'') => break,
                        Some(_) => self.i += 1,
                    }
                }
                let text = String::from_utf8_lossy(&self.d[s..self.i]).into_owned();
                self.i += 1;
                Ok(Value::Str(decode_string(&text)))
            }
            b'.' => {
                self.i += 1;
                let w = self.word().ok_or(())?;
                if self.peek() != Some(b'.') {
                    return Err(());
                }
                self.i += 1;
                Ok(match w.as_str() {
                    "T" => Value::Bool(true),
                    "F" => Value::Bool(false),
                    _ => Value::Enum(w),
                })
            }
            b'(' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek() == Some(b')') {
                    self.i += 1;
                    return Ok(Value::List(items));
                }
                loop {
                    if items.len() >= self.max_list {
                        return Err(());
                    }
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b')') => {
                            self.i += 1;
                            return Ok(Value::List(items));
                        }
                        _ => return Err(()),
                    }
                }
            }
            c if c.is_ascii_digit() || matches!(c, b'-' | b'+') => {
                let s = self.i;
                self.i += 1;
                while self.peek().is_some_and(|c| {
                    c.is_ascii_digit() || matches!(c, b'.' | b'E' | b'e' | b'-' | b'+')
                }) {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.d[s..self.i]).map_err(|_| ())?;
                if t.contains(['.', 'E', 'e']) {
                    t.parse::<f64>().map(Value::Real).map_err(|_| ())
                } else {
                    t.parse::<i64>().map(Value::Int).map_err(|_| ())
                }
            }
            c if c.is_ascii_alphabetic() => {
                let name = self.word().ok_or(())?;
                self.ws();
                if self.peek() != Some(b'(') {
                    return Err(());
                }
                // A defined type has one argument. More arguments stay as a list.
                match self.value(depth + 1)? {
                    Value::List(mut items) if items.len() == 1 => {
                        Ok(Value::Typed(name, Box::new(items.remove(0))))
                    }
                    other => Ok(Value::Typed(name, Box::new(other))),
                }
            }
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('x'),'2;1');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n/* c */ #1=IFCCARTESIANPOINT((0.,1.5E2,-3.));\n#2=IFCLABEL('it''s; ok');\n#3 = IFCPROJECT ( $ , *, .T., (#1,#2), IFCLABEL('a'), 7 ) ;\n#4=(A()B(1));\nENDSEC;\nEND-ISO-10303-21;\n";

    fn idx(s: &str) -> StepIndex {
        StepIndex::from_bytes(s.as_bytes().to_vec(), Limits::default()).unwrap()
    }

    #[test]
    fn indexes_and_parses_lazily() {
        let x = idx(FILE);
        assert_eq!(x.len(), 4);
        assert_eq!(x.schemas(), vec!["IFC4"]);
        assert_eq!(x.name_of(3), Some("IFCPROJECT"));
        assert_eq!(x.ids_of("ifcproject"), vec![3]);
        let p = x.entity(1).unwrap();
        assert_eq!(
            p.attrs[0],
            Value::List(vec![
                Value::Real(0.0),
                Value::Real(150.0),
                Value::Real(-3.0)
            ])
        );
        assert_eq!(x.entity(2).unwrap().attrs[0], Value::Str("it's; ok".into()));
        let e = x.entity(3).unwrap();
        assert_eq!(e.attrs[1], Value::Derived);
        assert_eq!(e.attrs[2], Value::Bool(true));
        assert_eq!(
            e.attrs[4],
            Value::Typed("IFCLABEL".into(), Box::new(Value::Str("a".into())))
        );
        assert_eq!(x.entity(4).unwrap().name, "COMPLEX");
        assert!(x.entity(99).is_none());
        assert_eq!(x.entity(1), x.entity(1));
        x.clear_cache();
        assert_eq!(x.iter().count(), 4);
    }

    #[test]
    fn matches_the_eager_parser() {
        let eager = crate::step::parse(
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCCARTESIANPOINT((0.,1.5E2));\n#2=IFCLABEL('a');\nENDSEC;\nEND-ISO-10303-21;\n",
        )
        .unwrap();
        let x = idx(
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCCARTESIANPOINT((0.,1.5E2));\n#2=IFCLABEL('a');\nENDSEC;\nEND-ISO-10303-21;\n",
        );
        for e in &eager.entities {
            assert_eq!(&*x.entity(e.id).unwrap(), e);
        }
    }

    #[test]
    fn truncated_and_broken_files() {
        let cut = &FILE[..FILE.find("#3").unwrap() + 10];
        let x = idx(cut);
        assert_eq!(x.len(), 2);
        assert!(x.notes.iter().any(|n| n.contains("cut off")));
        let x = idx(&FILE.replace("#2=IFCLABEL", "garbage;\n#2=IFCLABEL"));
        assert_eq!(x.len(), 4);
        assert!(x.notes.iter().any(|n| n.contains("skipped")));
        let x = idx(&FILE.replace("#2=", "#1="));
        assert!(x.notes.iter().any(|n| n.contains("repeat")));
        // Broken attributes: the entity is there, but does not parse.
        let x = idx(&FILE.replace("(0.,1.5E2,-3.)", "(0.,,"));
        assert!(x.entity(1).is_none());
    }

    #[test]
    fn refuses_other_files_and_big_files() {
        assert!(matches!(
            StepIndex::from_bytes(b"hello".to_vec(), Limits::default()),
            Err(StepError::Format(_))
        ));
        assert!(matches!(
            StepIndex::from_bytes(b"ISO-10303-21;\nHEADER;\n".to_vec(), Limits::default()),
            Err(StepError::Format(_))
        ));
        let small = Limits {
            max_bytes: 10,
            ..Limits::default()
        };
        assert_eq!(
            StepIndex::from_bytes(FILE.as_bytes().to_vec(), small.clone()).err(),
            Some(StepError::TooLarge(10))
        );
        assert!(matches!(
            StepIndex::from_reader(FILE.as_bytes(), small),
            Err(StepError::TooLarge(10))
        ));
        assert!(StepIndex::from_reader(FILE.as_bytes(), Limits::default()).is_ok());
        let few = Limits {
            max_entities: 2,
            ..Limits::default()
        };
        let x = StepIndex::from_bytes(FILE.as_bytes().to_vec(), few).unwrap();
        assert_eq!(x.len(), 2);
        assert!(x.notes.iter().any(|n| n.contains("more than 2")));
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let deep = format!("#1=IFCX({}{});", "(".repeat(200_000), ")".repeat(200_000));
        let file = format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{deep}\nENDSEC;\n");
        let x = idx(&file);
        assert_eq!(x.len(), 1);
        assert!(x.entity(1).is_none());
        let typed = format!("#1=IFCX({}1{});", "A(".repeat(100_000), ")".repeat(100_000));
        let x = idx(&format!(
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{typed}\nENDSEC;\n"
        ));
        assert!(x.entity(1).is_none());
        let long = StepIndex::from_bytes(
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=IFCX((1,2,3));\nENDSEC;\n"
                .as_bytes()
                .to_vec(),
            Limits {
                max_list: 2,
                ..Limits::default()
            },
        )
        .unwrap();
        assert!(long.entity(1).is_none());
    }

    #[test]
    fn odd_syntax() {
        let x = idx(
            "\u{feff}ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=IFCX(.UNSET.,'\\X2\\00E4\\X0\\',1.,-2,+3,#7,IFCA(1,2));\n#2=NOPE;\n#3=IFCY('open\n",
        );
        assert_eq!(x.len(), 1);
        let e = x.entity(1).unwrap();
        assert_eq!(e.attrs[0], Value::Enum("UNSET".into()));
        assert_eq!(e.attrs[1], Value::Str("\u{e4}".into()));
        assert_eq!(e.attrs[3], Value::Int(-2));
        assert_eq!(e.attrs[5], Value::Ref(7));
        assert_eq!(
            e.attrs[6],
            Value::Typed(
                "IFCA".into(),
                Box::new(Value::List(vec![Value::Int(1), Value::Int(2)]))
            )
        );
    }
}

#[cfg(test)]
mod fuzz {
    use super::*;
    use proptest::prelude::*;

    const SEED: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCCARTESIANPOINT((0.,1.5E2,-3.));\n#2=IFCPROJECT('g',$,'n \\X2\\00E4\\X0\\ it''s',$,$,$,$,(#1),#3);\n#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n#4=(A()B(1));\nENDSEC;\nEND-ISO-10303-21;\n";

    fn walk(x: &StepIndex) {
        let ids: Vec<u64> = x.iter().map(|(i, _)| i).collect();
        for id in ids {
            let _ = x.entity(id);
        }
        let _ = x.schemas();
    }

    proptest! {
        #[test]
        fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
            if let Ok(x) = StepIndex::from_bytes(bytes, Limits::default()) { walk(&x); }
        }

        #[test]
        fn random_bytes_after_a_valid_header_never_panic(tail in proptest::collection::vec(any::<u8>(), 0..400)) {
            let mut data = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n".to_vec();
            data.extend(tail);
            if let Ok(x) = StepIndex::from_bytes(data, Limits::default()) { walk(&x); }
        }

        #[test]
        fn truncated_files_never_panic(cut in 0usize..SEED.len()) {
            if let Ok(x) = StepIndex::from_bytes(SEED.as_bytes()[..cut].to_vec(), Limits::default()) { walk(&x); }
        }

        #[test]
        fn mutated_files_never_panic(at in 0usize..SEED.len(), b in any::<u8>(), n in 1usize..4) {
            let mut data = SEED.as_bytes().to_vec();
            for k in 0..n {
                let p = (at + k * 7) % data.len();
                data[p] = b.wrapping_add(k as u8);
            }
            if let Ok(x) = StepIndex::from_bytes(data, Limits::default()) { walk(&x); }
        }

        #[test]
        fn nested_junk_never_panics(open in 0usize..300, s in "[(),'#$*.A-Z0-9 ;/-]{0,60}") {
            let body = format!("#1=IFCX({}{});\n#2=IFCY({s});", "(".repeat(open), s);
            let data = format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{body}\nENDSEC;\n");
            if let Ok(x) = StepIndex::from_bytes(data.into_bytes(), Limits::default()) { walk(&x); }
        }
    }
}
