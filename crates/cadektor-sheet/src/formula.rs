//! Formulas: lexer, parser and printer.
//!
//! The same language is used in cells (`=SUM(A1:A9) * 2`), in database criteria
//! (`Kind = "plant" AND Soil.Depth > 300`) and in database columns (`Area * 1.2`).
//!
//! Syntax:
//! - Numbers `12`, `3.5`, `1e3`. Text in double quotes (`"a ""b"""`) or single quotes (`'a'`).
//! - Cell references `A1`, ranges `B2:C5`. Function calls `SUM(A1:A3, 2)`.
//! - Names such as `Area` and property paths: `Plant.price`, `Soil.Depth`, `'My record'.'My field'`.
//! - Operators, from low to high precedence: `OR`, `AND`, `NOT`, comparison (`= <> < <= > >=`),
//!   `&` (join text), `+ -`, `* /`, `^`, and a leading `-` or `+`.
//!
//! [`parse`] never panics. [`Expr`] prints back with `Display`, and the printed text parses to
//! the same expression.

use std::fmt;

/// Longest formula text that [`parse`] reads, in characters.
pub const MAX_LEN: usize = 4096;
/// Deepest nesting of brackets, operators and calls.
pub const MAX_DEPTH: usize = 64;
/// Largest column and row number of a cell.
pub const MAX_COL: u32 = 1000;
pub const MAX_ROW: u32 = 100_000;

/// A cell address. Column and row start at 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellRef {
    pub row: u32,
    pub col: u32,
}

impl CellRef {
    /// Reads `A1`, `b12` or `AA7`. Returns `None` for anything else or for an address outside the sheet limits.
    pub fn parse(s: &str) -> Option<CellRef> {
        let s = s.trim();
        let letters = s.bytes().take_while(u8::is_ascii_alphabetic).count();
        let (l, d) = s.split_at(letters);
        if l.is_empty() || l.len() > 3 || d.is_empty() || d.len() > 6 || !d.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let col = l.bytes().fold(0u32, |acc, b| acc * 26 + u32::from(b.to_ascii_uppercase() - b'A') + 1);
        let row: u32 = d.parse().ok()?;
        ((1..=MAX_COL).contains(&col) && (1..=MAX_ROW).contains(&row)).then_some(CellRef { row, col })
    }

    /// The column letters for a column number, for example 28 gives `AB`.
    pub fn col_name(col: u32) -> String {
        let mut n = col;
        let mut out = Vec::new();
        while n > 0 {
            n -= 1;
            out.push(b'A' + (n % 26) as u8);
            n /= 26;
        }
        out.reverse();
        String::from_utf8(out).unwrap_or_default()
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", CellRef::col_name(self.col), self.row)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl BinOp {
    fn prec(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 3,
            BinOp::Concat => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div => 6,
            BinOp::Pow => 7,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            BinOp::Or => "OR",
            BinOp::And => "AND",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Concat => "&",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
        }
    }
}

/// A parsed formula.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Cell(CellRef),
    /// A block of cells. The corners are in order: first is top left.
    Range(CellRef, CellRef),
    /// A property name such as `Area`.
    Name(String),
    /// A property path such as `Soil.Depth`.
    Path(Vec<String>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// A function call. The name is in upper case.
    Call(String, Vec<Expr>),
}

/// Why a formula cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Character position of the problem.
    pub at: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.at + 1)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    /// A name in single quotes.
    Quoted(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Colon,
    Dot,
}

fn ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn lex(src: &str) -> Result<Vec<(Tok, usize)>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    if chars.len() > MAX_LEN {
        return Err(ParseError { message: format!("the formula is longer than {MAX_LEN} characters"), at: MAX_LEN });
    }
    let mut out: Vec<(Tok, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let err = |m: &str, at: usize| ParseError { message: m.to_string(), at };
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let tok = if c == '"' || c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(err("a quote is not closed", start)),
                    Some(&q) if q == c => {
                        if chars.get(i + 1) == Some(&c) {
                            s.push(c);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            if c == '"' { Tok::Str(s) } else { Tok::Quoted(s) }
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit) && !matches!(out.last(), Some((Tok::Ident(_) | Tok::Quoted(_), _)))) {
            let mut j = i;
            while chars.get(j).is_some_and(|d| d.is_ascii_digit() || *d == '.') {
                j += 1;
            }
            if matches!(chars.get(j), Some('e' | 'E')) {
                let mut k = j + 1;
                if matches!(chars.get(k), Some('+' | '-')) {
                    k += 1;
                }
                if chars.get(k).is_some_and(char::is_ascii_digit) {
                    while chars.get(k).is_some_and(char::is_ascii_digit) {
                        k += 1;
                    }
                    j = k;
                }
            }
            let text: String = chars[i..j].iter().collect();
            i = j;
            match text.parse::<f64>() {
                Ok(n) if n.is_finite() => Tok::Num(n),
                _ => return Err(err("this number cannot be read", start)),
            }
        } else if ident_start(c) {
            let mut j = i;
            while chars.get(j).is_some_and(|d| ident_part(*d)) {
                j += 1;
            }
            let s: String = chars[i..j].iter().collect();
            i = j;
            Tok::Ident(s)
        } else {
            i += 1;
            match c {
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' | ';' => Tok::Comma,
                ':' => Tok::Colon,
                '.' => Tok::Dot,
                '+' => Tok::Op("+"),
                '-' => Tok::Op("-"),
                '*' => Tok::Op("*"),
                '/' => Tok::Op("/"),
                '^' => Tok::Op("^"),
                '&' => Tok::Op("&"),
                '=' => Tok::Op("="),
                '<' => match chars.get(i) {
                    Some('=') => {
                        i += 1;
                        Tok::Op("<=")
                    }
                    Some('>') => {
                        i += 1;
                        Tok::Op("<>")
                    }
                    _ => Tok::Op("<"),
                },
                '>' => {
                    if chars.get(i) == Some(&'=') {
                        i += 1;
                        Tok::Op(">=")
                    } else {
                        Tok::Op(">")
                    }
                }
                '!' if chars.get(i) == Some(&'=') => {
                    i += 1;
                    Tok::Op("<>")
                }
                other => return Err(err(&format!("unexpected character '{other}'"), start)),
            }
        };
        out.push((tok, start));
    }
    Ok(out)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    depth: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }

    fn at(&self) -> usize {
        self.toks.get(self.pos).map_or(self.end, |(_, a)| *a)
    }

    fn err<T>(&self, m: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError { message: m.into(), at: self.at() })
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { self.err("the formula is nested too deeply") } else { Ok(()) }
    }

    /// True if the next token is the word `kw`. Before an operand `NOT (` is a call, so the
    /// check can skip that case. After an operand `AND (` and `OR (` are operators.
    fn is_keyword(&self, kw: &str, call_ok: bool) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(kw)) && !(call_ok && matches!(self.toks.get(self.pos + 1), Some((Tok::LParen, _))))
    }

    fn or(&mut self) -> Result<Expr, ParseError> {
        self.enter()?;
        let mut left = self.and()?;
        while self.is_keyword("or", false) {
            self.pos += 1;
            let right = self.and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.not()?;
        while self.is_keyword("and", false) {
            self.pos += 1;
            let right = self.not()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, ParseError> {
        if self.is_keyword("not", true) {
            self.pos += 1;
            self.enter()?;
            let e = self.not()?;
            self.depth -= 1;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e)));
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.concat()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("=")) => BinOp::Eq,
                Some(Tok::Op("<>")) => BinOp::Ne,
                Some(Tok::Op("<")) => BinOp::Lt,
                Some(Tok::Op("<=")) => BinOp::Le,
                Some(Tok::Op(">")) => BinOp::Gt,
                Some(Tok::Op(">=")) => BinOp::Ge,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.concat()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn concat(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.add()?;
        while self.peek() == Some(&Tok::Op("&")) {
            self.pos += 1;
            let right = self.add()?;
            left = Expr::Binary(BinOp::Concat, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn add(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("+")) => BinOp::Add,
                Some(Tok::Op("-")) => BinOp::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.mul()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn mul(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.pow()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("*")) => BinOp::Mul,
                Some(Tok::Op("/")) => BinOp::Div,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.pow()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn pow(&mut self) -> Result<Expr, ParseError> {
        let left = self.unary()?;
        if self.peek() == Some(&Tok::Op("^")) {
            self.pos += 1;
            self.enter()?;
            let right = self.pow()?;
            self.depth -= 1;
            return Ok(Expr::Binary(BinOp::Pow, Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Op("-")) => {
                self.pos += 1;
                self.enter()?;
                let e = self.unary()?;
                self.depth -= 1;
                Ok(Expr::Unary(UnOp::Neg, Box::new(e)))
            }
            Some(Tok::Op("+")) => {
                self.pos += 1;
                self.enter()?;
                let e = self.unary()?;
                self.depth -= 1;
                Ok(e)
            }
            _ => self.primary(),
        }
    }

    fn segment(&mut self) -> Result<String, ParseError> {
        match self.peek().cloned() {
            Some(Tok::Ident(s) | Tok::Quoted(s)) => {
                self.pos += 1;
                Ok(s)
            }
            _ => self.err("a name is expected after '.'"),
        }
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let Some(tok) = self.peek().cloned() else { return self.err("the formula ends too early") };
        match tok {
            Tok::Num(n) => {
                self.pos += 1;
                Ok(Expr::Num(n))
            }
            Tok::Str(s) => {
                self.pos += 1;
                Ok(Expr::Str(s))
            }
            Tok::LParen => {
                self.pos += 1;
                let e = self.or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return self.err("a ')' is missing");
                }
                self.pos += 1;
                Ok(e)
            }
            Tok::Quoted(s) => {
                self.pos += 1;
                if self.peek() == Some(&Tok::Dot) {
                    return self.path(s);
                }
                Ok(Expr::Str(s))
            }
            Tok::Ident(name) => {
                self.pos += 1;
                match self.peek() {
                    Some(Tok::LParen) => self.call(name),
                    Some(Tok::Dot) => self.path(name),
                    _ => {
                        if let Some(a) = CellRef::parse(&name) {
                            if self.peek() == Some(&Tok::Colon) {
                                if let Some((Tok::Ident(second), _)) = self.toks.get(self.pos + 1)
                                    && let Some(b) = CellRef::parse(second)
                                {
                                    self.pos += 2;
                                    return Ok(Expr::Range(CellRef { row: a.row.min(b.row), col: a.col.min(b.col) }, CellRef { row: a.row.max(b.row), col: a.col.max(b.col) }));
                                }
                                return self.err("a range needs a cell after ':'");
                            }
                            return Ok(Expr::Cell(a));
                        }
                        if ["and", "or", "not"].contains(&name.to_ascii_lowercase().as_str()) {
                            self.pos -= 1;
                            return self.err(format!("'{name}' is a reserved word here"));
                        }
                        if name.eq_ignore_ascii_case("true") {
                            return Ok(Expr::Bool(true));
                        }
                        if name.eq_ignore_ascii_case("false") {
                            return Ok(Expr::Bool(false));
                        }
                        Ok(Expr::Name(name))
                    }
                }
            }
            Tok::RParen => self.err("unexpected ')'"),
            Tok::Comma => self.err("unexpected ','"),
            Tok::Colon => self.err("unexpected ':'"),
            Tok::Dot => self.err("unexpected '.'"),
            Tok::Op(o) => self.err(format!("unexpected '{o}'")),
        }
    }

    fn path(&mut self, first: String) -> Result<Expr, ParseError> {
        let mut segs = vec![first];
        while self.peek() == Some(&Tok::Dot) {
            self.pos += 1;
            segs.push(self.segment()?);
        }
        Ok(Expr::Path(segs))
    }

    fn call(&mut self, name: String) -> Result<Expr, ParseError> {
        self.pos += 1; // the '('
        self.enter()?;
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::RParen) {
            self.pos += 1;
        } else {
            loop {
                args.push(self.or()?);
                match self.peek() {
                    Some(Tok::Comma) => self.pos += 1,
                    Some(Tok::RParen) => {
                        self.pos += 1;
                        break;
                    }
                    _ => return self.err("a ',' or ')' is expected"),
                }
            }
        }
        self.depth -= 1;
        let name = name.to_uppercase();
        if name == "NOT" && args.len() == 1 {
            return Ok(Expr::Unary(UnOp::Not, Box::new(args.remove(0))));
        }
        Ok(Expr::Call(name, args))
    }
}

/// Reads a formula. A leading `=` is allowed and ignored.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let src = src.trim_start();
    let src = src.strip_prefix('=').unwrap_or(src);
    let toks = lex(src)?;
    let end = src.chars().count();
    if toks.is_empty() {
        return Err(ParseError { message: "the formula is empty".into(), at: 0 });
    }
    let mut p = Parser { toks, pos: 0, depth: 0, end };
    let e = p.or()?;
    if p.pos < p.toks.len() {
        return p.err("unexpected text after the end of the formula");
    }
    Ok(e)
}

impl Expr {
    fn prec(&self) -> u8 {
        match self {
            Expr::Binary(op, ..) => op.prec(),
            Expr::Unary(UnOp::Neg, _) => 8,
            _ => 9,
        }
    }

    /// Every cell address that the formula reads, with ranges as corner pairs.
    pub fn references(&self, cells: &mut Vec<CellRef>, ranges: &mut Vec<(CellRef, CellRef)>) {
        match self {
            Expr::Cell(c) => cells.push(*c),
            Expr::Range(a, b) => ranges.push((*a, *b)),
            Expr::Unary(_, e) => e.references(cells, ranges),
            Expr::Binary(_, l, r) => {
                l.references(cells, ranges);
                r.references(cells, ranges);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.references(cells, ranges)),
            Expr::Num(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Name(_) | Expr::Path(_) => {}
        }
    }
}

fn plain_ident(s: &str) -> bool {
    let mut it = s.chars();
    it.next().is_some_and(ident_start) && it.all(ident_part)
}

fn write_quoted(f: &mut fmt::Formatter<'_>, s: &str, q: char) -> fmt::Result {
    write!(f, "{q}")?;
    for c in s.chars() {
        if c == q {
            write!(f, "{q}")?;
        }
        write!(f, "{c}")?;
    }
    write!(f, "{q}")
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Num(n) => write!(f, "{n}"),
            Expr::Str(s) => write_quoted(f, s, '"'),
            Expr::Bool(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            Expr::Cell(c) => write!(f, "{c}"),
            Expr::Range(a, b) => write!(f, "{a}:{b}"),
            Expr::Name(n) => {
                if plain_ident(n) && CellRef::parse(n).is_none() && !["true", "false", "and", "or", "not"].contains(&n.to_ascii_lowercase().as_str()) {
                    f.write_str(n)
                } else {
                    // A name that is not a plain word cannot be written as a name. This only
                    // happens for expressions that were built by hand.
                    write_quoted(f, n, '\'')
                }
            }
            Expr::Path(p) => {
                for (i, seg) in p.iter().enumerate() {
                    if i > 0 {
                        f.write_str(".")?;
                    }
                    write_quoted(f, seg, '\'')?;
                }
                Ok(())
            }
            Expr::Unary(UnOp::Not, e) => write!(f, "NOT({e})"),
            Expr::Unary(UnOp::Neg, e) => {
                if e.prec() >= 9 || matches!(**e, Expr::Unary(UnOp::Neg, _)) {
                    write!(f, "-{e}")
                } else {
                    write!(f, "-({e})")
                }
            }
            Expr::Binary(op, l, r) => {
                let p = op.prec();
                let right_assoc = *op == BinOp::Pow;
                let (lp, rp) = if right_assoc { (p + 1, p) } else { (p, p + 1) };
                if l.prec() < lp { write!(f, "({l})")? } else { write!(f, "{l}")? }
                write!(f, " {} ", op.symbol())?;
                if r.prec() < rp { write!(f, "({r})") } else { write!(f, "{r}") }
            }
            Expr::Call(name, args) => {
                write!(f, "{name}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn p(s: &str) -> Expr {
        parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn show(s: &str) -> String {
        p(s).to_string()
    }

    #[test]
    fn cell_refs() {
        assert_eq!(CellRef::parse("A1"), Some(CellRef { row: 1, col: 1 }));
        assert_eq!(CellRef::parse(" ab12 "), Some(CellRef { row: 12, col: 28 }));
        assert_eq!(CellRef::parse("A0"), None);
        assert_eq!(CellRef::parse("1A"), None);
        assert_eq!(CellRef::parse("A"), None);
        assert_eq!(CellRef::parse("AAAA1"), None);
        assert_eq!(CellRef::parse("A1234567"), None);
        assert_eq!(CellRef::parse("A100001"), None);
        assert_eq!(CellRef::parse("ZZZ1"), None);
        assert_eq!(CellRef::parse("A1b"), None);
        assert_eq!(CellRef::col_name(1), "A");
        assert_eq!(CellRef::col_name(26), "Z");
        assert_eq!(CellRef::col_name(27), "AA");
        assert_eq!(CellRef::col_name(702), "ZZ");
        assert_eq!(CellRef::col_name(0), "");
        assert_eq!(CellRef { row: 7, col: 28 }.to_string(), "AB7");
        for col in 1..=MAX_COL {
            assert_eq!(CellRef::parse(&format!("{}1", CellRef::col_name(col))).map(|c| c.col), Some(col));
        }
    }

    #[test]
    fn precedence_and_printing() {
        assert_eq!(show("=1+2*3"), "1 + 2 * 3");
        assert_eq!(show("(1+2)*3"), "(1 + 2) * 3");
        assert_eq!(show("1-(2-3)"), "1 - (2 - 3)");
        assert_eq!(show("1-2-3"), "1 - 2 - 3");
        assert_eq!(show("2^3^2"), "2 ^ 3 ^ 2");
        assert_eq!(show("(2^3)^2"), "(2 ^ 3) ^ 2");
        assert_eq!(show("-2^2"), "-2 ^ 2");
        assert_eq!(show("-(2^2)"), "-(2 ^ 2)");
        assert_eq!(show("2^-1"), "2 ^ -1");
        assert_eq!(show("--1"), "--1");
        assert_eq!(show("+5"), "5");
        assert_eq!(show("a1 & \"x\" & 2"), "A1 & \"x\" & 2");
        assert_eq!(show("1<2 and 3>=2 or not 1=1"), "1 < 2 AND 3 >= 2 OR NOT(1 = 1)");
        assert_eq!(show("1!=2"), "1 <> 2");
        assert_eq!(show("1<>2"), "1 <> 2");
        assert_eq!(show("not(1)"), "NOT(1)");
        assert_eq!(show("1 or (2 and (3))"), "1 OR 2 AND 3");
        assert_eq!(show("(1 or 2) and (3)"), "(1 OR 2) AND 3");
        assert_eq!(show("1 and(2)"), "1 AND 2");
        assert_eq!(show("sum(a1:b2, 3;4)"), "SUM(A1:B2, 3, 4)");
        assert_eq!(show("sum( )"), "SUM()");
        assert_eq!(show("b2:a1"), "A1:B2");
        assert_eq!(show("TRUE and false"), "TRUE AND FALSE");
        assert_eq!(show("(1)"), "1");
        assert_eq!(show("1.5e3 + .5 + 2E-2"), "1500 + 0.5 + 0.02");
    }

    #[test]
    fn names_paths_and_strings() {
        assert_eq!(p("Area"), Expr::Name("Area".into()));
        assert_eq!(p("Soil.Depth"), Expr::Path(vec!["Soil".into(), "Depth".into()]));
        assert_eq!(p("'My record'.'My field'"), Expr::Path(vec!["My record".into(), "My field".into()]));
        assert_eq!(p("'Plant'"), Expr::Str("Plant".into()));
        assert_eq!(p("A1.x"), Expr::Path(vec!["A1".into(), "x".into()]));
        assert_eq!(p("Plant.custom.water"), Expr::Path(vec!["Plant".into(), "custom".into(), "water".into()]));
        assert_eq!(p("\"a \"\"b\"\"\""), Expr::Str("a \"b\"".into()));
        assert_eq!(p("'it''s'"), Expr::Str("it's".into()));
        assert_eq!(show("'it''s'"), "\"it's\"");
        assert_eq!(show("\"a \"\"b\"\"\""), "\"a \"\"b\"\"\"");
        assert_eq!(show("Soil.Depth > 300"), "'Soil'.'Depth' > 300");
        assert_eq!(show("Ärg.Öö"), "'Ärg'.'Öö'");
        assert_eq!(p("Äpple"), Expr::Name("Äpple".into()));
        // A name that would read as something else prints quoted, which parses as text. Only hand-made expressions do this.
        assert_eq!(Expr::Name("A1".into()).to_string(), "'A1'");
        assert_eq!(Expr::Name("a b".into()).to_string(), "'a b'");
        assert_eq!(Expr::Name("and".into()).to_string(), "'and'");
    }

    #[test]
    fn errors_have_positions() {
        let e = parse("1 +").unwrap_err();
        assert!(e.message.contains("ends too early") && e.at == 3, "{e}");
        assert!(parse("").unwrap_err().message.contains("empty"));
        assert!(parse("=").unwrap_err().message.contains("empty"));
        assert!(parse("(1").unwrap_err().message.contains("')'"));
        assert!(parse("1)").unwrap_err().message.contains("unexpected text"));
        assert!(parse("1 2").is_err());
        assert!(parse("sum(1 2)").unwrap_err().message.contains("','"));
        assert!(parse("sum(1,").is_err());
        assert!(parse("\"abc").unwrap_err().message.contains("quote"));
        assert!(parse("'abc").is_err());
        assert!(parse("1 # 2").unwrap_err().message.contains("'#'"));
        assert!(parse("!").is_err());
        assert!(parse("1e999").unwrap_err().message.contains("number"));
        assert!(parse("1.2.3").is_err());
        assert!(parse("a1:").unwrap_err().message.contains("range"));
        assert!(parse("a1:5").is_err());
        assert!(parse("x.").unwrap_err().message.contains("name is expected"));
        assert!(parse("x.1").is_err());
        assert!(parse(")").unwrap_err().message.contains("')'"));
        assert!(parse(",").is_err());
        assert!(parse(":").is_err());
        assert!(parse(".").is_err());
        assert!(parse("* 2").is_err());
        assert!(parse("1 + and").unwrap_err().message.contains("reserved"));
        assert!(parse("or").is_err());
        assert!(parse(&"1".repeat(MAX_LEN + 1)).unwrap_err().message.contains("longer"));
        assert!(parse(&format!("{}1{}", "(".repeat(100), ")".repeat(100))).unwrap_err().message.contains("nested"));
        assert!(parse(&format!("{}1", "-".repeat(100))).unwrap_err().message.contains("nested"));
        assert!(parse(&format!("{}1", "+".repeat(100))).unwrap_err().message.contains("nested"));
        assert!(parse(&format!("{}1", "not ".repeat(100))).unwrap_err().message.contains("nested"));
        assert!(parse(&format!("{}1", "1^".repeat(100))).unwrap_err().message.contains("nested"));
        assert!(parse(&format!("{}1{}", "f(".repeat(100), ")".repeat(100))).unwrap_err().message.contains("nested"));
        assert!(parse(&format!("{}1{}", "(".repeat(60), ")".repeat(60))).is_ok());
        assert_eq!(parse("1 +").unwrap_err().to_string(), "the formula ends too early (at character 4)");
    }

    #[test]
    fn references_are_collected() {
        let e = p("A1 + SUM(B2:C3, D4) * IF(E5 > 0, 1, F6)");
        let (mut cells, mut ranges) = (vec![], vec![]);
        e.references(&mut cells, &mut ranges);
        assert_eq!(cells.len(), 4);
        assert_eq!(ranges, vec![(CellRef { row: 2, col: 2 }, CellRef { row: 3, col: 3 })]);
        let e = p("-NOT(A1) & \"x\" & Area & Soil.Depth & 1 & TRUE");
        let (mut cells, mut ranges) = (vec![], vec![]);
        e.references(&mut cells, &mut ranges);
        assert_eq!((cells.len(), ranges.len()), (1, 0));
    }

    // --- Property tests ---

    fn arb_expr() -> impl Strategy<Value = Expr> {
        let leaf = prop_oneof![
            (0u32..1000).prop_map(|n| Expr::Num(f64::from(n) / 8.0)),
            "[a-z ]{0,5}".prop_map(Expr::Str),
            any::<bool>().prop_map(Expr::Bool),
            (1u32..50, 1u32..50).prop_map(|(row, col)| Expr::Cell(CellRef { row, col })),
            (1u32..20, 1u32..20, 0u32..5, 0u32..5).prop_map(|(row, col, dr, dc)| Expr::Range(CellRef { row, col }, CellRef { row: row + dr, col: col + dc })),
            prop::sample::select(vec!["Area", "Layer", "Kind", "Count", "Id"]).prop_map(|s| Expr::Name(s.to_string())),
            prop::collection::vec("[A-Za-z ]{1,6}", 2..4).prop_map(Expr::Path),
        ];
        leaf.prop_recursive(4, 40, 4, |inner| {
            let ops = prop::sample::select(vec![
                BinOp::Or,
                BinOp::And,
                BinOp::Eq,
                BinOp::Ne,
                BinOp::Lt,
                BinOp::Le,
                BinOp::Gt,
                BinOp::Ge,
                BinOp::Concat,
                BinOp::Add,
                BinOp::Sub,
                BinOp::Mul,
                BinOp::Div,
                BinOp::Pow,
            ]);
            prop_oneof![
                (ops, inner.clone(), inner.clone()).prop_map(|(op, l, r)| Expr::Binary(op, Box::new(l), Box::new(r))),
                inner.clone().prop_map(|e| Expr::Unary(UnOp::Neg, Box::new(e))),
                inner.clone().prop_map(|e| Expr::Unary(UnOp::Not, Box::new(e))),
                (prop::sample::select(vec!["SUM", "IF", "ROUND", "MAX", "CONCAT", "F"]), prop::collection::vec(inner, 0..4)).prop_map(|(n, a)| Expr::Call(n.to_string(), a)),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Printing an expression and reading it again gives the same expression.
        #[test]
        fn printed_formulas_parse_to_the_same_expression(e in arb_expr()) {
            let text = e.to_string();
            let back = parse(&text).unwrap_or_else(|err| panic!("{text}: {err}"));
            // A one-argument NOT call is the same as the operator, and Neg of a number is a number sign.
            prop_assert_eq!(back.to_string(), text.clone());
            let again = parse(&back.to_string()).unwrap();
            prop_assert_eq!(again, back);
        }

        /// The parser gives an answer for any text and never panics.
        #[test]
        fn random_text_never_panics(s in "\\PC{0,80}") {
            let _ = parse(&s);
        }

        /// Random strings made of formula characters never panic, and what parses prints and parses again.
        #[test]
        fn random_formula_like_text(s in "[A-Za-z0-9 ()+*/^&=<>:,.'\"!;-]{0,60}") {
            if let Ok(e) = parse(&s) {
                let text = e.to_string();
                let back = parse(&text).unwrap_or_else(|err| panic!("{s} printed as {text}: {err}"));
                prop_assert_eq!(back.to_string(), text);
            }
        }
    }
}
