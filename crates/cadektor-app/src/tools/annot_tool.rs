//! The annotation tools: Dimension (with modes), Callout, Data Tag, North Arrow, Scale Bar and
//! Revision Cloud. They collect clicks and make one annotation object per operation.
//! The math is in `cadektor_core::annot`.

use cadektor_core::annot::{
    self, Anchor, AnchorTarget, Annot, Axis, Callout, DataTag, DimMode, Dimension, Leader,
    NorthArrow, RevisionCloud, ScaleBar, shape_vertices,
};
use cadektor_core::command::Command;
use cadektor_core::model::{Attributes, Document, Shape};
use cadektor_core::resource::ResourceId;
use glam::DVec2;

use super::{Ctx, Effect, Input, Preview, State, Tool, ToolMachine};
use crate::hit;

/// What the Dimension tool measures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DimToolMode {
    /// Constrained horizontal: click two points, then the place of the line.
    #[default]
    Horizontal,
    Vertical,
    Aligned,
    /// Click the points, Enter finishes. One line with a value per segment.
    Chain,
    /// Click the points, Enter finishes. Every value runs from the first point.
    Baseline,
    /// Click the datum, then the points, Enter finishes.
    Ordinate,
    /// Click the vertex, then a point on each ray.
    Angular,
    /// Click a circle or an arc.
    Radial,
    Diameter,
    /// Click an arc.
    ArcLength,
}

impl DimToolMode {
    pub const ALL: [DimToolMode; 10] = [
        DimToolMode::Horizontal,
        DimToolMode::Vertical,
        DimToolMode::Aligned,
        DimToolMode::Chain,
        DimToolMode::Baseline,
        DimToolMode::Ordinate,
        DimToolMode::Angular,
        DimToolMode::Radial,
        DimToolMode::Diameter,
        DimToolMode::ArcLength,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DimToolMode::Horizontal => "Horizontal",
            DimToolMode::Vertical => "Vertical",
            DimToolMode::Aligned => "Aligned",
            DimToolMode::Chain => "Chain",
            DimToolMode::Baseline => "Baseline",
            DimToolMode::Ordinate => "Ordinate",
            DimToolMode::Angular => "Angular",
            DimToolMode::Radial => "Radial",
            DimToolMode::Diameter => "Diameter",
            DimToolMode::ArcLength => "Arc length",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            DimToolMode::Horizontal | DimToolMode::Vertical | DimToolMode::Aligned => {
                "Click two points, then click where the dimension line goes."
            }
            DimToolMode::Chain | DimToolMode::Baseline => {
                "Click the points. Enter or double-click finishes."
            }
            DimToolMode::Ordinate => {
                "Click the datum, then the points. Enter or double-click finishes."
            }
            DimToolMode::Angular => {
                "Click the vertex, then a point on the first ray and a point on the second ray."
            }
            DimToolMode::Radial | DimToolMode::Diameter => "Click a circle or an arc.",
            DimToolMode::ArcLength => "Click an arc.",
        }
    }

    pub fn next(self) -> DimToolMode {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn mode(self) -> DimMode {
        match self {
            DimToolMode::Horizontal => DimMode::Linear {
                axis: Axis::Horizontal,
            },
            DimToolMode::Vertical => DimMode::Linear {
                axis: Axis::Vertical,
            },
            DimToolMode::Aligned => DimMode::Linear {
                axis: Axis::Aligned,
            },
            DimToolMode::Chain => DimMode::Chain {
                axis: Axis::Horizontal,
            },
            DimToolMode::Baseline => DimMode::Baseline {
                axis: Axis::Horizontal,
            },
            DimToolMode::Ordinate => DimMode::Ordinate {
                axis: Axis::Horizontal,
            },
            DimToolMode::Angular => DimMode::Angular,
            DimToolMode::Radial => DimMode::Radial,
            DimToolMode::Diameter => DimMode::Diameter,
            DimToolMode::ArcLength => DimMode::ArcLength,
        }
    }

    /// Modes that click a list of points and finish with Enter.
    fn is_list(self) -> bool {
        matches!(
            self,
            DimToolMode::Chain | DimToolMode::Baseline | DimToolMode::Ordinate
        )
    }

    /// Modes that click an existing circle or arc.
    fn is_curve(self) -> bool {
        matches!(
            self,
            DimToolMode::Radial | DimToolMode::Diameter | DimToolMode::ArcLength
        )
    }

    /// Number of clicks before the line is placed or the dimension is made.
    fn clicks(self) -> usize {
        match self {
            DimToolMode::Angular => 3,
            _ => 2,
        }
    }
}

/// Settings of the annotation tools.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotSettings {
    pub dim_mode: DimToolMode,
    pub dim_style: Option<ResourceId>,
    pub tag_style: Option<ResourceId>,
    pub text_style: Option<ResourceId>,
    /// Distance of dimension lines of chain, baseline and ordinate dimensions, in paper mm.
    pub offset_mm: f64,
    pub callout_text: String,
    pub revision: String,
    pub north_size_mm: f64,
    pub bar_segment_m: f64,
    pub bar_segments: u32,
    /// Commands from buttons of the panel that the app runs next.
    pub pending: Vec<Command>,
    /// The Graphic Legend button was pressed.
    pub legend_requested: bool,
}

impl Default for AnnotSettings {
    fn default() -> Self {
        AnnotSettings {
            dim_mode: DimToolMode::Horizontal,
            dim_style: None,
            tag_style: None,
            text_style: None,
            offset_mm: 8.0,
            callout_text: "Note".into(),
            revision: "A".into(),
            north_size_mm: 15.0,
            bar_segment_m: 5.0,
            bar_segments: 4,
            pending: Vec::new(),
            legend_requested: false,
        }
    }
}

fn add(a: Annot) -> Effect {
    Effect::Exec(Command::AddObject {
        shape: Shape::Annotation(Box::new(a)),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
}

/// The scale of the active layer: layer units per paper mm.
fn paper(doc: &Document) -> f64 {
    doc.layer(doc.active_layer).map_or(1.0, |l| l.scale)
}

/// The vertex of an object that lies at `p` (the click was snapped to it), as an anchor.
fn anchor_at(doc: &Document, p: DVec2, tol: f64) -> Option<Anchor> {
    doc.objects
        .values()
        .rev()
        .filter(|o| {
            doc.is_pickable(o)
                && doc.layer(o.layer).is_some_and(|l| !l.locked)
                && !matches!(o.shape, Shape::Annotation(_))
        })
        .flat_map(|o| {
            shape_vertices(&o.shape)
                .into_iter()
                .enumerate()
                .map(move |(i, v)| (o.id, i, v.distance(p)))
        })
        .filter(|(_, _, d)| *d <= tol * 0.1)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(object, index, _)| Anchor::new(object, AnchorTarget::Vertex { index }))
}

fn dimension(
    mode: DimMode,
    points: Vec<DVec2>,
    anchors: Vec<Option<Anchor>>,
    offset: f64,
    style: Option<ResourceId>,
) -> Annot {
    let mut anchors = anchors;
    while anchors.last().is_some_and(Option::is_none) {
        anchors.pop();
    }
    Annot::Dimension(Dimension {
        mode,
        points,
        anchors: if anchors.iter().all(Option::is_none) {
            Vec::new()
        } else {
            anchors
        },
        offset,
        style,
        text: None,
        detached: false,
    })
}

impl ToolMachine {
    /// Input of the annotation tools.
    pub(super) fn handle_annot(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        match self.tool {
            Tool::Dimension => self.handle_dimension(input, cx),
            Tool::Callout => self.handle_callout(input, cx),
            Tool::DataTag => match input {
                Input::Press { p, .. } => self.place_tag(p, cx),
                _ => vec![],
            },
            Tool::NorthArrow | Tool::ScaleBar => match input {
                Input::Press { p, .. } => {
                    let s = &cx.settings.annot;
                    vec![add(if self.tool == Tool::NorthArrow {
                        Annot::NorthArrow(NorthArrow {
                            at: p,
                            size_mm: s.north_size_mm,
                            rotation_deg: 0.0,
                            use_georef: false,
                            style: Default::default(),
                        })
                    } else {
                        Annot::ScaleBar(ScaleBar {
                            at: p,
                            unit: annot::BarUnit::M,
                            segment: s.bar_segment_m,
                            segments: s.bar_segments,
                            height_mm: 2.0,
                            scale: None,
                            style: s.text_style,
                        })
                    })]
                }
                _ => vec![],
            },
            Tool::RevisionCloud => match input {
                Input::Press { p, .. } => {
                    let mut pts = match std::mem::replace(&mut self.state, State::Idle) {
                        State::Points(v) => v,
                        _ => vec![],
                    };
                    pts.push(p);
                    self.state = State::Points(pts);
                    vec![]
                }
                Input::DoubleClick | Input::Enter => {
                    let State::Points(mut pts) = std::mem::replace(&mut self.state, State::Idle)
                    else {
                        return vec![];
                    };
                    pts.dedup();
                    if pts.len() < 3 {
                        return vec![];
                    }
                    let s = &cx.settings.annot;
                    vec![add(Annot::RevisionCloud(RevisionCloud {
                        points: pts,
                        bump_mm: 6.0,
                        revision: s.revision.clone(),
                        date: String::new(),
                        description: String::new(),
                        author: String::new(),
                        mark: true,
                    }))]
                }
                _ => vec![],
            },
            _ => vec![],
        }
    }

    fn handle_dimension(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        let s = &cx.settings.annot;
        let mode = s.dim_mode;
        match input {
            Input::Press { p, .. } if mode.is_curve() => {
                let Some(obj) = hit::hit_test(cx.doc, p, cx.tol).and_then(|id| cx.doc.object(id))
                else {
                    return vec![Effect::Notice("Click a circle or an arc".into())];
                };
                let (center, radius, arc) = match &obj.shape {
                    Shape::Circle { center, radius } => (*center, *radius, None),
                    Shape::Arc {
                        center,
                        radius,
                        start,
                        end,
                    } => (*center, *radius, Some((*start, *end))),
                    _ => return vec![Effect::Notice("Click a circle or an arc".into())],
                };
                let center_anchor = Some(Anchor::new(obj.id, AnchorTarget::Center));
                let dir = (p - center).normalize_or(DVec2::X);
                let a = match mode {
                    DimToolMode::ArcLength => {
                        let Some((start, end)) = arc else {
                            return vec![Effect::Notice("Arc length needs an arc".into())];
                        };
                        let sweep = cadektor_core::model::arc_sweep(start, end);
                        let pts = vec![
                            center,
                            center + DVec2::from_angle(start) * radius,
                            center + DVec2::from_angle(start + sweep) * radius,
                        ];
                        dimension(
                            DimMode::ArcLength,
                            pts,
                            vec![center_anchor],
                            s.offset_mm,
                            s.dim_style,
                        )
                    }
                    m => dimension(
                        m.mode(),
                        vec![center, center + dir * radius],
                        vec![center_anchor],
                        0.0,
                        s.dim_style,
                    ),
                };
                vec![add(a)]
            }
            Input::Press { p, .. } => {
                let mut pts = match std::mem::replace(&mut self.state, State::Idle) {
                    State::Points(v) => v,
                    _ => vec![],
                };
                // The click that places the line of a linear dimension.
                if pts.len() == mode.clicks() && !mode.is_list() && mode != DimToolMode::Angular {
                    let m = mode.mode();
                    let off = annot::dim::offset_for(m, &pts, p, paper(cx.doc));
                    let anchors = pts.iter().map(|q| anchor_at(cx.doc, *q, cx.tol)).collect();
                    return vec![add(dimension(m, pts, anchors, off, s.dim_style))];
                }
                pts.push(p);
                if mode == DimToolMode::Angular && pts.len() == 3 {
                    let (a, b) = annot::dim::angular_order(pts[0], pts[1], pts[2]);
                    pts = vec![pts[0], a, b];
                    let anchors = pts.iter().map(|q| anchor_at(cx.doc, *q, cx.tol)).collect();
                    return vec![add(dimension(
                        DimMode::Angular,
                        pts,
                        anchors,
                        0.0,
                        s.dim_style,
                    ))];
                }
                self.state = State::Points(pts);
                vec![]
            }
            Input::DoubleClick | Input::Enter if mode.is_list() => {
                let State::Points(mut pts) = std::mem::replace(&mut self.state, State::Idle) else {
                    return vec![];
                };
                pts.dedup();
                let need = if mode == DimToolMode::Ordinate { 2 } else { 3 };
                if pts.len() < need {
                    return vec![];
                }
                let anchors = pts.iter().map(|q| anchor_at(cx.doc, *q, cx.tol)).collect();
                vec![add(dimension(
                    mode.mode(),
                    pts,
                    anchors,
                    s.offset_mm,
                    s.dim_style,
                ))]
            }
            _ => vec![],
        }
    }

    fn handle_callout(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        let Input::Press { p, .. } = input else {
            return vec![];
        };
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Points(pts) if !pts.is_empty() => {
                let s = &cx.settings.annot;
                let anchor = anchor_at(cx.doc, pts[0], cx.tol);
                vec![add(Annot::Callout(Callout {
                    at: p,
                    text: s.callout_text.clone(),
                    note: None,
                    notes_db: None,
                    style: s.text_style,
                    leaders: vec![Leader {
                        points: vec![pts[0]],
                        anchor,
                    }],
                    shoulder_mm: 5.0,
                    terminator: Default::default(),
                    detached: false,
                }))]
            }
            _ => {
                self.state = State::Points(vec![p]);
                vec![]
            }
        }
    }

    fn place_tag(&self, p: DVec2, cx: &Ctx) -> Vec<Effect> {
        let doc = cx.doc;
        let style = cx
            .settings
            .annot
            .tag_style
            .filter(|s| doc.resources.tag_style(*s).is_some())
            .or_else(|| {
                doc.resources
                    .iter()
                    .find(|r| doc.resources.tag_style(r.id).is_some())
                    .map(|r| r.id)
            });
        let Some(style) = style else {
            return vec![Effect::Notice(
                "Create a tag style first (Data Tag panel or the tag_style.create API method)"
                    .into(),
            )];
        };
        let Some(obj) = hit::hit_test(doc, p, cx.tol)
            .and_then(|id| doc.object(id))
            .filter(|o| !matches!(o.shape, Shape::Annotation(_)))
        else {
            return vec![Effect::Notice("Click the object to tag".into())];
        };
        let Some(anchor) = annot::ref_point(&obj.shape) else {
            return vec![Effect::Notice("This object cannot be tagged".into())];
        };
        let offset = DVec2::new(8.0, 8.0);
        let scale = doc.layer(obj.layer).map_or(1.0, |l| l.scale);
        vec![Effect::Exec(Command::AddObject {
            shape: Shape::Annotation(Box::new(Annot::DataTag(DataTag {
                style,
                object: obj.id,
                offset,
                at: anchor + offset * scale,
                anchor,
            }))),
            layer: Some(obj.layer),
            class: None,
            attrs: Attributes::default(),
        })]
    }

    /// Feedback of the annotation tools: the annotation as it would be made now.
    pub(super) fn annot_preview(&self, cursor: DVec2, cx: &Ctx) -> Option<Preview> {
        let s = &cx.settings.annot;
        let pts = match &self.state {
            State::Points(p) => p.clone(),
            _ => vec![],
        };
        let shape = |a: Annot| Some(Preview::Shape(Shape::Annotation(Box::new(a))));
        match self.tool {
            Tool::Dimension if s.dim_mode.is_curve() => None,
            Tool::Dimension => {
                let mode = s.dim_mode;
                let m = mode.mode();
                let (lo, _) = m.point_range();
                if pts.is_empty() {
                    return None;
                }
                if !mode.is_list() && pts.len() == mode.clicks() && mode != DimToolMode::Angular {
                    let off = annot::dim::offset_for(m, &pts, cursor, paper(cx.doc));
                    return shape(dimension(m, pts, vec![], off, s.dim_style));
                }
                let mut all = pts;
                all.push(cursor);
                if mode == DimToolMode::Angular && all.len() == 3 {
                    let (a, b) = annot::dim::angular_order(all[0], all[1], all[2]);
                    all = vec![all[0], a, b];
                }
                (all.len() >= lo)
                    .then(|| dimension(m, all, vec![], s.offset_mm, s.dim_style))
                    .and_then(shape)
            }
            Tool::NorthArrow => shape(Annot::NorthArrow(NorthArrow {
                at: cursor,
                size_mm: s.north_size_mm,
                rotation_deg: 0.0,
                use_georef: false,
                style: Default::default(),
            })),
            Tool::ScaleBar => shape(Annot::ScaleBar(ScaleBar {
                at: cursor,
                unit: annot::BarUnit::M,
                segment: s.bar_segment_m,
                segments: s.bar_segments,
                height_mm: 2.0,
                scale: None,
                style: s.text_style,
            })),
            Tool::RevisionCloud if !pts.is_empty() => {
                let mut all = pts;
                all.push(cursor);
                (all.len() >= 3)
                    .then(|| {
                        Annot::RevisionCloud(RevisionCloud {
                            points: all,
                            bump_mm: 6.0,
                            revision: s.revision.clone(),
                            date: String::new(),
                            description: String::new(),
                            author: String::new(),
                            mark: false,
                        })
                    })
                    .and_then(shape)
            }
            Tool::Callout if !pts.is_empty() => shape(Annot::Callout(Callout {
                at: cursor,
                text: s.callout_text.clone(),
                note: None,
                notes_db: None,
                style: s.text_style,
                leaders: vec![Leader {
                    points: vec![pts[0]],
                    anchor: None,
                }],
                shoulder_mm: 5.0,
                terminator: Default::default(),
                detached: false,
            })),
            _ => None,
        }
    }

    /// The hint for the status line.
    pub(super) fn annot_hint(&self, settings: &super::ToolSettings) -> &'static str {
        match self.tool {
            Tool::Dimension => settings.annot.dim_mode.help(),
            Tool::Callout => "Click the point to point at, then click where the text goes.",
            Tool::DataTag => "Click the object to tag. The tag style is in the Tool bar.",
            Tool::NorthArrow => "Click to place the north arrow.",
            Tool::ScaleBar => "Click to place the scale bar. It follows the scale of the layer.",
            _ => "Click the corners around the changes. Enter or double-click finishes.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolSettings;

    fn click(m: &mut ToolMachine, doc: &Document, s: &ToolSettings, p: DVec2) -> Vec<Effect> {
        let cx = Ctx {
            doc,
            selection: &[],
            settings: s,
            tol: 1.0,
        };
        m.handle(Input::Press { p, shift: false }, &cx)
    }

    fn made(fx: &[Effect]) -> Annot {
        let Some(Effect::Exec(Command::AddObject {
            shape: Shape::Annotation(a),
            ..
        })) = fx.first()
        else {
            panic!("{fx:?}")
        };
        (**a).clone()
    }

    #[test]
    fn clockwise_angular_clicks_measure_the_small_angle() {
        let doc = Document::default();
        let mut s = ToolSettings::default();
        s.annot.dim_mode = DimToolMode::Angular;
        for (p1, p2) in [
            (DVec2::new(1000.0, 0.0), DVec2::new(0.0, 1000.0)),
            (DVec2::new(0.0, 1000.0), DVec2::new(1000.0, 0.0)),
        ] {
            let mut m = ToolMachine::default();
            m.set_tool(Tool::Dimension);
            assert!(click(&mut m, &doc, &s, DVec2::ZERO).is_empty());
            assert!(click(&mut m, &doc, &s, p1).is_empty());
            let Annot::Dimension(d) = made(&click(&mut m, &doc, &s, p2)) else {
                panic!()
            };
            assert!(
                (annot::dim::values(&d)[0] - 90.0).abs() < 1e-9,
                "both click orders give 90 degrees"
            );
        }
    }

    #[test]
    fn the_tools_make_annotations() {
        let doc = Document::default();
        let s = ToolSettings::default();
        let mut m = ToolMachine::default();
        m.set_tool(Tool::Dimension);
        click(&mut m, &doc, &s, DVec2::ZERO);
        click(&mut m, &doc, &s, DVec2::new(5000.0, 0.0));
        assert!(
            m.preview(
                DVec2::new(100.0, 1000.0),
                &Ctx {
                    doc: &doc,
                    selection: &[],
                    settings: &s,
                    tol: 1.0
                }
            ) != Preview::None
        );
        let Annot::Dimension(d) = made(&click(&mut m, &doc, &s, DVec2::new(2000.0, 1000.0))) else {
            panic!()
        };
        assert_eq!(d.offset, 10.0, "1000 units at 1:100 are 10 paper mm");
        for (tool, kind) in [
            (Tool::NorthArrow, "north_arrow"),
            (Tool::ScaleBar, "scale_bar"),
        ] {
            let mut m = ToolMachine::default();
            m.set_tool(tool);
            assert_eq!(
                made(&click(&mut m, &doc, &s, DVec2::ZERO)).type_name(),
                kind
            );
        }
        let mut m = ToolMachine::default();
        m.set_tool(Tool::RevisionCloud);
        for p in [DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0)] {
            click(&mut m, &doc, &s, p);
        }
        let cx = Ctx {
            doc: &doc,
            selection: &[],
            settings: &s,
            tol: 1.0,
        };
        assert!(m.preview(DVec2::new(0.0, 10.0), &cx) != Preview::None);
        assert_eq!(
            made(&m.handle(Input::Enter, &cx)).type_name(),
            "revision_cloud"
        );
        // A data tag needs a tag style and an object.
        let mut m = ToolMachine::default();
        m.set_tool(Tool::DataTag);
        assert!(matches!(
            click(&mut m, &doc, &s, DVec2::ZERO)[0],
            Effect::Notice(_)
        ));
        // A callout takes two clicks.
        let mut m = ToolMachine::default();
        m.set_tool(Tool::Callout);
        assert!(click(&mut m, &doc, &s, DVec2::ZERO).is_empty());
        assert!(m.preview(DVec2::new(5.0, 5.0), &cx) != Preview::None);
        assert_eq!(
            made(&click(&mut m, &doc, &s, DVec2::new(500.0, 500.0))).type_name(),
            "callout"
        );
        // Radial needs a circle.
        let mut s2 = s.clone();
        s2.annot.dim_mode = DimToolMode::Radial;
        let mut m = ToolMachine::default();
        m.set_tool(Tool::Dimension);
        assert!(matches!(
            click(&mut m, &doc, &s2, DVec2::ZERO)[0],
            Effect::Notice(_)
        ));
        for mode in DimToolMode::ALL {
            assert!(!mode.label().is_empty() && !mode.help().is_empty());
            assert_ne!(mode.next(), mode);
        }
        assert!(!m.annot_hint(&s2).is_empty());
    }
}
