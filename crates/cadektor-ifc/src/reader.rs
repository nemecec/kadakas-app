//! A reader for large STEP physical files (ISO 10303-21): one pass to build an index, then lazy
//! parsing of single entities.
//!
//! [`step::parse`](crate::step::parse) reads a whole file into memory as parsed values. This reader
//! does not. [`StepIndex::from_file`] reads the file once, in blocks, and keeps only a 24 byte record
//! per entity (ID, byte range, class). The file stays on disk. [`StepIndex::entity`] reads the bytes
//! of one entity and parses them when a caller asks for it, and keeps the result in a cache. So a
//! file of 1 GB with 10 million entities needs about 240 MB for the index, not the file size.
//! [`StepIndex::from_bytes`] and [`StepIndex::from_reader`] keep the bytes in memory instead (for
//! data that is in memory already).
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
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::rc::Rc;

use crate::step::{Entity, Value, decode_string};

/// Caps for one file.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Largest file in bytes (at most 4 GiB: the index keeps 32 bit offsets).
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
            max_bytes: 1 << 30,
            max_entities: 20_000_000,
            max_depth: 48,
            max_list: 20_000_000,
        }
    }
}

/// One entity in the index. Offsets are bytes from the start of the file.
#[derive(Clone, Copy, Debug)]
struct Rec {
    id: u64,
    /// Start of the `(` of the attributes.
    body_start: u32,
    /// Index of the `;`.
    end: u32,
    /// Index in the table of class names.
    name: u16,
}

/// Where the bytes are.
enum Store {
    Mem(Vec<u8>),
    File(RefCell<std::fs::File>),
}

/// An index over the entities of a STEP file.
pub struct StepIndex {
    store: Store,
    /// Sorted by ID.
    recs: Vec<Rec>,
    /// Upper-case class names of the entities. The empty name is a complex entity.
    names: Vec<String>,
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

/// One statement of the file: where it starts, the index of its `;`, and its first bytes.
struct Stmt {
    start: u64,
    end: u64,
    prefix: Vec<u8>,
}

/// Cuts a stream into statements without keeping more than the first `cap` bytes of each.
/// Strings and comments may contain `;`.
struct Statements<R: BufRead> {
    r: R,
    pos: u64,
    max: u64,
    too_large: bool,
}

impl<R: BufRead> Statements<R> {
    /// The next statement. `Err(true)` is a cut-off statement at the end of the data, `Err(false)` the end.
    fn next(&mut self, cap: usize) -> Result<Stmt, bool> {
        // 0: between statements, 1: in a statement, 2: in a string, 3: in a comment.
        let (mut state, mut ret, mut slash, mut star) = (0u8, 0u8, false, false);
        let mut prefix: Vec<u8> = Vec::new();
        let mut start = 0u64;
        loop {
            let (used, done) = {
                let buf = match self.r.fill_buf() {
                    Ok(b) => b,
                    Err(_) => return Err(state != 0),
                };
                if buf.is_empty() {
                    return Err(state != 0);
                }
                let mut used = 0;
                let mut done = None;
                for &c in buf {
                    used += 1;
                    let at = self.pos + used as u64 - 1;
                    if state == 3 {
                        if star && c == b'/' {
                            state = ret;
                        }
                        star = c == b'*';
                        continue;
                    }
                    if state == 2 {
                        if c == b'\'' {
                            state = 1;
                        }
                        if prefix.len() < cap {
                            prefix.push(c);
                        }
                        continue;
                    }
                    if slash {
                        slash = false;
                        if c == b'*' {
                            ret = state;
                            state = 3;
                            star = false;
                            continue;
                        }
                        // Not a comment: the slash is part of the statement.
                        if state == 0 {
                            start = at - 1;
                            state = 1;
                        }
                        if prefix.len() < cap {
                            prefix.push(b'/');
                        }
                    }
                    if state == 0 && is_space(c) {
                        continue;
                    }
                    if c == b'/' {
                        slash = true;
                        continue;
                    }
                    if state == 0 {
                        start = at;
                        state = 1;
                    }
                    if c == b';' {
                        done = Some(at);
                        break;
                    }
                    if c == b'\'' {
                        state = 2;
                    }
                    if prefix.len() < cap {
                        prefix.push(c);
                    }
                }
                (used, done)
            };
            self.r.consume(used);
            self.pos += used as u64;
            if self.pos > self.max {
                self.too_large = true;
                return Err(false);
            }
            if let Some(end) = done {
                return Ok(Stmt { start, end, prefix });
            }
        }
    }
}

/// Reads `#id = NAME (` from the first bytes of a statement. Gives the ID, the class name and the
/// offset of the `(` inside the statement (`name` is empty for a complex entity).
fn head(d: &[u8]) -> Option<(u64, &[u8], usize)> {
    if d.first() != Some(&b'#') {
        return None;
    }
    let mut i = 1;
    let s = i;
    while i < d.len() && d[i].is_ascii_digit() {
        i += 1;
    }
    let id: u64 = std::str::from_utf8(&d[s..i]).ok()?.parse().ok()?;
    i = skip_ws(d, i);
    if d.get(i) != Some(&b'=') {
        return None;
    }
    i = skip_ws(d, i + 1);
    if i >= d.len() {
        return None;
    }
    if d[i] == b'(' {
        return Some((id, &d[i..i], i));
    }
    let ns = i;
    while i < d.len() && is_word(d[i]) {
        i += 1;
    }
    if i == ns || i - ns > 200 {
        return None;
    }
    let name = &d[ns..i];
    i = skip_ws(d, i);
    if i >= d.len() || d[i] != b'(' {
        return None;
    }
    Some((id, name, i))
}

impl StepIndex {
    /// Indexes a file on disk. The file is read once in blocks and stays on disk.
    pub fn from_file(path: &Path, limits: Limits) -> Result<StepIndex, StepError> {
        let file = std::fs::File::open(path)
            .map_err(|e| StepError::Io(format!("cannot read {}: {e}", path.display())))?;
        let len = file
            .metadata()
            .map_err(|e| StepError::Io(e.to_string()))?
            .len();
        if len > limits.max_bytes as u64 {
            return Err(StepError::TooLarge(limits.max_bytes));
        }
        let mut idx = StepIndex::empty(
            Store::File(RefCell::new(
                file.try_clone().map_err(|e| StepError::Io(e.to_string()))?,
            )),
            limits,
        );
        idx.scan(BufReader::with_capacity(1 << 20, file))?;
        Ok(idx)
    }

    /// Reads the whole stream (up to the size cap) into memory and indexes it.
    pub fn from_reader<R: Read>(r: R, limits: Limits) -> Result<StepIndex, StepError> {
        let mut data = Vec::new();
        let cap = limits.max_bytes as u64;
        r.take(cap + 1)
            .read_to_end(&mut data)
            .map_err(|e| StepError::Io(e.to_string()))?;
        StepIndex::from_bytes(data, limits)
    }

    /// Indexes bytes that are in memory already.
    pub fn from_bytes(data: Vec<u8>, limits: Limits) -> Result<StepIndex, StepError> {
        if data.len() > limits.max_bytes {
            return Err(StepError::TooLarge(limits.max_bytes));
        }
        let mut idx = StepIndex::empty(Store::Mem(Vec::new()), limits);
        idx.scan(Cursor::new(&data[..]))?;
        idx.store = Store::Mem(data);
        Ok(idx)
    }

    fn empty(store: Store, mut limits: Limits) -> StepIndex {
        limits.max_bytes = limits.max_bytes.min(u32::MAX as usize);
        StepIndex {
            store,
            recs: Vec::new(),
            names: Vec::new(),
            header: String::new(),
            notes: Vec::new(),
            limits,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn scan<R: BufRead>(&mut self, r: R) -> Result<(), StepError> {
        let mut st = Statements {
            r,
            pos: 0,
            max: self.limits.max_bytes as u64,
            too_large: false,
        };
        let too_large = |st: &Statements<R>, lim: usize| st.too_large.then_some(lim);
        // The first statement.
        let first = st.next(64 * 1024);
        if let Some(l) = too_large(&st, self.limits.max_bytes) {
            return Err(StepError::TooLarge(l));
        }
        let first =
            first.map_err(|_| StepError::Format("the file ends in the first line".into()))?;
        let p = first
            .prefix
            .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
            .unwrap_or(&first.prefix);
        if !p.starts_with(b"ISO-10303-21") {
            return Err(StepError::Format(
                "the file does not start with ISO-10303-21".into(),
            ));
        }
        // The header: statements up to `DATA;`.
        let mut header = String::new();
        loop {
            let s = match st.next(64 * 1024) {
                Ok(s) => s,
                Err(_) => {
                    if let Some(l) = too_large(&st, self.limits.max_bytes) {
                        return Err(StepError::TooLarge(l));
                    }
                    return Err(StepError::Format("no DATA section".into()));
                }
            };
            if s.prefix.starts_with(b"DATA") && s.prefix[4..].iter().all(|c| is_space(*c)) {
                break;
            }
            header.push_str(&String::from_utf8_lossy(&s.prefix));
            header.push_str(";\n");
        }
        self.header = header;

        let mut bad = 0usize;
        let mut cut = true;
        let mut recs: Vec<Rec> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut name_ids: HashMap<Vec<u8>, u16> = HashMap::new();
        loop {
            let s = match st.next(600) {
                Ok(s) => s,
                Err(_) => {
                    if let Some(l) = too_large(&st, self.limits.max_bytes) {
                        return Err(StepError::TooLarge(l));
                    }
                    break;
                }
            };
            if s.prefix.starts_with(b"ENDSEC") {
                cut = false;
                break;
            }
            if recs.len() >= self.limits.max_entities {
                self.notes.push(format!(
                    "the file has more than {} entities: the rest is ignored",
                    self.limits.max_entities
                ));
                cut = false;
                break;
            }
            match head(&s.prefix) {
                Some((id, name, open)) => {
                    let key = name.to_ascii_uppercase();
                    let n = match name_ids.get(&key) {
                        Some(n) => *n,
                        None if names.len() < u16::MAX as usize => {
                            names.push(String::from_utf8_lossy(&key).into_owned());
                            name_ids.insert(key, (names.len() - 1) as u16);
                            (names.len() - 1) as u16
                        }
                        None => {
                            bad += 1;
                            continue;
                        }
                    };
                    recs.push(Rec {
                        id,
                        body_start: (s.start + open as u64) as u32,
                        end: s.end as u32,
                        name: n,
                    });
                }
                None => bad += 1,
            }
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
        // Sorted by ID. The first of two entities with one ID stays.
        if !recs.windows(2).all(|w| w[0].id < w[1].id) {
            recs.sort_by_key(|r| r.id);
        }
        let before = recs.len();
        recs.dedup_by_key(|r| r.id);
        if recs.len() < before {
            self.notes.push(format!(
                "{} entities repeat an ID: the first one is used",
                before - recs.len()
            ));
        }
        self.recs = recs;
        self.names = names;
        Ok(())
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

    fn rec(&self, id: u64) -> Option<&Rec> {
        self.recs
            .binary_search_by_key(&id, |r| r.id)
            .ok()
            .map(|i| &self.recs[i])
    }

    /// The upper-case name of an entity, without parsing its attributes. Complex entities give `""`.
    pub fn name_of(&self, id: u64) -> Option<&str> {
        Some(&self.names[self.rec(id)?.name as usize])
    }

    /// Every `(id, name)` in the order of the IDs.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &str)> {
        self.recs
            .iter()
            .map(|r| (r.id, self.names[r.name as usize].as_str()))
    }

    /// The IDs of all entities with this upper-case name.
    pub fn ids_of(&self, name: &str) -> Vec<u64> {
        self.iter()
            .filter(|(_, n)| n.eq_ignore_ascii_case(name))
            .map(|(i, _)| i)
            .collect()
    }

    /// The bytes from the `(` to the `;` of an entity.
    fn bytes(&self, r: &Rec) -> Option<Vec<u8>> {
        let (a, b) = (r.body_start as usize, r.end as usize);
        match &self.store {
            Store::Mem(d) => d.get(a..b).map(<[u8]>::to_vec),
            Store::File(f) => {
                let mut f = f.borrow_mut();
                f.seek(SeekFrom::Start(a as u64)).ok()?;
                let mut buf = vec![0u8; b.checked_sub(a)?];
                f.read_exact(&mut buf).ok()?;
                Some(buf)
            }
        }
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
        let r = *self.rec(id)?;
        let data = self.bytes(&r)?;
        let mut p = ValueParser {
            d: &data,
            i: 0,
            max_depth: self.limits.max_depth,
            max_list: self.limits.max_list,
        };
        let name = &self.names[r.name as usize];
        if name.is_empty() {
            // Complex entity: `(NAME(args) NAME(args))`.
            p.i += 1;
            let mut parts = Vec::new();
            loop {
                p.ws();
                if p.peek()? == b')' {
                    break;
                }
                let part = p.word()?;
                p.ws();
                let args = p.value(1).ok()?;
                parts.push(Value::Typed(part, Box::new(args)));
            }
            return Some(Entity {
                id,
                name: "COMPLEX".into(),
                attrs: parts,
            });
        }
        let attrs = match p.value(0).ok()? {
            Value::List(items) => items,
            _ => return None,
        };
        Some(Entity {
            id,
            name: name.clone(),
            attrs,
        })
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

#[cfg(test)]
mod stream_tests {
    use super::*;

    const FILE: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n/* c; 'x */ #1=IFCCARTESIANPOINT((0.,1.5E2,-3.));\n#2=IFCLABEL('it''s; ok'); /* tail */\n#3 = IFCPROJECT ( $ , *, .T., (#1,#2), IFCLABEL('a'), 7 ) ;\n#4=(A()B(1));\n#5=IFCX('a/*b');\nENDSEC;\nEND-ISO-10303-21;\n";

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("cadektor-reader-{}-{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn a_file_is_indexed_from_disk_and_read_on_demand() {
        let p = temp("a.ifc", FILE.as_bytes());
        let x = StepIndex::from_file(&p, Limits::default()).unwrap();
        let m = StepIndex::from_bytes(FILE.as_bytes().to_vec(), Limits::default()).unwrap();
        assert_eq!(x.len(), 5);
        assert_eq!(x.schemas(), vec!["IFC4"]);
        for id in 1..=5 {
            assert_eq!(x.entity(id), m.entity(id), "#{id}");
        }
        assert_eq!(x.entity(5).unwrap().attrs[0], Value::Str("a/*b".into()));
        assert!(matches!(x.store, Store::File(_)));
        // The file is gone: the entities that are cached still answer, the others do not.
        std::fs::remove_file(&p).unwrap();
        assert!(x.entity(1).is_some());
        assert!(StepIndex::from_file(&p, Limits::default()).is_err());
    }

    #[test]
    fn blocks_of_any_size_give_the_same_index() {
        let whole = StepIndex::from_bytes(FILE.as_bytes().to_vec(), Limits::default()).unwrap();
        for cap in [1, 2, 3, 5, 7, 16, 100] {
            let mut x = StepIndex::empty(Store::Mem(FILE.as_bytes().to_vec()), Limits::default());
            x.scan(BufReader::with_capacity(cap, Cursor::new(FILE.as_bytes())))
                .unwrap();
            assert_eq!(x.len(), whole.len(), "block {cap}");
            assert_eq!(x.header(), whole.header(), "block {cap}");
            for id in 1..=5 {
                assert_eq!(x.entity(id), whole.entity(id), "block {cap} #{id}");
            }
        }
    }

    #[test]
    fn the_size_limit_holds_while_reading() {
        let p = temp("big.ifc", FILE.as_bytes());
        let small = Limits {
            max_bytes: 40,
            ..Limits::default()
        };
        assert_eq!(
            StepIndex::from_file(&p, small).err(),
            Some(StepError::TooLarge(40))
        );
        // A stream that is longer than its limit stops, whatever the blocks.
        let mut x = StepIndex::empty(
            Store::Mem(Vec::new()),
            Limits {
                max_bytes: 60,
                ..Limits::default()
            },
        );
        let r = x.scan(BufReader::with_capacity(4, Cursor::new(FILE.as_bytes())));
        assert_eq!(r.err(), Some(StepError::TooLarge(60)));
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn the_index_is_small() {
        assert!(std::mem::size_of::<Rec>() <= 24);
    }

    #[test]
    fn entities_out_of_order_are_sorted() {
        let f =
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#9=IFCA();\n#2=IFCB();\n#5=IFCC();\nENDSEC;\n";
        let x = StepIndex::from_bytes(f.as_bytes().to_vec(), Limits::default()).unwrap();
        assert_eq!(
            x.iter()
                .map(|(i, n)| (i, n.to_string()))
                .collect::<Vec<_>>(),
            vec![
                (2, "IFCB".to_string()),
                (5, "IFCC".to_string()),
                (9, "IFCA".to_string())
            ]
        );
        assert_eq!(x.name_of(5), Some("IFCC"));
        assert!(x.entity(5).is_some() && x.entity(6).is_none());
    }
}
