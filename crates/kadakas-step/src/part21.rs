//! STEP physical files (ISO 10303-21): the value and entity model, the string encoding, a streaming
//! writer and a parser. The parser is safe for files that nobody trusts: it limits the nesting
//! of values and the number of entities, and it never panics.
//!
//! [`Entity`] and [`Value`] are the common model. The writer prints them. A future IFC reader gets
//! them from [`parse`].

use std::io::{self, Write};

use crate::codepages;

/// An attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `$`: not set.
    Null,
    /// `*`: derived.
    Derived,
    Int(i64),
    Real(f64),
    Str(String),
    /// An enumeration literal without the dots, for example `BASESLAB`.
    Enum(String),
    /// `.T.` or `.F.`.
    Bool(bool),
    /// `#12`.
    Ref(u64),
    List(Vec<Value>),
    /// A defined type with a value, for example `IFCLABEL('x')`.
    Typed(String, Box<Value>),
}

impl Value {
    pub fn s(text: impl Into<String>) -> Value {
        Value::Str(text.into())
    }

    pub fn e(name: &str) -> Value {
        Value::Enum(name.to_string())
    }

    pub fn typed(name: &str, v: Value) -> Value {
        Value::Typed(name.to_string(), Box::new(v))
    }

    pub fn refs(ids: impl IntoIterator<Item = u64>) -> Value {
        Value::List(ids.into_iter().map(Value::Ref).collect())
    }

    pub fn reals(xs: &[f64]) -> Value {
        Value::List(xs.iter().map(|x| Value::Real(*x)).collect())
    }

    /// Prints the value in STEP syntax.
    pub fn write(&self, out: &mut dyn Write) -> io::Result<()> {
        match self {
            Value::Null => out.write_all(b"$"),
            Value::Derived => out.write_all(b"*"),
            Value::Int(i) => write!(out, "{i}"),
            Value::Real(x) => out.write_all(format_real(*x).as_bytes()),
            Value::Str(s) => write!(out, "'{}'", encode_string(s)),
            Value::Enum(e) => write!(out, ".{e}."),
            Value::Bool(b) => out.write_all(if *b { b".T." } else { b".F." }),
            Value::Ref(id) => write!(out, "#{id}"),
            Value::List(items) => {
                out.write_all(b"(")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.write_all(b",")?;
                    }
                    v.write(out)?;
                }
                out.write_all(b")")
            }
            Value::Typed(name, v) => {
                write!(out, "{name}(")?;
                v.write(out)?;
                out.write_all(b")")
            }
        }
    }

    /// The target of a reference.
    pub fn as_ref_id(&self) -> Option<u64> {
        if let Value::Ref(id) = self {
            Some(*id)
        } else {
            None
        }
    }

    /// Calls `f` with every reference inside the value.
    pub fn each_ref(&self, f: &mut dyn FnMut(u64)) {
        match self {
            Value::Ref(id) => f(*id),
            Value::List(items) => items.iter().for_each(|v| v.each_ref(f)),
            Value::Typed(_, v) => v.each_ref(f),
            _ => {}
        }
    }
}

/// The name of an entity that has several parts (a complex instance, for example
/// `#5=(BOUNDED_SURFACE() B_SPLINE_SURFACE(...) ...)`). Its attributes are one
/// `Value::Typed(part name, Value::List(attributes))` for each part, in the order of the file.
pub const COMPLEX: &str = "(COMPLEX)";

/// One entity instance of the data section.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: u64,
    /// The upper-case entity name, for example `IFCSLAB`.
    pub name: String,
    pub attrs: Vec<Value>,
}

impl Entity {
    /// The attributes of the part `name` of a complex instance, or of the entity itself when it
    /// is a simple instance with this name.
    pub fn part(&self, name: &str) -> Option<&[Value]> {
        if self.name == COMPLEX {
            self.attrs.iter().find_map(|v| match v {
                Value::Typed(n, inner) if n == name => match inner.as_ref() {
                    Value::List(items) => Some(items.as_slice()),
                    _ => None,
                },
                _ => None,
            })
        } else if self.name == name {
            Some(self.attrs.as_slice())
        } else {
            None
        }
    }

    /// True when the entity is the type `name`, or has a part of this type.
    pub fn is(&self, name: &str) -> bool {
        self.part(name).is_some()
    }
}

/// Prints a real number in STEP syntax: it always has a decimal point, and large or small
/// numbers use an exponent. Numbers that are not finite print as `0.`.
pub fn format_real(x: f64) -> String {
    if !x.is_finite() || x == 0.0 {
        return "0.".into();
    }
    let a = x.abs();
    if (1e-4..1e15).contains(&a) {
        let s = format!("{x}");
        if s.contains('.') { s } else { format!("{s}.") }
    } else {
        let s = format!("{x:E}");
        let (mantissa, exp) = s.split_once('E').unwrap_or((&s, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.")
        };
        format!("{mantissa}E{exp}")
    }
}

/// Encodes text for a STEP string: `'` and `\` are doubled, and characters outside printable ASCII use
/// the `\X2\...\X0\` form (UTF-16, four hex digits per unit).
pub fn encode_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut wide = false;
    let close = |out: &mut String, wide: &mut bool| {
        if *wide {
            out.push_str("\\X0\\");
            *wide = false;
        }
    };
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            close(&mut out, &mut wide);
            match c {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                c => out.push(c),
            }
        } else {
            if !wide {
                out.push_str("\\X2\\");
                wide = true;
            }
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("{unit:04X}"));
            }
        }
    }
    close(&mut out, &mut wide);
    out
}

/// Reads the text of a STEP string (without the quotes). It understands `''`, `\\`, `\X\hh` (always
/// ISO 8859-1, not the page of `\P\`; the two digits must be hexadecimal),
/// `\S\c` (a character of the upper half of the code page), `\P<letter>\` (selects the ISO 8859
/// part A to I, part 1 at first), `\X2\...\X0\` (UTF-16) and `\X4\...\X0\` (UTF-32). An escape
/// that is not complete or not valid stays as it is, so no text is lost.
pub fn decode_string(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut page = 0usize;
    // Exactly hexadecimal digits: `from_str_radix` alone would accept a sign.
    let hex = |digits: &[u8]| {
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    };
    while i < b.len() {
        match b[i] {
            b'\'' if b.get(i + 1) == Some(&b'\'') => {
                out.push('\'');
                i += 2;
            }
            b'\\' if b.get(i + 1) == Some(&b'\\') => {
                out.push('\\');
                i += 2;
            }
            b'\\' if b[i..].starts_with(b"\\X\\") && i + 5 <= b.len() => {
                // `\X\hh` is the character with this code of ISO 8859-1 (Latin-1), whatever page
                // `\P..\` chose: only `\S\` follows the page (as IfcOpenShell reads it too).
                match hex(&b[i + 3..i + 5]).and_then(char::from_u32) {
                    Some(c) => {
                        out.push(c);
                        i += 5;
                    }
                    None => {
                        out.push('\\');
                        i += 1;
                    }
                }
            }
            b'\\'
                if b[i..].starts_with(b"\\S\\")
                    && b.get(i + 3).is_some_and(|c| (0x20..0x7F).contains(c)) =>
            {
                out.push(codepages::char_of(page, b[i + 3] | 0x80));
                // The character after `\S\` is written as in any string: an apostrophe or a
                // backslash is doubled, and both bytes belong to the escape.
                let doubled = matches!(b[i + 3], b'\'' | b'\\') && b.get(i + 4) == Some(&b[i + 3]);
                i += if doubled { 5 } else { 4 };
            }
            b'\\'
                if b.get(i + 1) == Some(&b'P')
                    && b.get(i + 3) == Some(&b'\\')
                    && b.get(i + 2).and_then(|l| codepages::page_of(*l)).is_some() =>
            {
                page = codepages::page_of(b[i + 2]).unwrap_or(0);
                i += 4;
            }
            b'\\' if b[i..].starts_with(b"\\X2\\") || b[i..].starts_with(b"\\X4\\") => {
                let width = if b[i + 2] == b'2' { 4 } else { 8 };
                let end = s[i + 4..].find("\\X0\\").map(|p| p + i + 4);
                let Some(end) = end else {
                    out.push('\\');
                    i += 1;
                    continue;
                };
                let digits = &b[i + 4..end];
                if width == 4 {
                    let units: Vec<u16> = digits
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter_map(|d| hex(d).map(|u| u as u16))
                        .collect();
                    out.extend(char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')));
                } else {
                    out.extend(
                        digits
                            .as_chunks::<8>()
                            .0
                            .iter()
                            .filter_map(|d| hex(d).and_then(char::from_u32)),
                    );
                }
                i = end + 4;
            }
            _ => {
                // Copy one whole UTF-8 character.
                let len = s[i..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&s[i..i + len]);
                i += len;
            }
        }
    }
    // A NUL (`\X\00`, `\X2\0000\X0\`) is no text: it is replaced by U+FFFD.
    if out.contains('\0') {
        out = out.replace('\0', "\u{FFFD}");
    }
    out
}

/// Writes a STEP file one entity at a time. Nothing is kept in memory except the counter.
pub struct StepWriter<W: Write> {
    out: W,
    next: u64,
}

impl<W: Write> StepWriter<W> {
    /// Starts an IFC4 file: writes the header section and opens the data section.
    pub fn begin(
        out: W,
        file_name: &str,
        time_stamp: &str,
        author: &str,
        organization: &str,
        view: &str,
    ) -> io::Result<Self> {
        Self::begin_schema(
            out,
            file_name,
            time_stamp,
            author,
            organization,
            view,
            "IFC4",
        )
    }

    /// Like `begin`, with the name of the schema for `FILE_SCHEMA`.
    pub fn begin_schema(
        mut out: W,
        file_name: &str,
        time_stamp: &str,
        author: &str,
        organization: &str,
        view: &str,
        schema: &str,
    ) -> io::Result<Self> {
        writeln!(out, "ISO-10303-21;")?;
        writeln!(out, "HEADER;")?;
        writeln!(out, "FILE_DESCRIPTION(('{}'),'2;1');", encode_string(view))?;
        writeln!(
            out,
            "FILE_NAME('{}','{}',('{}'),('{}'),'Kadakas','Kadakas','');",
            encode_string(file_name),
            encode_string(time_stamp),
            encode_string(author),
            encode_string(organization)
        )?;
        writeln!(out, "FILE_SCHEMA(('{}'));", encode_string(schema))?;
        writeln!(out, "ENDSEC;")?;
        writeln!(out, "DATA;")?;
        Ok(StepWriter { out, next: 1 })
    }

    /// The ID that the next entity gets.
    pub fn next_id(&self) -> u64 {
        self.next
    }

    /// Writes an entity and returns its ID.
    pub fn entity(&mut self, name: &str, attrs: &[Value]) -> io::Result<u64> {
        self.entity_with(name, |out| {
            for (i, v) in attrs.iter().enumerate() {
                if i > 0 {
                    out.write_all(b",")?;
                }
                v.write(out)?;
            }
            Ok(())
        })
    }

    /// Writes an entity whose attribute text comes from `body`. Use it for big lists that should not
    /// be built in memory.
    pub fn entity_with(
        &mut self,
        name: &str,
        body: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> io::Result<u64> {
        let id = self.next;
        write!(self.out, "#{id}={name}(")?;
        body(&mut self.out)?;
        self.out.write_all(b");\n")?;
        self.next += 1;
        Ok(id)
    }

    /// Closes the data section and returns the output.
    pub fn finish(mut self) -> io::Result<W> {
        writeln!(self.out, "ENDSEC;")?;
        writeln!(self.out, "END-ISO-10303-21;")?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// A parsed file.
#[derive(Clone, Debug, PartialEq)]
pub struct StepFile {
    /// The text of the header section, without `HEADER;` and `ENDSEC;`.
    pub header: String,
    pub entities: Vec<Entity>,
    /// Entity numbers that the file defines more than once. The first definition is kept.
    pub duplicates: Vec<u64>,
}

/// The schema names of `FILE_SCHEMA` in the text of a header. The keyword is found in any case
/// (some exporters write `file_schema`), outside of strings.
pub fn header_schemas(header: &str) -> Vec<String> {
    let Some(at) = find_keyword(header, "FILE_SCHEMA") else {
        return Vec::new();
    };
    let rest = &header[at..];
    // The statement ends at the first semicolon outside of a string.
    let (mut end, mut quote) = (rest.len(), false);
    for (i, c) in rest.char_indices() {
        match c {
            '\'' => quote = !quote,
            ';' if !quote => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut parts = rest[..end].split('\'');
    parts.next();
    while let Some(name) = parts.next() {
        out.push(name.to_string());
        parts.next();
    }
    out
}

/// The place of a header keyword (upper case) in any case, outside of strings and comments
/// (`/* ... */`) and on a word start.
pub fn find_keyword(header: &str, keyword: &str) -> Option<usize> {
    let (b, k) = (header.as_bytes(), keyword.as_bytes());
    let mut quote = false;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\'' {
            quote = !quote;
        } else if !quote && b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            // A comment: skip to its end (an unfinished comment hides the rest).
            match header[i + 2..].find("*/") {
                Some(end) => i += end + 3,
                None => return None,
            }
        } else if !quote
            && b[i..].len() >= k.len()
            && b[i..i + k.len()].eq_ignore_ascii_case(k)
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

impl StepFile {
    /// The schema names of `FILE_SCHEMA`.
    pub fn schemas(&self) -> Vec<String> {
        header_schemas(&self.header)
    }

    /// The entity with this ID.
    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.entities
            .binary_search_by_key(&id, |e| e.id)
            .ok()
            .map(|i| &self.entities[i])
    }

    /// References to IDs that no entity defines: pairs of the entity and the missing target.
    pub fn dangling(&self) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for e in &self.entities {
            for v in &e.attrs {
                v.each_ref(&mut |r| {
                    if self.get(r).is_none() {
                        out.push((e.id, r));
                    }
                });
            }
        }
        out
    }
}

/// The deepest nesting of lists and typed values that the parser accepts.
pub const MAX_DEPTH: usize = 48;

/// The most entities that [`parse`] reads.
pub const MAX_ENTITIES: usize = 20_000_000;

struct Parser<'a> {
    s: &'a str,
    i: usize,
    depth: usize,
    /// Estimated bytes that the parsed values take in memory.
    cost: u64,
}

/// Bytes that one value takes, about (the value itself and the share of its list).
const VALUE_BYTES: u64 = 40;
/// Bytes of an entity without its values.
const ENTITY_BYTES: u64 = 96;

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.i).copied()
    }

    /// Skips white space and comments (`/* ... */`).
    fn skip_space(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self.s.as_bytes()[self.i.min(self.s.len())..].starts_with(b"/*") {
                match self.s[self.i + 2..].find("*/") {
                    Some(n) => self.i += n + 4,
                    None => self.i = self.s.len(),
                }
            } else {
                return;
            }
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.skip_space();
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }

    fn word(&mut self) -> &str {
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            self.i += 1;
        }
        &self.s[start..self.i]
    }

    /// A name in upper case: STEP names are not case sensitive, and some exporters write them
    /// in lower case.
    fn name(&mut self) -> String {
        self.word().to_ascii_uppercase()
    }

    /// The attributes after the opening bracket of an entity, up to the closing bracket.
    fn attr_list(&mut self, id: u64) -> Result<Vec<Value>, String> {
        let mut attrs = Vec::new();
        self.skip_space();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok(attrs);
        }
        loop {
            attrs.push(self.value()?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Ok(attrs);
                }
                _ => {
                    return Err(format!(
                        "entity #{id}: expected ',' or ')' at byte {}",
                        self.i
                    ));
                }
            }
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        if self.depth >= MAX_DEPTH {
            return Err(format!("values are nested deeper than {MAX_DEPTH} levels"));
        }
        self.depth += 1;
        self.cost += VALUE_BYTES;
        let r = self.value_inner();
        self.depth -= 1;
        r
    }

    fn value_inner(&mut self) -> Result<Value, String> {
        self.skip_space();
        let Some(c) = self.peek() else {
            return Err("unexpected end of file".into());
        };
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
                let n = self.word().to_string();
                n.parse()
                    .map(Value::Ref)
                    .map_err(|_| format!("bad reference #{n}"))
            }
            b'\'' => {
                self.i += 1;
                let start = self.i;
                loop {
                    match self.peek() {
                        None => return Err("string is not closed".into()),
                        Some(b'\'') if self.s.as_bytes().get(self.i + 1) == Some(&b'\'') => {
                            self.i += 2
                        }
                        Some(b'\'') => break,
                        Some(_) => self.i += 1,
                    }
                }
                let text = &self.s[start..self.i];
                self.i += 1;
                Ok(Value::Str(decode_string(text)))
            }
            b'.' => {
                self.i += 1;
                let w = self.name();
                self.eat(b'.')?;
                Ok(match w.as_str() {
                    "T" => Value::Bool(true),
                    "F" => Value::Bool(false),
                    _ => Value::Enum(w),
                })
            }
            b'(' => {
                self.i += 1;
                let mut items = Vec::new();
                self.skip_space();
                if self.peek() == Some(b')') {
                    self.i += 1;
                    return Ok(Value::List(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_space();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b')') => {
                            self.i += 1;
                            return Ok(Value::List(items));
                        }
                        _ => return Err(format!("expected ',' or ')' at byte {}", self.i)),
                    }
                }
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' => {
                let start = self.i;
                self.i += 1;
                while self.peek().is_some_and(|c| {
                    c.is_ascii_digit() || matches!(c, b'.' | b'E' | b'e' | b'-' | b'+')
                }) {
                    self.i += 1;
                }
                let t = &self.s[start..self.i];
                if t.contains(['.', 'E', 'e']) {
                    t.parse()
                        .map(Value::Real)
                        .map_err(|_| format!("bad number {t}"))
                } else {
                    t.parse()
                        .map(Value::Int)
                        .map_err(|_| format!("bad number {t}"))
                }
            }
            c if c.is_ascii_alphabetic() => {
                let name = self.name();
                self.skip_space();
                self.eat(b'(')?;
                let inner = self.value()?;
                self.skip_space();
                // Only one argument: a defined type.
                self.eat(b')')?;
                Ok(Value::Typed(name, Box::new(inner)))
            }
            c => Err(format!("unexpected '{}' at byte {}", c as char, self.i)),
        }
    }
}

/// The offset just after the `DATA;` of the first data section. Edition 3 files name the
/// section: `DATA('name',('schema'));`.
/// The first place of `needle` (upper case) in `s`, whatever the case of `s`.
fn find_ci(s: &str, needle: &str) -> Option<usize> {
    let n = needle.as_bytes();
    s.as_bytes()
        .windows(n.len())
        .position(|w| w.eq_ignore_ascii_case(n))
}

fn find_data(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = find_ci(&s[from..], "DATA") {
        let start = from + at;
        let before_ok = s[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        let rest = &s[start + 4..];
        let trimmed = rest.trim_start();
        if before_ok {
            if trimmed.starts_with(';') {
                return Some(start + 4 + (rest.len() - trimmed.len()) + 1);
            }
            if trimmed.starts_with('(')
                && let Some(end) = trimmed.find(");")
            {
                return Some(start + 4 + (rest.len() - trimmed.len()) + end + 2);
            }
        }
        from = start + 4;
    }
    None
}

/// Reads a STEP physical file. It handles the header and the simple entity instances of the data section.
pub fn parse(text: &str) -> Result<StepFile, String> {
    parse_limited(text, MAX_ENTITIES, u64::MAX).map_err(|e| e.to_string())
}

/// Why [`parse_limited`] stopped.
#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    /// The text is not a valid file. The message says where.
    Syntax(String),
    /// More entities than the limit.
    TooManyEntities { limit: usize },
    /// The entities would take more memory than the limit (an estimate, in bytes).
    TooMuchMemory { estimate: u64, limit: u64 },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Syntax(m) => f.write_str(m),
            ParseError::TooManyEntities { limit } => {
                write!(f, "the file has more than {limit} entities")
            }
            ParseError::TooMuchMemory { limit, .. } => {
                write!(
                    f,
                    "the file needs more than {limit} bytes of memory to read"
                )
            }
        }
    }
}

impl From<String> for ParseError {
    fn from(m: String) -> Self {
        ParseError::Syntax(m)
    }
}

impl From<&str> for ParseError {
    fn from(m: &str) -> Self {
        ParseError::Syntax(m.to_string())
    }
}

/// Like [`parse`], with the most entities and the most memory (an estimate, in bytes) that
/// the parsed file may take. The reader stops at once when a limit is passed, so a hostile
/// file cannot fill the memory before the limit is looked at.
pub fn parse_limited(
    text: &str,
    max_entities: usize,
    max_memory: u64,
) -> Result<StepFile, ParseError> {
    let header_start = find_ci(text, "HEADER;").ok_or("no HEADER section")? + "HEADER;".len();
    let header_end =
        find_ci(&text[header_start..], "ENDSEC;").ok_or("HEADER is not closed")? + header_start;
    let data_start = find_data(&text[header_end..]).ok_or("no DATA section")? + header_end;
    let mut p = Parser {
        s: text,
        i: data_start,
        depth: 0,
        cost: 0,
    };
    let mut entities = Vec::new();
    loop {
        p.skip_space();
        if p.s.as_bytes()[p.i..].len() >= 7
            && p.s.as_bytes()[p.i..p.i + 7].eq_ignore_ascii_case(b"ENDSEC;")
        {
            break;
        }
        if p.i >= p.s.len() {
            return Err("unexpected end of file: ENDSEC is missing".into());
        }
        if entities.len() >= max_entities.min(MAX_ENTITIES) {
            return Err(ParseError::TooManyEntities {
                limit: max_entities.min(MAX_ENTITIES),
            });
        }
        if p.cost > max_memory {
            return Err(ParseError::TooMuchMemory {
                estimate: p.cost,
                limit: max_memory,
            });
        }
        p.eat(b'#')?;
        let id: u64 = p
            .word()
            .parse()
            .map_err(|_| format!("bad entity number at byte {}", p.i))?;
        p.eat(b'=')?;
        p.skip_space();
        let (name, attrs) = if p.peek() == Some(b'(') {
            // A complex instance: several entity parts in one pair of brackets.
            p.i += 1;
            let mut parts = Vec::new();
            loop {
                p.skip_space();
                if p.peek() == Some(b')') {
                    p.i += 1;
                    break;
                }
                let part = p.name();
                if part.is_empty() {
                    return Err(format!("entity #{id}: a part has no name at byte {}", p.i).into());
                }
                p.eat(b'(')?;
                let attrs = p.attr_list(id)?;
                parts.push(Value::Typed(part, Box::new(Value::List(attrs))));
            }
            if parts.is_empty() {
                return Err(format!("entity #{id} has no parts").into());
            }
            (COMPLEX.to_string(), parts)
        } else {
            let name = p.name();
            if name.is_empty() {
                return Err(format!("entity #{id} has no name").into());
            }
            p.eat(b'(')?;
            (name, p.attr_list(id)?)
        };
        p.eat(b';')?;
        p.cost += ENTITY_BYTES + name.len() as u64;
        entities.push(Entity { id, name, attrs });
    }
    if p.cost > max_memory {
        return Err(ParseError::TooMuchMemory {
            estimate: p.cost,
            limit: max_memory,
        });
    }
    // A stable sort keeps the first definition of a number first.
    entities.sort_by_key(|e| e.id);
    let mut duplicates = Vec::new();
    entities.dedup_by(|later, first| {
        let same = later.id == first.id;
        if same && duplicates.last() != Some(&later.id) {
            duplicates.push(later.id);
        }
        same
    });
    Ok(StepFile {
        header: text[header_start..header_end].to_string(),
        entities,
        duplicates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(v: &Value) -> String {
        let mut out = Vec::new();
        v.write(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(encode_string("plain"), "plain");
        assert_eq!(encode_string("it's"), "it''s");
        assert_eq!(encode_string("a\\b"), "a\\\\b");
        assert_eq!(
            encode_string("Pähkinä"),
            "P\\X2\\00E4\\X0\\hkin\\X2\\00E4\\X0\\"
        );
        assert_eq!(encode_string("äö"), "\\X2\\00E400F6\\X0\\");
        assert_eq!(encode_string("a\nb"), "a\\X2\\000A\\X0\\b");
        // A character outside the BMP becomes a surrogate pair.
        assert_eq!(encode_string("\u{1F333}"), "\\X2\\D83CDF33\\X0\\");
        assert!(encode_string("tree \u{1F333} ü").is_ascii());
    }

    #[test]
    fn strings_round_trip() {
        for s in [
            "",
            "abc",
            "it's \\ here",
            "Pähkinäpuu",
            "\u{1F333} tree",
            "line\nbreak",
            "日本語",
        ] {
            assert_eq!(decode_string(&encode_string(s)), s, "{s:?}");
        }
        assert_eq!(decode_string("\\X\\E4"), "ä");
        assert_eq!(decode_string("\\X4\\0001F333\\X0\\"), "\u{1F333}");
        // Broken escapes stay as they are.
        assert_eq!(decode_string("\\X2\\00E4"), "\\X2\\00E4");
        assert_eq!(decode_string("\\X\\zz"), "\\X\\zz");
        assert_eq!(decode_string("\\Q"), "\\Q");
    }

    #[test]
    fn all_string_escapes_are_decoded() {
        // \X\hh: the apostrophe of the buildingSMART landscaping scene, and a code of the upper half.
        assert_eq!(decode_string("earth\\X\\27s"), "earth's");
        assert_eq!(decode_string("\\X\\41\\X\\E4"), "A\u{E4}");
        // \S\c adds 128 to the code of c.
        assert_eq!(decode_string("\\S\\d\\S\\|"), "\u{E4}\u{FC}");
        // \P<letter>\ selects the part of ISO 8859: part 2 (B) has a-ogonek at 0xB1.
        assert_eq!(decode_string("\\PB\\\\S\\1"), "\u{105}");
        // \X\hh is Latin-1 whatever the page (the plus sign of from_str_radix is no digit).
        assert_eq!(decode_string("\\PB\\\\X\\B1"), "\u{B1}");
        assert_eq!(decode_string("\\X\\+1"), "\\X\\+1");
        assert_eq!(decode_string("\\X\\-1"), "\\X\\-1");
        assert_eq!(decode_string("\\X\\4g"), "\\X\\4g");
        // The doubled apostrophe and backslash after \S\ are one character.
        assert_eq!(decode_string("a\\S\\''b"), "a\u{A7}b");
        assert_eq!(decode_string("a\\S\\\\\\b"), "a\u{DC}b");
        assert_eq!(decode_string("\\PE\\\\S\\0"), "\u{410}");
        // The page stays until the next \P...\.
        assert_eq!(decode_string("\\PB\\\\S\\1\\PA\\\\S\\q"), "\u{105}\u{F1}");
        // Unfinished or unknown escapes stay as they are; nothing panics.
        for s in [
            "\\S\\",
            "\\S\\\u{E4}",
            "\\PZ\\",
            "\\P",
            "\\X\\4",
            "\\X\\+1",
            "\\X2\\00E",
            "\\X4\\0000",
        ] {
            let _ = decode_string(s);
        }
        assert_eq!(decode_string("\\S\\"), "\\S\\");
        assert_eq!(decode_string("\\PZ\\"), "\\PZ\\");
        // A partial unit inside \X2\ is dropped, not read as a number.
        assert_eq!(decode_string("\\X2\\00E4F\\X0\\"), "\u{E4}");
        assert_eq!(decode_string("\\X4\\00000041\\X0\\"), "A");
    }

    #[test]
    fn encoded_text_decodes_to_the_same_text() {
        for s in [
            "",
            "plain",
            "o'k \\ x",
            "\u{D8}ya \u{E4}\u{F6}",
            "tree \u{1F333} end",
            "a\r\nb\tc",
            "\u{104}\u{416}\u{3A9}\u{20AC}",
        ] {
            assert_eq!(decode_string(&encode_string(s)), s, "{s:?}");
        }
    }

    #[test]
    fn a_nul_escape_is_replaced_not_put_into_the_text() {
        assert_eq!(decode_string("a\\X\\00b"), "a\u{FFFD}b");
        assert_eq!(decode_string("a\\X2\\0000\\X0\\b"), "a\u{FFFD}b");
        assert_eq!(decode_string("a\\X4\\00000000\\X0\\b"), "a\u{FFFD}b");
    }

    #[test]
    fn keywords_inside_comments_are_not_found() {
        let h = "/* FILE_SCHEMA(('WRONG')); */ FILE_SCHEMA(('IFC4'));";
        assert_eq!(
            find_keyword(h, "FILE_SCHEMA"),
            Some(h.rfind("FILE_SCHEMA").unwrap())
        );
        assert_eq!(header_schemas(h), vec!["IFC4".to_string()]);
        assert_eq!(find_keyword("/* FILE_SCHEMA", "FILE_SCHEMA"), None);
        assert_eq!(find_keyword("'/*' FILE_SCHEMA", "FILE_SCHEMA"), Some(5));
    }

    #[test]
    fn lower_case_keywords_are_read() {
        let f = parse("header;\nfile_schema(('IFC4'));\nendsec;\ndata;\n#1= ifcactorrole(.architect.,$,$);\n#2=(a() b(1));\n#3=ifcx(ifclabel('x'));\nendsec;").unwrap();
        assert_eq!(f.entities.len(), 3);
        assert_eq!(f.entities[0].name, "IFCACTORROLE");
        assert_eq!(f.entities[0].attrs[0], Value::Enum("ARCHITECT".into()));
        assert!(f.entities[1].is("A") && f.entities[1].is("B"));
        assert_eq!(
            f.entities[2].attrs[0],
            Value::Typed("IFCLABEL".into(), Box::new(Value::Str("x".into())))
        );
    }

    #[test]
    fn reals_always_have_a_point() {
        assert_eq!(format_real(0.0), "0.");
        assert_eq!(format_real(-0.0), "0.");
        assert_eq!(format_real(f64::NAN), "0.");
        assert_eq!(format_real(1.0), "1.");
        assert_eq!(format_real(-2.5), "-2.5");
        assert_eq!(format_real(0.001), "0.001");
        assert_eq!(format_real(1e-7), "1.E-7");
        assert_eq!(format_real(1.5e20), "1.5E20");
        assert_eq!(format_real(-3e-9), "-3.E-9");
        for x in [0.1, 123456.789, 1e-7, 1.5e20, -4.25e-12, 9.99e14] {
            assert_eq!(format_real(x).parse::<f64>().unwrap(), x);
        }
    }

    #[test]
    fn values_print_in_step_syntax() {
        let v = Value::List(vec![
            Value::Null,
            Value::Derived,
            Value::Int(-3),
            Value::Real(2.0),
            Value::s("a'b"),
            Value::e("MILLI"),
            Value::Bool(true),
            Value::Bool(false),
            Value::Ref(7),
            Value::typed("IFCLABEL", Value::s("x")),
        ]);
        assert_eq!(
            text(&v),
            "($,*,-3,2.,'a''b',.MILLI.,.T.,.F.,#7,IFCLABEL('x'))"
        );
        assert_eq!(text(&Value::refs([1, 2])), "(#1,#2)");
        assert_eq!(text(&Value::reals(&[0.5, 1.0])), "(0.5,1.)");
        assert_eq!(Value::Ref(4).as_ref_id(), Some(4));
        assert_eq!(Value::Null.as_ref_id(), None);
    }

    fn sample() -> Vec<u8> {
        let mut w = StepWriter::begin(
            Vec::new(),
            "a'b.ifc",
            "2000-01-01T00:00:00",
            "Ünal",
            "Org",
            "ReferenceView_V1.2",
        )
        .unwrap();
        assert_eq!(w.next_id(), 1);
        let p = w
            .entity("IFCCARTESIANPOINT", &[Value::reals(&[0.0, 1.5, -2.0])])
            .unwrap();
        let q = w
            .entity_with("IFCPOLYLINE", |out| {
                out.write_all(b"(")?;
                Value::Ref(p).write(out)?;
                out.write_all(b")")
            })
            .unwrap();
        w.entity(
            "IFCTHING",
            &[
                Value::Ref(q),
                Value::s("ä"),
                Value::List(vec![]),
                Value::typed("IFCLENGTHMEASURE", Value::Real(3.0)),
            ],
        )
        .unwrap();
        w.finish().unwrap()
    }

    #[test]
    fn writer_makes_a_valid_file() {
        let bytes = sample();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("ISO-10303-21;\nHEADER;\n"));
        assert!(text.contains("FILE_SCHEMA(('IFC4'));"));
        assert!(text.contains("FILE_NAME('a''b.ifc','2000-01-01T00:00:00',('\\X2\\00DC\\X0\\nal'),('Org'),'Kadakas','Kadakas','');"));
        assert!(text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
        assert!(text.is_ascii());
        assert!(text.contains("#1=IFCCARTESIANPOINT((0.,1.5,-2.));"));
    }

    #[test]
    fn parser_reads_what_the_writer_wrote() {
        let text = String::from_utf8(sample()).unwrap();
        let f = parse(&text).unwrap();
        assert_eq!(f.schemas(), vec!["IFC4".to_string()]);
        assert_eq!(f.entities.len(), 3);
        assert_eq!(f.get(1).unwrap().name, "IFCCARTESIANPOINT");
        assert_eq!(
            f.get(1).unwrap().attrs,
            vec![Value::reals(&[0.0, 1.5, -2.0])]
        );
        let thing = f.get(3).unwrap();
        assert_eq!(thing.attrs[1], Value::s("ä"));
        assert_eq!(thing.attrs[2], Value::List(vec![]));
        assert_eq!(
            thing.attrs[3],
            Value::typed("IFCLENGTHMEASURE", Value::Real(3.0))
        );
        assert!(f.dangling().is_empty());
        assert!(f.get(99).is_none());
    }

    #[test]
    fn parser_finds_dangling_references_and_rejects_bad_files() {
        let f = parse("HEADER;\nFILE_SCHEMA(('IFC4','X'));\nENDSEC;\nDATA;\n#1=A(#5,(#6),B(#7));\n#2=C();\nENDSEC;").unwrap();
        assert_eq!(f.schemas(), vec!["IFC4".to_string(), "X".to_string()]);
        assert_eq!(f.dangling(), vec![(1, 5), (1, 6), (1, 7)]);
        assert!(parse("nothing").is_err());
        assert!(parse("HEADER;").is_err());
        assert!(parse("HEADER;\nENDSEC;").is_err());
        for bad in [
            "#1=A(;",
            "#1=A('x);",
            "#1=A(1 2);",
            "#x=A();",
            "#1=(1);",
            "#1=A(#q);",
            "#1=A(%);",
            "#1=A(--1);",
            "#1=A((1;",
            "#1=A(B(1,2));",
            "#1 A();",
            "#1=A(.X);",
            "#1=A(1.2.3);",
        ] {
            let file = format!("HEADER;\nENDSEC;\nDATA;\n{bad}\nENDSEC;");
            assert!(parse(&file).is_err(), "{bad}");
        }
        assert_eq!(
            StepFile {
                header: String::new(),
                entities: vec![],
                duplicates: vec![],
            }
            .schemas(),
            Vec::<String>::new()
        );
        assert_eq!(
            StepFile {
                header: "FILE_SCHEMA".into(),
                entities: vec![],
                duplicates: vec![],
            }
            .schemas(),
            Vec::<String>::new()
        );
        let f = parse("HEADER;\nENDSEC;\nDATA;\n#1=A(1,1.5E3,-2,.T.,.F.,.ENUM.,$,*);\nENDSEC;")
            .unwrap();
        assert_eq!(
            f.entities[0].attrs,
            vec![
                Value::Int(1),
                Value::Real(1500.0),
                Value::Int(-2),
                Value::Bool(true),
                Value::Bool(false),
                Value::e("ENUM"),
                Value::Null,
                Value::Derived
            ]
        );
    }

    #[test]
    fn comments_complex_instances_and_depth() {
        let f = parse(
            "HEADER;\nENDSEC;\nDATA;\n/* a comment */\n#1=(A(1.) /* x */ B('s',#2) C());\n#2=D((1.,2.));\nENDSEC;",
        )
        .unwrap();
        assert_eq!(f.entities.len(), 2);
        let e = f.get(1).unwrap();
        assert_eq!(e.name, COMPLEX);
        assert!(e.is("B") && e.is("C") && !e.is("D"));
        assert_eq!(e.part("A").unwrap(), &[Value::Real(1.0)]);
        assert_eq!(e.part("B").unwrap()[1], Value::Ref(2));
        assert!(f.get(2).unwrap().is("D"));
        assert!(f.get(2).unwrap().part("A").is_none());
        // An unclosed comment ends the data.
        assert!(parse("HEADER;\nENDSEC;\nDATA;\n/* open").is_err());
        // Deep nesting is refused, it does not overflow the stack.
        let deep = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
        let file = format!("HEADER;\nENDSEC;\nDATA;\n#1=A({deep});\nENDSEC;");
        assert!(parse(&file).unwrap_err().contains("nested"));
        for bad in ["#1=();", "#1=(A(;", "#1=(A() 5);", "#1=A(1 2);"] {
            let file = format!("HEADER;\nENDSEC;\nDATA;\n{bad}\nENDSEC;");
            assert!(parse(&file).is_err(), "{bad}");
        }
    }

    #[test]
    fn named_data_sections_duplicates_and_limits() {
        let f = parse(
            "HEADER;\nENDSEC;\nDATA('part',('AP242'));\n#1=A(1.);\n#1=B(2.);\n#2=C();\nENDSEC;",
        )
        .unwrap();
        assert_eq!(f.entities.len(), 2);
        assert_eq!(f.get(1).unwrap().name, "A");
        assert_eq!(f.duplicates, vec![1]);
        assert!(parse("HEADER;\nENDSEC;\nDATA ;\n#1=A();\nENDSEC;").is_ok());
        assert!(parse("HEADER;\nENDSEC;\nMYDATA;\n#1=A();\nENDSEC;").is_err());
        let many = "HEADER;\nENDSEC;\nDATA;\n#1=A();\n#2=A();\n#3=A();\nENDSEC;";
        assert!(
            parse_limited(many, 2, u64::MAX).unwrap_err()
                == ParseError::TooManyEntities { limit: 2 }
        );
        assert!(matches!(
            parse_limited(many, 9, 100).unwrap_err(),
            ParseError::TooMuchMemory { limit: 100, .. }
        ));
        assert!(parse_limited(many, 9, 100_000).is_ok());
    }
}
