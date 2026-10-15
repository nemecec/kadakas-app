//! A small XML writer for the BCF files. It escapes text and keeps count of the characters that
//! XML 1.0 cannot hold.

/// True for a character that XML 1.0 allows in text.
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r')
        || ('\u{20}'..='\u{D7FF}').contains(&c)
        || ('\u{E000}'..='\u{FFFD}').contains(&c)
        || ('\u{10000}'..='\u{10FFFF}').contains(&c)
}

#[derive(Default)]
pub struct Xml {
    out: String,
    stack: Vec<&'static str>,
    /// How many characters were replaced because XML cannot hold them.
    pub replaced: usize,
}

impl Xml {
    pub fn new() -> Xml {
        Xml {
            out: "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n".into(),
            ..Xml::default()
        }
    }

    fn escape(&mut self, text: &str, attribute: bool) -> String {
        let mut s = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '&' => s.push_str("&amp;"),
                '<' => s.push_str("&lt;"),
                '>' => s.push_str("&gt;"),
                '"' if attribute => s.push_str("&quot;"),
                '\n' if attribute => s.push_str("&#10;"),
                '\r' => s.push_str("&#13;"),
                '\t' if attribute => s.push_str("&#9;"),
                c if xml_char(c) => s.push(c),
                _ => {
                    self.replaced += 1;
                    s.push('\u{FFFD}');
                }
            }
        }
        s
    }

    fn indent(&mut self) {
        for _ in 0..self.stack.len() {
            self.out.push_str("  ");
        }
    }

    fn open_tag(&mut self, name: &str, attrs: &[(&str, &str)], close: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(name);
        for (k, v) in attrs {
            let v = self.escape(v, true);
            self.out.push_str(&format!(" {k}=\"{v}\""));
        }
        self.out.push_str(close);
        self.out.push('\n');
    }

    /// `<name attrs>`; call [`Xml::end`] to close it.
    pub fn start(&mut self, name: &'static str, attrs: &[(&str, &str)]) {
        self.open_tag(name, attrs, ">");
        self.stack.push(name);
    }

    pub fn end(&mut self) {
        let name = self.stack.pop().expect("an open element");
        self.indent();
        self.out.push_str(&format!("</{name}>\n"));
    }

    /// `<name attrs/>`
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.open_tag(name, attrs, "/>");
    }

    /// `<name>text</name>`
    pub fn text(&mut self, name: &str, text: &str) {
        self.text_attrs(name, &[], text);
    }

    pub fn text_attrs(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(name);
        for (k, v) in attrs {
            let v = self.escape(v, true);
            self.out.push_str(&format!(" {k}=\"{v}\""));
        }
        let t = self.escape(text, false);
        self.out.push_str(&format!(">{t}</{name}>\n"));
    }

    /// A number in the shortest form that reads back as the same number.
    pub fn number(&mut self, name: &str, v: f64) {
        self.text(name, &fmt(v));
    }

    pub fn point(&mut self, name: &'static str, p: [f64; 3]) {
        self.start(name, &[]);
        for (axis, v) in ["X", "Y", "Z"].into_iter().zip(p) {
            self.number(axis, v);
        }
        self.end();
    }

    pub fn finish(self) -> String {
        debug_assert!(self.stack.is_empty(), "an element is open");
        self.out
    }
}

/// A finite number as text. `-0` is written as `0`.
pub fn fmt(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v}")
}
