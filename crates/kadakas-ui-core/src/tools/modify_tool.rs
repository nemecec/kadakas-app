//! The modify tools: Rotate, Mirror, Scale, Offset, Fillet, Chamfer, Trim, Extend and Split.
//!
//! The tools collect clicks and make one command per operation. The math is in
//! `kadakas_core::edit2d`; the plans in `edit2d::plan` build the commands.

use glam::DVec2;
use kadakas_core::command::Command;
use kadakas_core::edit2d::Transform;
use kadakas_core::edit2d::modify::{ChamferSize, JoinKind, JoinMode};
use kadakas_core::edit2d::offset::Side;
use kadakas_core::edit2d::plan::{self, Corner, OffsetTo};
use kadakas_core::model::ObjectId;
use kadakas_l10n::tr;

use super::{Ctx, Effect, Input, Preview, State, Tool, ToolMachine};
use crate::hit;

/// How the Trim and Extend tools pick the objects they cut to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CutTo {
    /// All other objects of the layer.
    #[default]
    AllObjects,
    /// Only the objects that are selected.
    SelectedObjects,
}

/// Options of the modify tools (the Tool bar fields).
#[derive(Clone, Debug, PartialEq)]
pub struct ModifySettings {
    /// Rotate, Mirror and Scale make copies and leave the originals.
    pub copy: bool,
    /// Fillet radius in mm.
    pub radius: f64,
    /// Chamfer distances from the corner, in mm.
    pub chamfer_a: f64,
    pub chamfer_b: f64,
    /// Standard, Split or Trim, for Fillet and Chamfer.
    pub join_mode: JoinMode,
    /// Offset distance in mm. 0: the copy passes through the second click.
    pub offset_distance: f64,
    /// Offset makes a copy (default) or moves the object.
    pub offset_copy: bool,
    pub cut_to: CutTo,
}

impl Default for ModifySettings {
    fn default() -> Self {
        ModifySettings {
            copy: false,
            radius: 500.0,
            chamfer_a: 500.0,
            chamfer_b: 500.0,
            join_mode: JoinMode::Trim,
            offset_distance: 0.0,
            offset_copy: true,
            cut_to: CutTo::AllObjects,
        }
    }
}

/// What a modify tool has collected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModifyState {
    pub pts: Vec<DVec2>,
    /// The first object and the place where it was clicked (Offset, Fillet, Chamfer).
    pub first: Option<(ObjectId, DVec2)>,
}

/// True for the tools of this module.
pub fn is_modify(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Rotate
            | Tool::Mirror
            | Tool::Scale
            | Tool::Offset
            | Tool::Fillet
            | Tool::Chamfer
            | Tool::Trim
            | Tool::Extend
            | Tool::Split
    )
}

/// The hint for the status line.
pub fn hint(tool: Tool, step: usize) -> String {
    match (tool, step) {
        (Tool::Rotate, 0) => tr!("uic-modify-tool-select-objects-then-click-the-center-of"),
        (Tool::Rotate, 1) => tr!("uic-modify-tool-click-the-start-direction"),
        (Tool::Rotate, _) => tr!("uic-modify-tool-click-the-end-direction"),
        (Tool::Mirror, 0) => tr!("uic-modify-tool-select-objects-then-click-the-first-point"),
        (Tool::Mirror, _) => tr!("uic-modify-tool-click-the-second-point-of-the-mirror"),
        (Tool::Scale, 0) => tr!("uic-modify-tool-select-objects-then-click-the-fixed-point"),
        (Tool::Scale, 1) => tr!("uic-modify-tool-click-the-reference-point"),
        (Tool::Scale, _) => tr!("uic-modify-tool-click-the-new-place-of-the-reference"),
        (Tool::Offset, 0) => tr!("uic-modify-tool-click-the-object-to-offset"),
        (Tool::Offset, _) => {
            tr!("uic-modify-tool-click-the-side-of-the-copy-set")
        }
        (Tool::Fillet, 0) => {
            tr!("uic-modify-tool-click-the-first-object-or-double-click")
        }
        (Tool::Fillet, _) => tr!("uic-modify-tool-click-the-second-object-or-a-neighbor"),
        (Tool::Chamfer, 0) => {
            tr!("uic-modify-tool-click-the-first-object-or-double-click-2")
        }
        (Tool::Chamfer, _) => tr!("uic-modify-tool-click-the-second-object-or-a-neighbor"),
        (Tool::Trim, _) => tr!("uic-modify-tool-click-the-part-of-an-object-to"),
        (Tool::Extend, _) => tr!("uic-modify-tool-click-the-end-of-an-object-to"),
        (Tool::Split, _) => tr!("uic-modify-tool-click-the-point-where-the-object-is"),
        _ => String::new(),
    }
}

impl ToolMachine {
    fn modify_state(&self) -> ModifyState {
        match &self.state {
            State::Modify(m) => m.clone(),
            _ => ModifyState::default(),
        }
    }

    /// The step of a modify tool: 0 before the first click.
    pub fn modify_step(&self) -> usize {
        match &self.state {
            State::Modify(m) => m.pts.len() + usize::from(m.first.is_some()),
            _ => 0,
        }
    }

    pub(super) fn handle_modify(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        let tool = self.tool;
        let mut st = self.modify_state();
        let s = &cx.settings.modify;
        let notice = |m: String| vec![Effect::Notice(m)];
        match input {
            Input::Press { p, .. } => match tool {
                Tool::Rotate | Tool::Mirror | Tool::Scale => {
                    if cx.selection.is_empty() {
                        self.state = State::Idle;
                        return notice(tr!("uic-modify-tool-select-the-objects-first"));
                    }
                    if st.pts.last() == Some(&p) {
                        return vec![];
                    }
                    st.pts.push(p);
                    let need = if tool == Tool::Mirror { 2 } else { 3 };
                    if st.pts.len() < need {
                        self.state = State::Modify(st);
                        return vec![];
                    }
                    self.state = State::Idle;
                    let ids = cx.selection.to_vec();
                    let pts = &st.pts;
                    let transform = match tool {
                        Tool::Mirror => Transform::Mirror {
                            a: pts[0],
                            b: pts[1],
                        },
                        Tool::Rotate => {
                            let (u, v) = (pts[1] - pts[0], pts[2] - pts[0]);
                            if u.length_squared() == 0.0 || v.length_squared() == 0.0 {
                                return notice(tr!(
                                    "uic-modify-tool-click-points-away-from-the-center"
                                ));
                            }
                            Transform::Rotate {
                                center: pts[0],
                                angle: v.to_angle() - u.to_angle(),
                            }
                        }
                        _ => {
                            let (u, v) = (pts[1].distance(pts[0]), pts[2].distance(pts[0]));
                            if u <= 0.0 || v <= 0.0 {
                                return notice(tr!(
                                    "uic-modify-tool-click-points-away-from-the-fixed-point"
                                ));
                            }
                            Transform::Scale {
                                center: pts[0],
                                sx: v / u,
                                sy: None,
                            }
                        }
                    };
                    vec![Effect::Exec(Command::TransformObjects {
                        ids,
                        transform,
                        copy: s.copy,
                    })]
                }
                Tool::Offset => match st.first {
                    None => match hit::hit_test_in(cx.annot, cx.doc, p, cx.tol, cx.context) {
                        Some(id) => {
                            st.first = Some((id, p));
                            self.state = State::Modify(st);
                            vec![]
                        }
                        None => notice(tr!("uic-modify-tool-click-an-object-to-offset")),
                    },
                    Some((id, _)) => {
                        self.state = State::Idle;
                        let to = if s.offset_distance > 0.0 {
                            let side = cx
                                .doc
                                .object(id)
                                .and_then(|o| {
                                    kadakas_core::edit2d::offset::offset_through(&o.shape, p).ok()
                                })
                                .map_or(Side::Left, |(_, side)| side);
                            OffsetTo::Distance(s.offset_distance, side)
                        } else {
                            OffsetTo::Through(p)
                        };
                        plan_effect(plan::offset(cx.doc, id, to, s.offset_copy))
                    }
                },
                Tool::Fillet | Tool::Chamfer => {
                    let Some(id) = hit::hit_test_in(cx.annot, cx.doc, p, cx.tol, cx.context) else {
                        return notice(tr!("uic-modify-tool-click-an-object"));
                    };
                    match st.first {
                        None => {
                            st.first = Some((id, p));
                            self.state = State::Modify(st);
                            vec![]
                        }
                        // The second click of a double-click: wait for the double-click.
                        Some((a, pa)) if a == id && pa.distance(p) <= cx.tol => vec![],
                        Some((a, pa)) => {
                            self.state = State::Idle;
                            plan_effect(plan::join(cx.doc, a, pa, id, p, corner(tool, s)))
                        }
                    }
                }
                Tool::Trim | Tool::Extend | Tool::Split => {
                    let Some(id) = hit::hit_test_in(cx.annot, cx.doc, p, cx.tol, cx.context) else {
                        return notice(tr!("uic-modify-tool-click-an-object"));
                    };
                    let others: Vec<ObjectId> = cx.selection.to_vec();
                    let list = (s.cut_to == CutTo::SelectedObjects).then_some(others.as_slice());
                    plan_effect(match tool {
                        Tool::Trim => plan::trim(cx.doc, id, p, list),
                        Tool::Extend => plan::extend(cx.doc, id, p, list),
                        _ => plan::split(cx.doc, id, &[p]),
                    })
                }
                _ => vec![],
            },
            Input::DoubleClick if matches!(tool, Tool::Fillet | Tool::Chamfer) => {
                // The first click of the double-click picked the object.
                let Some((id, _)) = st.first else {
                    return vec![];
                };
                self.state = State::Idle;
                plan_effect(plan::round_all(cx.doc, id, None, corner(tool, s).kind))
            }
            _ => vec![],
        }
    }

    /// Feedback for a modify tool: the lines that the clicks define.
    pub(super) fn modify_preview(&self, cursor: DVec2) -> Preview {
        let State::Modify(m) = &self.state else {
            return Preview::None;
        };
        if m.pts.is_empty() {
            return Preview::None;
        }
        let mut v = m.pts.clone();
        if self.tool == Tool::Rotate && v.len() == 2 {
            // Both rays from the center.
            v = vec![v[1], v[0], cursor];
        } else if self.tool == Tool::Scale && v.len() == 2 {
            v = vec![v[1], v[0], cursor];
        } else {
            v.push(cursor);
        }
        Preview::Path {
            vertices: kadakas_core::curve::corners(&v),
            closed: false,
        }
    }

    pub fn modify_readout(&self, cursor: DVec2) -> String {
        let State::Modify(m) = &self.state else {
            return String::new();
        };
        let Some(c) = m.pts.first() else {
            return String::new();
        };
        match (self.tool, m.pts.len()) {
            (Tool::Rotate, 2) => {
                let a = (cursor - *c).to_angle() - (m.pts[1] - *c).to_angle();
                tr!(
                    "uic-modify-tool-readout-turn",
                    angle = kadakas_units::current()
                        .plain_angle()
                        .angle_text_d(a.to_degrees(), 2)
                )
            }
            (Tool::Scale, 2) => {
                let (u, v) = (m.pts[1].distance(*c), cursor.distance(*c));
                tr!(
                    "uic-modify-tool-readout-scale",
                    factor =
                        kadakas_units::current().number_text(if u > 0.0 { v / u } else { 0.0 }, 3)
                )
            }
            _ => String::new(),
        }
    }
}

fn corner(tool: Tool, s: &ModifySettings) -> Corner {
    Corner {
        kind: if tool == Tool::Fillet {
            JoinKind::Fillet(s.radius)
        } else {
            JoinKind::Chamfer(ChamferSize::Distances(s.chamfer_a, s.chamfer_b))
        },
        mode: s.join_mode,
    }
}

fn plan_effect(plan: Result<Command, String>) -> Vec<Effect> {
    match plan {
        Ok(cmd) => vec![Effect::Exec(cmd)],
        Err(e) => vec![Effect::Notice(e)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolMachine, ToolSettings};
    use kadakas_core::command::Editor;
    use kadakas_core::model::{Attributes, Shape};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    struct Fx {
        ed: Editor,
        m: ToolMachine,
        settings: ToolSettings,
        selection: Vec<ObjectId>,
    }

    impl Fx {
        fn new(tool: Tool) -> Fx {
            let mut m = ToolMachine::default();
            m.set_tool(tool);
            Fx {
                ed: Editor::default(),
                m,
                settings: ToolSettings::default(),
                selection: vec![],
            }
        }

        fn add(&mut self, shape: Shape) -> ObjectId {
            ObjectId(
                self.ed
                    .execute(&Command::AddObject {
                        shape,
                        layer: None,
                        class: None,
                        attrs: Attributes::default(),
                    })
                    .unwrap()
                    .created[0],
            )
        }

        fn click(&mut self, p: DVec2) -> Vec<Effect> {
            let cx = Ctx {
                annot: Default::default(),
                doc: self.ed.doc(),
                selection: &self.selection,
                settings: &self.settings,
                tol: 5.0,
                context: None,
            };
            self.m.handle(Input::Press { p, shift: false }, &cx)
        }

        /// Clicks and runs what the tool asks for.
        fn run(&mut self, p: DVec2) -> Vec<String> {
            let mut notes = vec![];
            for e in self.click(p) {
                match e {
                    Effect::Exec(c) => {
                        self.ed.execute(&c).unwrap();
                    }
                    Effect::Notice(n) => notes.push(n),
                    Effect::SetSelection(_) => {}
                }
            }
            notes
        }
    }

    #[test]
    fn rotate_turns_the_selection_in_three_clicks() {
        let mut f = Fx::new(Tool::Rotate);
        let c = f.add(Shape::Circle {
            center: v(100.0, 0.0),
            radius: 10.0,
        });
        f.selection = vec![c];
        f.run(v(0.0, 0.0));
        assert_eq!(f.m.modify_step(), 1);
        f.run(v(50.0, 0.0));
        f.run(v(0.0, 50.0));
        assert!(!f.m.is_busy());
        let Shape::Circle { center, .. } = f.ed.doc().object(c).unwrap().shape else {
            panic!()
        };
        assert!(center.distance(v(0.0, 100.0)) < 1e-9);
        // Copy mode keeps the original.
        f.settings.modify.copy = true;
        for p in [v(0.0, 0.0), v(50.0, 0.0), v(0.0, 50.0)] {
            f.run(p);
        }
        assert_eq!(f.ed.doc().objects.len(), 2);
    }

    #[test]
    fn rotate_mirror_scale_need_a_selection() {
        for t in [Tool::Rotate, Tool::Mirror, Tool::Scale] {
            let mut f = Fx::new(t);
            assert_eq!(
                f.run(v(1.0, 1.0)),
                vec!["Select the objects first".to_string()]
            );
            assert!(!f.m.is_busy());
        }
    }

    #[test]
    fn mirror_and_scale() {
        let mut f = Fx::new(Tool::Mirror);
        let r = f.add(Shape::Rect {
            min: v(10.0, 0.0),
            max: v(20.0, 5.0),
        });
        f.selection = vec![r];
        f.run(v(0.0, 0.0));
        assert_eq!(f.m.modify_readout(v(1.0, 1.0)), "");
        f.run(v(0.0, 100.0));
        assert_eq!(
            f.ed.doc().object(r).unwrap().shape,
            Shape::Rect {
                min: v(-20.0, 0.0),
                max: v(-10.0, 5.0)
            }
        );
        let mut f = Fx::new(Tool::Scale);
        let c = f.add(Shape::Circle {
            center: v(10.0, 0.0),
            radius: 2.0,
        });
        f.selection = vec![c];
        f.run(v(0.0, 0.0));
        f.run(v(10.0, 0.0));
        assert_eq!(f.m.modify_readout(v(30.0, 0.0)), "Scale 3.000");
        f.run(v(30.0, 0.0));
        assert_eq!(
            f.ed.doc().object(c).unwrap().shape,
            Shape::Circle {
                center: v(30.0, 0.0),
                radius: 6.0
            }
        );
        // A point on the fixed point is refused.
        let mut f = Fx::new(Tool::Scale);
        f.selection = vec![c];
        f.run(v(0.0, 0.0));
        f.run(v(0.0, 0.0));
        assert!(f.m.modify_step() <= 1);
    }

    #[test]
    fn offset_by_a_click_or_a_distance() {
        let mut f = Fx::new(Tool::Offset);
        let l = f.add(Shape::Line {
            a: v(0.0, 0.0),
            b: v(100.0, 0.0),
        });
        assert!(f.run(v(500.0, 500.0)).len() == 1, "no object there");
        f.run(v(50.0, 0.0));
        assert_eq!(f.m.modify_step(), 1);
        f.run(v(50.0, 30.0));
        assert_eq!(f.ed.doc().objects.len(), 2);
        let _ = l;
        let Shape::Line { a, .. } = f.ed.doc().objects.values().last().unwrap().shape else {
            panic!()
        };
        assert_eq!(a, v(0.0, 30.0));
        // A fixed distance, on the side of the click.
        f.settings.modify.offset_distance = 10.0;
        f.run(v(50.0, 0.0));
        f.run(v(50.0, -3.0));
        let Shape::Line { a, .. } = f.ed.doc().objects.values().last().unwrap().shape else {
            panic!()
        };
        assert_eq!(a, v(0.0, -10.0));
    }

    #[test]
    fn fillet_chamfer_trim_extend_split() {
        let mut f = Fx::new(Tool::Fillet);
        let a = f.add(Shape::Line {
            a: v(0.0, 0.0),
            b: v(1000.0, 0.0),
        });
        f.add(Shape::Line {
            a: v(0.0, 0.0),
            b: v(0.0, 1000.0),
        });
        f.settings.modify.radius = 100.0;
        f.run(v(800.0, 0.0));
        f.run(v(0.0, 800.0));
        assert_eq!(f.ed.doc().objects.len(), 3);
        let Shape::Line { a: pa, .. } = f.ed.doc().object(a).unwrap().shape else {
            panic!()
        };
        assert!(pa.distance(v(100.0, 0.0)) < 1e-9);
        // Chamfer in standard mode leaves the lines.
        f.m.set_tool(Tool::Chamfer);
        f.settings.modify.join_mode = JoinMode::Standard;
        f.run(v(800.0, 0.0));
        f.run(v(0.0, 800.0));
        assert_eq!(f.ed.doc().objects.len(), 4);
        // Errors become notices.
        f.settings.modify.chamfer_a = -1.0;
        f.run(v(800.0, 0.0));
        let notes = f.run(v(0.0, 800.0));
        assert_eq!(notes.len(), 1);
        // Trim and extend.
        let mut f = Fx::new(Tool::Trim);
        let t = f.add(Shape::Line {
            a: v(0.0, 0.0),
            b: v(1000.0, 0.0),
        });
        f.add(Shape::Line {
            a: v(300.0, -50.0),
            b: v(300.0, 50.0),
        });
        f.run(v(600.0, 0.0));
        assert_eq!(
            f.ed.doc().object(t).unwrap().shape,
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(300.0, 0.0)
            }
        );
        f.m.set_tool(Tool::Extend);
        f.add(Shape::Line {
            a: v(900.0, -50.0),
            b: v(900.0, 50.0),
        });
        f.run(v(290.0, 0.0));
        assert_eq!(
            f.ed.doc().object(t).unwrap().shape,
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(900.0, 0.0)
            }
        );
        f.m.set_tool(Tool::Split);
        f.run(v(500.0, 0.0));
        assert_eq!(f.ed.doc().objects.len(), 4);
        // Only the selected cutters count.
        let mut f = Fx::new(Tool::Trim);
        f.settings.modify.cut_to = CutTo::SelectedObjects;
        let t = f.add(Shape::Line {
            a: v(0.0, 0.0),
            b: v(1000.0, 0.0),
        });
        f.add(Shape::Line {
            a: v(300.0, -50.0),
            b: v(300.0, 50.0),
        });
        let notes = f.run(v(600.0, 0.0));
        assert_eq!(notes.len(), 1, "no selected cutter");
        let _ = t;
    }

    #[test]
    fn double_click_rounds_every_corner() {
        let mut f = Fx::new(Tool::Fillet);
        let p = f.add(Shape::Polyline {
            points: kadakas_core::curve::corners(&[
                v(0.0, 0.0),
                v(1000.0, 0.0),
                v(1000.0, 1000.0),
                v(0.0, 1000.0),
            ]),
            closed: true,
        });
        f.settings.modify.radius = 100.0;
        f.run(v(500.0, 0.0));
        let cx = Ctx {
            annot: Default::default(),
            doc: f.ed.doc(),
            selection: &[],
            settings: &f.settings,
            tol: 5.0,
            context: None,
        };
        let effects = f.m.handle(Input::DoubleClick, &cx);
        assert_eq!(effects.len(), 1);
        if let Effect::Exec(c) = &effects[0] {
            f.ed.execute(c).unwrap();
        }
        let area = f.ed.doc().object(p).unwrap().shape.area().unwrap();
        assert!(
            (area - (1_000_000.0 - (4.0 - std::f64::consts::PI) * 10_000.0)).abs() < 1.0,
            "{area}"
        );
    }

    #[test]
    fn hints_previews_and_state() {
        for t in [
            Tool::Rotate,
            Tool::Mirror,
            Tool::Scale,
            Tool::Offset,
            Tool::Fillet,
            Tool::Chamfer,
            Tool::Trim,
            Tool::Extend,
            Tool::Split,
        ] {
            assert!(is_modify(t));
            for step in 0..3 {
                assert!(!hint(t, step).is_empty());
            }
        }
        assert!(!is_modify(Tool::Line));
        let mut f = Fx::new(Tool::Rotate);
        let c = f.add(Shape::Circle {
            center: v(1.0, 1.0),
            radius: 1.0,
        });
        f.selection = vec![c];
        assert_eq!(f.m.modify_preview(v(1.0, 1.0)), Preview::None);
        f.run(v(0.0, 0.0));
        f.run(v(10.0, 0.0));
        let Preview::Path { vertices, .. } = f.m.modify_preview(v(0.0, 10.0)) else {
            panic!()
        };
        assert_eq!(vertices.len(), 3);
        assert!(f.m.modify_readout(v(0.0, 10.0)).starts_with("Turn 90"));
        assert_eq!(ModifySettings::default().cut_to, CutTo::AllObjects);
    }
}
