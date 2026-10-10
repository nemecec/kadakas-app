//! PDF output with `pdf-writer`. Vector paths with real line weights, fills, arcs as Bezier
//! curves, text in the standard font Helvetica, and 3D viewports as images. One page per sheet.

use std::collections::BTreeMap;

use glam::DVec2;
use pdf_writer::types::LineCapStyle;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use crate::display::*;

/// PDF points per mm.
pub const PT: f64 = 72.0 / 25.4;

/// Characters of Windows-1252 (WinAnsi) at the codes 0x80 to 0x9F. The standard font has all of them.
const CP1252_HIGH: [(char, u8); 27] = [
    ('\u{20AC}', 0x80),
    ('\u{201A}', 0x82),
    ('\u{0192}', 0x83),
    ('\u{201E}', 0x84),
    ('\u{2026}', 0x85),
    ('\u{2020}', 0x86),
    ('\u{2021}', 0x87),
    ('\u{02C6}', 0x88),
    ('\u{2030}', 0x89),
    ('\u{0160}', 0x8A),
    ('\u{2039}', 0x8B),
    ('\u{0152}', 0x8C),
    ('\u{017D}', 0x8E),
    ('\u{2018}', 0x91),
    ('\u{2019}', 0x92),
    ('\u{201C}', 0x93),
    ('\u{201D}', 0x94),
    ('\u{2022}', 0x95),
    ('\u{2013}', 0x96),
    ('\u{2014}', 0x97),
    ('\u{02DC}', 0x98),
    ('\u{2122}', 0x99),
    ('\u{0161}', 0x9A),
    ('\u{203A}', 0x9B),
    ('\u{0153}', 0x9C),
    ('\u{017E}', 0x9E),
    ('\u{0178}', 0x9F),
];

/// Text for the standard font: Windows-1252 (WinAnsi) bytes. Characters that the encoding does not have become `?`.
pub fn winansi(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c {
            c if (c as u32) < 0x80 || ((0xA0..0x100).contains(&(c as u32))) => c as u32 as u8,
            c => CP1252_HIGH
                .iter()
                .find(|(x, _)| *x == c)
                .map_or(b'?', |(_, b)| *b),
        })
        .collect()
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

struct Ids {
    next: i32,
}

impl Ids {
    fn alloc(&mut self) -> Ref {
        self.next += 1;
        Ref::new(self.next)
    }
}

/// What one page needs besides its content stream.
#[derive(Default)]
struct Used {
    images: Vec<(String, Ref)>,
    alphas: BTreeMap<u8, Ref>,
}

fn rgb(c: Rgba) -> [f32; 3] {
    [
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
    ]
}

fn pt(p: DVec2) -> (f32, f32) {
    ((p.x * PT) as f32, (p.y * PT) as f32)
}

fn trace(c: &mut Content, segs: &[PSeg], closed: bool) {
    for s in segs {
        match *s {
            PSeg::Move(p) => {
                let (x, y) = pt(p);
                c.move_to(x, y);
            }
            PSeg::Line(p) => {
                let (x, y) = pt(p);
                c.line_to(x, y);
            }
            PSeg::Cubic(a, b, p) => {
                let (a, b, p) = (pt(a), pt(b), pt(p));
                c.cubic_to(a.0, a.1, b.0, b.1, p.0, p.1);
            }
        }
    }
    if closed {
        c.close_path();
    }
}

/// Writes a document of pages. `title` becomes the document title.
pub fn write_pdf(pages: &[&Page], title: &str) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let mut ids = Ids { next: 0 };
    let catalog = ids.alloc();
    let tree = ids.alloc();
    let font = ids.alloc();
    let info = ids.alloc();
    pdf.catalog(catalog).pages(tree);
    pdf.type1_font(font)
        .base_font(Name(b"Helvetica"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    pdf.document_info(info)
        .title(TextStr(title))
        .producer(TextStr("Cadektor"));
    let page_ids: Vec<Ref> = pages.iter().map(|_| ids.alloc()).collect();
    pdf.pages(tree)
        .kids(page_ids.iter().copied())
        .count(pages.len() as i32);
    for (page, page_id) in pages.iter().zip(&page_ids) {
        let content_id = ids.alloc();
        let mut used = Used::default();
        let content = page_content(&mut pdf, &mut ids, page, &mut used);
        let compressed = deflate(&content.finish());
        pdf.stream(content_id, &compressed)
            .filter(Filter::FlateDecode);
        let mut p = pdf.page(*page_id);
        p.media_box(Rect::new(
            0.0,
            0.0,
            (page.size.x * PT) as f32,
            (page.size.y * PT) as f32,
        ));
        p.parent(tree);
        p.contents(content_id);
        let mut res = p.resources();
        res.fonts().pair(Name(b"F1"), font);
        if !used.images.is_empty() {
            let mut x = res.x_objects();
            for (name, id) in &used.images {
                x.pair(Name(name.as_bytes()), *id);
            }
            x.finish();
        }
        if !used.alphas.is_empty() {
            let mut g = res.ext_g_states();
            for (a, id) in &used.alphas {
                g.pair(Name(format!("A{a}").as_bytes()), *id);
            }
            g.finish();
        }
        res.finish();
        p.finish();
    }
    pdf.finish()
}

fn page_content(pdf: &mut Pdf, ids: &mut Ids, page: &Page, used: &mut Used) -> Content {
    let mut c = Content::new();
    for prim in &page.prims {
        match prim {
            Prim::PushClip(poly) => {
                c.save_state();
                trace(&mut c, &PathPrim::polyline(poly), true);
                c.clip_nonzero();
                c.end_path();
            }
            Prim::PopClip => {
                c.restore_state();
            }
            Prim::Path(p) => path(&mut c, ids, used, p, pdf),
            Prim::Text(t) => text(&mut c, t),
            Prim::Image(i) => {
                let id = ids.alloc();
                let name = format!("I{}", used.images.len());
                let data = deflate(&i.rgb);
                let mut x = pdf.image_xobject(id, &data);
                x.filter(Filter::FlateDecode);
                x.width(i.width as i32);
                x.height(i.height as i32);
                x.color_space().device_rgb();
                x.bits_per_component(8);
                x.finish();
                let (w, h) = ((i.max.x - i.min.x) * PT, (i.max.y - i.min.y) * PT);
                c.save_state();
                c.transform([
                    w as f32,
                    0.0,
                    0.0,
                    h as f32,
                    (i.min.x * PT) as f32,
                    (i.min.y * PT) as f32,
                ]);
                c.x_object(Name(name.as_bytes()));
                c.restore_state();
                used.images.push((name, id));
            }
        }
    }
    c
}

/// Sets the transparency of a color, if it is not opaque. Call inside `q ... Q`.
fn alpha(c: &mut Content, ids: &mut Ids, used: &mut Used, a: u8, pdf: &mut Pdf) {
    if a == 255 {
        return;
    }
    let id = *used.alphas.entry(a).or_insert_with(|| {
        let id = ids.alloc();
        pdf.ext_graphics(id)
            .non_stroking_alpha(a as f32 / 255.0)
            .stroking_alpha(a as f32 / 255.0);
        id
    });
    let _ = id;
    c.set_parameters(Name(format!("A{a}").as_bytes()));
}

fn path(c: &mut Content, ids: &mut Ids, used: &mut Used, p: &PathPrim, pdf: &mut Pdf) {
    if p.segs.is_empty() || (p.stroke.is_none() && p.fill.is_none()) {
        return;
    }
    c.save_state();
    if let Some(s) = p.stroke {
        let [r, g, b] = rgb(s);
        c.set_stroke_rgb(r, g, b);
        c.set_line_width((p.width * PT) as f32);
        c.set_line_cap(LineCapStyle::ButtCap);
    }
    if let Some(f) = p.fill {
        let [r, g, b] = rgb(f);
        c.set_fill_rgb(r, g, b);
    }
    // One transparency for the fill and the stroke: the fill's, else the stroke's.
    alpha(
        c,
        ids,
        used,
        p.fill
            .map_or_else(|| p.stroke.map_or(255, |s| s[3]), |f| f[3]),
        pdf,
    );
    if let Some([d, g]) = p.dash {
        c.set_dash_pattern([(d * PT) as f32, (g * PT) as f32], 0.0);
    }
    trace(c, &p.segs, false);
    match (p.fill.is_some(), p.stroke.is_some(), p.closed) {
        (true, true, true) => c.close_fill_nonzero_and_stroke(),
        (true, true, false) => c.fill_nonzero_and_stroke(),
        (true, false, _) => c.fill_nonzero(),
        (false, true, true) => c.close_and_stroke(),
        (false, true, false) => c.stroke(),
        (false, false, _) => c.end_path(),
    };
    c.restore_state();
}

fn text(c: &mut Content, t: &TextPrim) {
    if t.text.is_empty() || t.height <= 0.0 {
        return;
    }
    let mut x = t.at.x;
    if t.align == Align::Center {
        x -= text_width(&t.text, t.height) / 2.0;
    }
    let [r, g, b] = rgb(t.color);
    c.begin_text();
    c.set_fill_rgb(r, g, b);
    c.set_font(Name(b"F1"), (t.height * PT) as f32);
    c.next_line((x * PT) as f32, (t.at.y * PT) as f32);
    c.show(Str(&winansi(&t.text)));
    c.end_text();
}
