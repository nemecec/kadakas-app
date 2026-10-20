//! Property test for groups and components (MOD-07): random sequences of make group, make
//! component, explode, make unique, edit in place, copy, move, delete, undo and redo keep the
//! model valid, undo exactly, and do not change the area of the geometry when an operation only
//! regroups it.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::component;
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::surface::Edit;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    AddSquare,
    AddTwoSquares,
    GroupFace(u8, u8),
    GroupObjects(Vec<u8>),
    ComponentFace(u8, u8),
    ComponentObjects(Vec<u8>),
    Explode(u8),
    MakeUnique(u8),
    Copy(u8),
    OpenClose(u8),
    OpenDrawClose(u8),
    Move(u8),
    Delete(u8),
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::AddSquare),
        Just(Op::AddTwoSquares),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::GroupFace(a, b)),
        prop::collection::vec(any::<u8>(), 1..4).prop_map(Op::GroupObjects),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::ComponentFace(a, b)),
        prop::collection::vec(any::<u8>(), 1..4).prop_map(Op::ComponentObjects),
        any::<u8>().prop_map(Op::Explode),
        any::<u8>().prop_map(Op::MakeUnique),
        any::<u8>().prop_map(Op::Copy),
        any::<u8>().prop_map(Op::OpenClose),
        any::<u8>().prop_map(Op::OpenDrawClose),
        any::<u8>().prop_map(Op::Move),
        any::<u8>().prop_map(Op::Delete),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

fn square(x: f64, y: f64, w: f64) -> Edit {
    let p = [
        [x, y, 0.0],
        [x + w, y, 0.0],
        [x + w, y + w, 0.0],
        [x, y + w, 0.0],
    ];
    Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }
}

/// The area of all geometry in the drawing: models, and the models of the instances. (Every
/// transform here is a move, so the area of a model is the area in the drawing.)
fn world_area(doc: &Document) -> f64 {
    let mut total = 0.0;
    for o in doc.objects.values() {
        match &o.shape {
            Shape::SurfaceModel(m) => total += m.area(),
            Shape::Component { symbol, .. } => {
                total += component::leaves(&doc.resources, *symbol)
                    .expect("a definition within the limits")
                    .iter()
                    .map(|l| l.model.area())
                    .sum::<f64>();
            }
            _ => {}
        }
    }
    total
}

/// Everything that must hold after every operation.
fn check(doc: &Document) {
    let mut d = doc.clone();
    d.validate().expect("the document is valid");
    for o in doc.objects.values() {
        match &o.shape {
            Shape::SurfaceModel(m) => {
                assert!(m.problem().is_none(), "{:?}", m.problem());
                assert!(m.check().problems.is_empty(), "{:?}", m.check().problems);
            }
            Shape::Component { symbol, .. } => {
                let def = doc
                    .resources
                    .symbol(*symbol)
                    .expect("the definition exists");
                assert!(component::parts_problem(&def.parts).is_none());
                component::leaves(&doc.resources, *symbol).expect("within the limits");
            }
            _ => {}
        }
        if let Some(p) = o.parent {
            assert!(doc.is_group(p), "a parent is a group");
        }
    }
}

struct State {
    ed: Editor,
    next_x: f64,
    next_name: u32,
    next_move: f64,
}

impl State {
    fn new() -> Self {
        State {
            ed: Editor::with_sequential_ids(),
            next_x: 0.0,
            next_name: 0,
            next_move: 1.0e6,
        }
    }

    fn pick(&self, k: u8, f: impl Fn(&Shape) -> bool) -> Option<ObjectId> {
        let all: Vec<ObjectId> = self
            .ed
            .doc()
            .objects
            .values()
            .filter(|o| f(&o.shape))
            .map(|o| o.id)
            .collect();
        (!all.is_empty()).then(|| all[k as usize % all.len()])
    }

    fn pick_many(&self, ks: &[u8]) -> Vec<ObjectId> {
        let all: Vec<ObjectId> = self.ed.doc().objects.keys().copied().collect();
        if all.is_empty() {
            return vec![];
        }
        let first = all[ks[0] as usize % all.len()];
        let parent = self.ed.doc().object(first).unwrap().parent;
        let mut out = vec![first];
        for k in &ks[1..] {
            let id = all[*k as usize % all.len()];
            if self.ed.doc().object(id).unwrap().parent == parent && !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    fn add_model(&mut self, edits: Vec<Edit>) {
        let out = self
            .ed
            .execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::default()),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let id = ObjectId(out.created[0]);
        for e in edits {
            self.ed
                .execute(&Command::EditSurface { id, edit: e })
                .unwrap();
        }
    }

    /// The command of an operation and whether it only regroups the geometry.
    fn command(&mut self, op: &Op) -> Option<(Command, bool)> {
        let any_model = |s: &Shape| matches!(s, Shape::SurfaceModel(m) if m.face_count() > 0);
        match op {
            Op::GroupFace(a, f) | Op::ComponentFace(a, f) => {
                let m = self.pick(*a, any_model)?;
                let Shape::SurfaceModel(sm) = &self.ed.doc().object(m).unwrap().shape else {
                    return None;
                };
                let faces: Vec<_> = sm.faces().map(|(id, _)| id).collect();
                let face = faces[*f as usize % faces.len()];
                self.next_name += 1;
                Some((
                    if matches!(op, Op::GroupFace(..)) {
                        Command::MakeGroup {
                            model: Some(m),
                            faces: vec![face],
                            edges: vec![],
                            objects: vec![],
                            name: None,
                        }
                    } else {
                        Command::MakeComponent {
                            name: format!("C{}", self.next_name),
                            model: Some(m),
                            faces: vec![face],
                            edges: vec![],
                            objects: vec![],
                            replace: true,
                        }
                    },
                    true,
                ))
            }
            Op::GroupObjects(ks) | Op::ComponentObjects(ks) => {
                let objects = self.pick_many(ks);
                if objects.is_empty() {
                    return None;
                }
                self.next_name += 1;
                Some((
                    if matches!(op, Op::GroupObjects(_)) {
                        Command::MakeGroup {
                            model: None,
                            faces: vec![],
                            edges: vec![],
                            objects,
                            name: None,
                        }
                    } else {
                        Command::MakeComponent {
                            name: format!("C{}", self.next_name),
                            model: None,
                            faces: vec![],
                            edges: vec![],
                            objects,
                            replace: true,
                        }
                    },
                    true,
                ))
            }
            Op::Explode(k) => {
                let id = self.pick(*k, |s| {
                    matches!(s, Shape::Group { .. } | Shape::Component { .. })
                })?;
                Some((Command::Explode { ids: vec![id] }, true))
            }
            Op::MakeUnique(k) => {
                let id = self.pick(*k, |s| matches!(s, Shape::Component { .. }))?;
                Some((Command::MakeUnique { ids: vec![id] }, true))
            }
            Op::Copy(k) => {
                let id = self.pick(*k, |s| matches!(s, Shape::Component { .. }))?;
                self.next_move += 1.0e6;
                Some((
                    Command::CopyObjects {
                        ids: vec![id],
                        delta: DVec2::new(self.next_move, 0.0),
                    },
                    false,
                ))
            }
            Op::Move(k) => {
                let id = self.pick(*k, |s| {
                    matches!(
                        s,
                        Shape::Group { .. } | Shape::Component { .. } | Shape::SurfaceModel(_)
                    )
                })?;
                self.next_move += 1.0e6;
                Some((
                    Command::MoveObjects {
                        ids: vec![id],
                        delta: DVec2::new(self.next_move, 0.0),
                    },
                    true,
                ))
            }
            Op::Delete(k) => {
                let id = self.pick(*k, |_| true)?;
                Some((Command::DeleteObjects { ids: vec![id] }, false))
            }
            _ => None,
        }
    }

    fn run(&mut self, op: &Op) {
        let before = self.ed.doc().clone();
        let area = world_area(&before);
        match op {
            Op::AddSquare | Op::AddTwoSquares => {
                let x = self.next_x;
                self.next_x += 5000.0;
                let mut e = vec![square(x, 0.0, 1000.0)];
                if matches!(op, Op::AddTwoSquares) {
                    e.push(square(x + 1000.0, 0.0, 1000.0));
                }
                self.add_model(e);
            }
            Op::Undo => {
                self.ed.undo();
            }
            Op::Redo => {
                self.ed.redo();
            }
            Op::OpenClose(k) | Op::OpenDrawClose(k) => {
                let Some(inst) = self.pick(*k, |s| matches!(s, Shape::Component { .. })) else {
                    return;
                };
                let Ok(out) = self.ed.execute(&Command::EditComponent { id: inst }) else {
                    return;
                };
                let eg = ObjectId(out.created[0]);
                let mut drew = false;
                if matches!(op, Op::OpenDrawClose(_)) {
                    let models: Vec<ObjectId> = self
                        .ed
                        .doc()
                        .children(eg)
                        .into_iter()
                        .filter(|c| {
                            matches!(
                                self.ed.doc().object(*c).map(|o| &o.shape),
                                Some(Shape::SurfaceModel(_))
                            )
                        })
                        .collect();
                    if let Some(m) = models.first() {
                        self.next_move += 1.0e6;
                        // Far away from everything, in the drawing; the model converts it.
                        let Shape::SurfaceModel(sm) = &self.ed.doc().object(*m).unwrap().shape
                        else {
                            unreachable!()
                        };
                        let inv = sm.transform.inverse();
                        let at =
                            inv.transform_point3(glam::DVec3::new(self.next_move, 50_000.0, 0.0));
                        self.ed
                            .execute(&Command::EditSurface {
                                id: *m,
                                edit: square(at.x, at.y, 1000.0),
                            })
                            .unwrap();
                        drew = true;
                    }
                }
                let closed = self.ed.execute(&Command::CloseComponent { group: eg });
                assert!(closed.is_ok(), "{closed:?}");
                if !drew {
                    assert!((world_area(self.ed.doc()) - area).abs() < 1e-3);
                }
            }
            _ => {
                let Some((cmd, neutral)) = self.command(op) else {
                    return;
                };
                let depth = self.ed.undo_depth();
                match self.ed.execute(&cmd) {
                    Ok(_) => {
                        assert_eq!(self.ed.undo_depth(), depth + 1, "one undo step");
                        let after = self.ed.doc().clone();
                        if neutral {
                            let a = world_area(&after);
                            assert!((a - area).abs() < 1e-3, "{cmd:?}: area {area} -> {a}");
                        }
                        assert!(self.ed.undo());
                        assert_eq!(self.ed.doc(), &before, "undo of {cmd:?}");
                        assert!(self.ed.redo());
                        assert_eq!(self.ed.doc(), &after, "redo of {cmd:?}");
                    }
                    Err(_) => assert_eq!(self.ed.doc(), &before, "an error changes nothing"),
                }
            }
        }
        check(self.ed.doc());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn random_groups_and_components_stay_valid_and_undo_exactly(ops in prop::collection::vec(op(), 1..60)) {
        let mut s = State::new();
        // A start with some geometry, so that most operations have something to work on.
        for _ in 0..3 {
            s.run(&Op::AddTwoSquares);
        }
        for op in &ops {
            s.run(op);
        }
        // Everything undoes to the empty document.
        while s.ed.undo() {}
        check(s.ed.doc());
        prop_assert!(s.ed.doc().objects.is_empty());
        while s.ed.redo() {}
        check(s.ed.doc());
    }
}
