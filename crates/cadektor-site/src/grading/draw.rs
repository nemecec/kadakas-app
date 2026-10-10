//! Neutral drawing primitives of the grading objects. The plan SVG, the app canvas and the print
//! display list all draw from the same `Drawn`, so a sheet shows what the canvas shows.
//!
//! Positions are model coordinates (mm, Y up). Line weights and text heights are model mm too:
//! the builders multiply paper sizes by the scale of the layer.

use std::fmt::Write;

use glam::DVec2;

use cadektor_core::model::ObjectId;

use super::esc;

/// A filled or stroked set of rings. A fill uses the even-odd rule, so a ring inside a ring is a hole.
#[derive(Clone, Debug, PartialEq)]
pub struct Poly {
    pub rings: Vec<Vec<DVec2>>,
    /// False: the rings are open lines.
    pub closed: bool,
    pub fill: Option<[u8; 4]>,
    pub stroke: Option<[u8; 4]>,
    /// Line weight in model mm.
    pub weight: f64,
    /// What it is, for `data-kind` in SVG and for tests.
    pub tag: &'static str,
    pub owner: ObjectId,
    /// The analysis band, if it belongs to one.
    pub band: Option<usize>,
}

impl Poly {
    pub fn new(tag: &'static str, owner: ObjectId, rings: Vec<Vec<DVec2>>, closed: bool) -> Poly {
        Poly {
            rings,
            closed,
            fill: None,
            stroke: None,
            weight: 0.0,
            tag,
            owner,
            band: None,
        }
    }

    pub fn filled(mut self, color: [u8; 4]) -> Poly {
        self.fill = Some(color);
        self
    }

    pub fn stroked(mut self, color: [u8; 4], weight: f64) -> Poly {
        self.stroke = Some(color);
        self.weight = weight;
        self
    }
}

/// A line of text.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// The start of the baseline, or its middle when `center` is set.
    pub at: DVec2,
    pub text: String,
    /// Font size in model mm.
    pub height: f64,
    pub center: bool,
    pub bold: bool,
    /// Counterclockwise, in degrees.
    pub angle_deg: f64,
    pub tag: &'static str,
    pub owner: ObjectId,
}

impl Label {
    pub fn new(tag: &'static str, owner: ObjectId, at: DVec2, text: String, height: f64) -> Label {
        Label {
            at,
            text,
            height,
            center: false,
            bold: false,
            angle_deg: 0.0,
            tag,
            owner,
        }
    }
}

/// Everything that one object adds to a drawing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawn {
    pub polys: Vec<Poly>,
    pub labels: Vec<Label>,
}

fn css(c: [u8; 4]) -> String {
    format!("rgb({},{},{})", c[0], c[1], c[2])
}

fn opacity(c: [u8; 4]) -> String {
    if c[3] == 255 {
        String::new()
    } else {
        format!(
            " OPACITY-opacity=\"{}\"",
            (c[3] as f64 / 255.0 * 1000.0).round() / 1000.0
        )
    }
}

impl Drawn {
    pub fn is_empty(&self) -> bool {
        self.polys.is_empty() && self.labels.is_empty()
    }

    pub fn extend(&mut self, other: Drawn) {
        self.polys.extend(other.polys);
        self.labels.extend(other.labels);
    }

    /// SVG elements, with the Y axis flipped as in the plan SVG of core.
    pub fn svg(&self) -> String {
        let mut out = String::new();
        for p in &self.polys {
            let mut d = String::new();
            for ring in p.rings.iter().filter(|r| !r.is_empty()) {
                for (i, q) in ring.iter().enumerate() {
                    let _ = write!(d, "{}{} {}", if i == 0 { "M " } else { " L " }, q.x, -q.y);
                }
                d.push_str(if p.closed { " Z " } else { " " });
            }
            let fill = p.fill.map_or("none".to_string(), css);
            let fill_op = p
                .fill
                .map_or(String::new(), opacity)
                .replace("OPACITY", "fill");
            let stroke = match p.stroke {
                Some(c) => format!(
                    r#" stroke="{}" stroke-width="{}"{}"#,
                    css(c),
                    p.weight,
                    opacity(c).replace("OPACITY", "stroke")
                ),
                None => r#" stroke="none""#.to_string(),
            };
            let band = p
                .band
                .map_or(String::new(), |b| format!(r#" data-band="{b}""#));
            let _ = writeln!(
                out,
                r#"<path d="{}" fill="{fill}"{fill_op}{stroke} fill-rule="evenodd" data-kind="{}" data-owner="{}"{band}/>"#,
                d.trim_end(),
                p.tag,
                p.owner
            );
        }
        for l in &self.labels {
            let anchor = if l.center {
                r#" text-anchor="middle""#
            } else {
                ""
            };
            let weight = if l.bold { r#" font-weight="bold""# } else { "" };
            let angle = if l.angle_deg == 0.0 {
                0.0
            } else {
                -l.angle_deg
            };
            let _ = writeln!(
                out,
                r#"<text x="{}" y="{}" font-size="{}" font-family="sans-serif"{anchor}{weight} fill="black" transform="rotate({angle} {} {})" data-kind="{}" data-owner="{}">{}</text>"#,
                l.at.x,
                -l.at.y,
                l.height,
                l.at.x,
                -l.at.y,
                l.tag,
                l.owner,
                esc(&l.text)
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_has_paths_and_text() {
        let mut d = Drawn::default();
        assert!(d.is_empty());
        let ring = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 5.0),
        ];
        d.polys.push(
            Poly::new(
                "t",
                ObjectId::from_num(3),
                vec![ring.clone(), ring.clone()],
                true,
            )
            .filled([1, 2, 3, 128])
            .stroked([4, 5, 6, 255], 0.5),
        );
        let mut open = Poly::new("line", ObjectId::from_num(3), vec![ring], false)
            .stroked([0, 0, 0, 255], 1.0);
        open.band = Some(2);
        d.polys.push(open);
        let mut l = Label::new(
            "label",
            ObjectId::from_num(3),
            DVec2::new(1.0, 2.0),
            "a<b".into(),
            4.0,
        );
        l.center = true;
        l.bold = true;
        l.angle_deg = 30.0;
        d.labels.push(l);
        let mut other = Drawn::default();
        other.labels.push(Label::new(
            "plain",
            ObjectId::from_num(3),
            DVec2::ZERO,
            "x".into(),
            1.0,
        ));
        d.extend(other);
        assert!(!d.is_empty());
        let svg = d.svg();
        let lines: Vec<&str> = svg.lines().collect();
        assert_eq!(lines.len(), 4);
        // A filled shape with two rings is one path with two subpaths, with a fill opacity.
        assert!(
            lines[0].contains(r#"fill="rgb(1,2,3)""#)
                && lines[0].contains(r#"fill-opacity="0.502""#),
            "{svg}"
        );
        assert_eq!(lines[0].matches('M').count(), 2);
        assert!(
            lines[0].contains(r#"stroke="rgb(4,5,6)""#)
                && lines[0].contains(r#"stroke-width="0.5""#)
        );
        assert!(
            lines[0].contains(r#"data-kind="t""#)
                && lines[0].contains(&format!(r#"data-owner="{}""#, ObjectId::from_num(3)))
        );
        // An open line is not closed and has no fill. The band shows.
        assert!(
            lines[1].contains(r#"fill="none""#)
                && !lines[1].contains(" Z")
                && lines[1].contains(r#"data-band="2""#)
        );
        // A shape with a fill and no pen has no stroke.
        let plain = Drawn {
            polys: vec![
                Poly::new(
                    "p",
                    ObjectId::from_num(1),
                    vec![vec![DVec2::ZERO, DVec2::X, DVec2::Y]],
                    true,
                )
                .filled([9, 9, 9, 255]),
            ],
            labels: vec![],
        };
        assert!(plain.svg().contains(r#"stroke="none""#) && !plain.svg().contains("fill-opacity"));
        // Text: escaped, centered, bold, turned against the flipped Y axis.
        assert!(
            lines[2].contains("a&lt;b")
                && lines[2].contains(r#"text-anchor="middle""#)
                && lines[2].contains(r#"font-weight="bold""#)
        );
        assert!(lines[2].contains("rotate(-30 1 -2)"));
        assert!(lines[3].contains("rotate(0 0 -0)") && !lines[3].contains("text-anchor"));
    }
}
