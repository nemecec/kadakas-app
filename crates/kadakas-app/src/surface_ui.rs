//! Object Info for surface models, groups of them and component instances (MOD-13): the counts of
//! vertices, edges and faces, whether the model is a closed solid with its volume and area, the
//! box, the materials, and an action that opens the drawing in the 3D modeler. The numbers come
//! from `kadakas_core::surface::info`, the same that the agent API tells. The geometry is edited
//! in the modeler, so the panel only shows it.

use egui::Ui;
use kadakas_core::model::{Document, Object, ObjectId, Shape};
use kadakas_core::surface::info::{self, SolidState, SurfaceInfo};
use kadakas_l10n::tr;
use kadakas_units::Units;

/// What the panel keeps between frames.
#[derive(Default)]
pub struct State {
    /// The numbers of the last object, for the revision of the document they were made at.
    cached: Option<Cached>,
    /// The big model that the person asked to check.
    checked: Option<ObjectId>,
    /// The person pressed "Open in the 3D modeler". The app does it after the frame.
    pub open_in_modeler: bool,
    /// Whether the modeler is installed, looked at once.
    pub modeler: Option<bool>,
}

struct Cached {
    id: ObjectId,
    revision: u64,
    checked: bool,
    info: SurfaceInfo,
}

/// One line of the panel: a label and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub value: String,
}

fn row(label: String, value: String) -> Row {
    Row { label, value }
}

fn xyz(units: &Units, p: [f64; 3]) -> String {
    format!(
        "{}, {}, {}",
        units.length_text(p[0]),
        units.length_text(p[1]),
        units.length_text(p[2])
    )
}

/// The name of what the object is, for the heading.
pub fn kind_name(shape: &Shape) -> String {
    match shape {
        Shape::Component { .. } => tr!("surface-ui-kind-component"),
        Shape::Group { .. } => tr!("surface-ui-kind-group"),
        _ => tr!("surface-ui-kind-surface-model"),
    }
}

/// The lines of the panel for the numbers of an object, in the units of the person.
pub fn rows(info: &SurfaceInfo, units: &Units, plan: Option<&str>) -> Vec<Row> {
    let mut out = Vec::new();
    // The plan shows less than the model has: say so, where the person looks at the numbers.
    if let Some(text) = plan {
        out.push(row(tr!("surface-ui-plan"), text.to_string()));
    }
    if info.models > 1 {
        out.push(row(tr!("surface-ui-models"), info.models.to_string()));
    }
    out.push(row(tr!("panels-vertices"), info.vertices.to_string()));
    out.push(row(tr!("surface-ui-edges"), info.edges.to_string()));
    out.push(row(tr!("surface-ui-faces"), info.faces.to_string()));
    out.push(row(
        tr!("surface-ui-solid"),
        match &info.solid {
            SolidState::Empty => tr!("surface-ui-solid-empty"),
            SolidState::Unchecked => tr!("surface-ui-solid-unchecked"),
            SolidState::Closed { solids, .. } => tr!("surface-ui-solid-closed", count = *solids),
            SolidState::Open { open_edges } => tr!("surface-ui-solid-open", count = *open_edges),
            SolidState::Crossing { crossings } => {
                tr!("surface-ui-solid-crossing", count = *crossings)
            }
            SolidState::Inward => tr!("surface-ui-solid-inward"),
            SolidState::Flat => tr!("surface-ui-solid-flat"),
        },
    ));
    if let SolidState::Closed { volume, .. } = &info.solid {
        out.push(row(tr!("surface-ui-volume"), units.volume_text(*volume)));
    }
    if info.faces > 0 {
        out.push(row(tr!("surface-ui-area"), units.area_text(info.area)));
    }
    if let Some((lo, hi)) = info.bounds {
        out.push(row(
            tr!("surface-ui-size"),
            format!(
                "{} x {} x {}",
                units.length_text(hi[0] - lo[0]),
                units.length_text(hi[1] - lo[1]),
                units.length_text(hi[2] - lo[2])
            ),
        ));
        out.push(row(tr!("surface-ui-bounds-min"), xyz(units, lo)));
        out.push(row(tr!("surface-ui-bounds-max"), xyz(units, hi)));
    }
    if info.hidden_faces + info.hidden_edges > 0 {
        out.push(row(
            tr!("surface-ui-hidden"),
            tr!(
                "surface-ui-hidden-value",
                faces = info.hidden_faces,
                edges = info.hidden_edges
            ),
        ));
    }
    if info.soft_edges > 0 {
        out.push(row(tr!("surface-ui-soft"), info.soft_edges.to_string()));
    }
    for m in &info.materials {
        out.push(row(
            tr!("surface-ui-material"),
            tr!(
                "surface-ui-material-value",
                name = m.name.clone(),
                count = m.faces
            ),
        ));
    }
    if info.unpainted_faces > 0 {
        out.push(row(
            tr!("surface-ui-material"),
            tr!("surface-ui-no-material", count = info.unpainted_faces),
        ));
    }
    out
}

/// What the plan says when it draws a model as less than it is, or `None` for the full drawing.
/// The canvas and Object Info say the same thing.
pub fn simplified_text(d: &kadakas_core::surface::plan::PlanDrawing) -> Option<String> {
    use kadakas_core::component::grouped;
    use kadakas_core::surface::plan::Detail;
    match d.detail {
        Detail::Full => None,
        Detail::Outline if d.faces_total > d.limits.faces => Some(tr!(
            "surface-ui-note-outline-faces",
            count = grouped(d.faces_total),
            limit = grouped(d.limits.faces)
        )),
        Detail::Outline => Some(tr!(
            "surface-ui-note-outline-edges",
            count = grouped(d.edges_total),
            limit = grouped(d.limits.edges)
        )),
        Detail::Box => Some(tr!(
            "surface-ui-note-box",
            limit = grouped(d.limits.outline)
        )),
    }
}

/// What the plan says about an object: the first member (the object itself, or the models of a
/// group) that is drawn as less than it is.
pub fn plan_text(doc: &Document, obj: &Object) -> Option<String> {
    let ids = match obj.shape {
        Shape::Group { .. } => doc.leaves(&[obj.id]),
        _ => vec![obj.id],
    };
    ids.into_iter().find_map(|id| {
        let o = doc.object(id)?;
        let b = crate::surface_draw::built(doc, o, true)?;
        simplified_text(b.drawing()?)
    })
}

/// True for the objects that have this panel.
pub fn applies(obj: &Object) -> bool {
    matches!(
        obj.shape,
        Shape::SurfaceModel(_) | Shape::Component { .. } | Shape::Group { .. }
    )
}

/// The numbers of an object for the panel, kept for the revision of the document.
fn numbers(doc: &Document, revision: u64, obj: &Object, st: &mut State) -> Option<SurfaceInfo> {
    let checked = st.checked == Some(obj.id);
    if let Some(c) = &st.cached
        && c.id == obj.id
        && c.revision == revision
        && c.checked == checked
    {
        return Some(c.info.clone());
    }
    let check = if checked {
        info::Check::Always
    } else {
        info::Check::Auto
    };
    let info = info::of_object(doc, obj, check)?;
    st.cached = Some(Cached {
        id: obj.id,
        revision,
        checked,
        info: info.clone(),
    });
    Some(info)
}

/// True when the 3D modeler is installed next to this program.
pub fn modeler_available() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::current_exe()
            .is_ok_and(|exe| kadakas_update::launch::sibling_exists(&exe, MODELER))
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

/// The program of the 3D modeler.
#[cfg(not(target_arch = "wasm32"))]
pub const MODELER: &str = "kadakas-modeler";

/// The block of Object Info for a surface model, a group of them or an instance. Nothing is
/// edited here. `with_modeler` shows the action that opens the drawing in the modeler.
pub fn object_info(
    ui: &mut Ui,
    doc: &Document,
    revision: u64,
    obj: &Object,
    st: &mut State,
    with_modeler: bool,
) {
    let Some(info) = numbers(doc, revision, obj, st) else {
        return;
    };
    ui.label(kind_name(&obj.shape));
    if let Shape::SurfaceModel(m) = &obj.shape
        && !m.name.is_empty()
    {
        ui.horizontal(|ui| {
            ui.label(tr!("panels-name"));
            ui.label(&m.name);
        });
    }
    let units = kadakas_units::current();
    egui::Grid::new(("surface-info", obj.id.0.as_u128()))
        .num_columns(2)
        .show(ui, |ui| {
            for r in rows(&info, &units, plan_text(doc, obj).as_deref()) {
                ui.label(r.label);
                ui.label(r.value);
                ui.end_row();
            }
        });
    if info.solid == SolidState::Unchecked && ui.button(tr!("surface-ui-check")).clicked() {
        st.checked = Some(obj.id);
    }
    ui.weak(tr!("surface-ui-edited-in-modeler"));
    if with_modeler && ui.button(tr!("surface-ui-open-in-modeler")).clicked() {
        st.open_in_modeler = true;
    }
}

/// Opens the drawing in the 3D modeler and says what happened, for the status line. The modeler
/// opens the saved file, so a drawing that was never saved, or has changes that are not saved,
/// waits for a save. `root` is the folder of the launcher and `own` the id of this run.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_in_modeler(
    path: Option<&std::path::Path>,
    unsaved: bool,
    exe: &std::path::Path,
    root: Option<&std::path::Path>,
    own: &str,
) -> String {
    use kadakas_update::launch::{Sent, SiblingError, open_in_sibling};
    let Some(path) = path else {
        return tr!("surface-ui-save-first");
    };
    // The modeler is another process, with its own working folder: it needs the whole path.
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let path = absolute.as_path();
    if unsaved {
        return tr!("surface-ui-save-changes-first");
    }
    match open_in_sibling(root, own, exe, MODELER, path) {
        Ok(Sent::Spawned) => tr!("surface-ui-opening-modeler"),
        Ok(Sent::Asked) => tr!("surface-ui-sent-to-modeler"),
        Err(SiblingError::Missing) => tr!("surface-ui-modeler-missing"),
        Err(SiblingError::Failed(e)) => tr!("surface-ui-modeler-failed", error = e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::Attributes;
    use kadakas_core::surface::{Edit, SurfaceModel};

    fn box_model() -> SurfaceModel {
        let mut m = SurfaceModel::new();
        m.name = "Shed".into();
        let p = [
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ];
        m.apply(&Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        })
        .unwrap();
        let face = m.faces().next().unwrap().0;
        m.apply(&Edit::PushPull {
            face,
            distance: 500.0,
        })
        .unwrap();
        m
    }

    fn add(ed: &mut Editor, m: SurfaceModel) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::new(m)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    fn value(rows: &[Row], label: &str) -> String {
        rows.iter()
            .find(|r| r.label == label)
            .map(|r| r.value.clone())
            .unwrap_or_else(|| panic!("no row {label}: {rows:?}"))
    }

    #[test]
    fn the_rows_of_a_closed_box_have_the_counts_the_solid_state_and_the_numbers() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let doc = ed.doc();
        let info = info::of_object(doc, doc.object(id).unwrap(), info::Check::Auto).unwrap();
        let units = kadakas_units::current();
        let rows = rows(&info, &units, None);
        assert_eq!(value(&rows, "Vertices"), "8");
        assert_eq!(value(&rows, "Edges"), "12");
        assert_eq!(value(&rows, "Faces"), "6");
        assert_eq!(value(&rows, "Solid"), "Closed solid");
        // 1 m x 1 m x 0.5 m.
        assert_eq!(value(&rows, "Volume"), units.volume_text(5.0e8));
        assert!(value(&rows, "Volume").starts_with("0.50"), "{rows:?}");
        assert_eq!(value(&rows, "Area"), units.area_text(4.0e6));
        assert_eq!(
            value(&rows, "Size"),
            format!(
                "{} x {} x {}",
                units.length_text(1000.0),
                units.length_text(1000.0),
                units.length_text(500.0)
            )
        );
        assert_eq!(
            value(&rows, "Lowest corner"),
            format!("{0}, {0}, {0}", units.length_text(0.0))
        );
        assert_eq!(value(&rows, "Material"), "None (6 faces)");
        assert!(
            !rows
                .iter()
                .any(|r| r.label == "Hidden" || r.label == "Soft edges")
        );
    }

    #[test]
    fn an_open_surface_a_hidden_part_and_a_material_show_in_the_rows() {
        let mut ed = Editor::default();
        let mut m = box_model();
        let top = m
            .faces()
            .find(|(_, f)| f.plane.normal.z > 0.9)
            .map(|(f, _)| f)
            .unwrap();
        m.apply(&Edit::EraseFaces { faces: vec![top] }).unwrap();
        let wall = m
            .faces()
            .find(|(_, f)| f.plane.normal.x > 0.9)
            .map(|(f, _)| f)
            .unwrap();
        m.apply(&Edit::SetHidden {
            faces: vec![wall],
            edges: vec![],
            hidden: true,
        })
        .unwrap();
        let id = add(&mut ed, m);
        let doc = ed.doc();
        let info = info::of_object(doc, doc.object(id).unwrap(), info::Check::Auto).unwrap();
        let rows = rows(&info, &kadakas_units::current(), None);
        assert_eq!(value(&rows, "Faces"), "5");
        assert_eq!(value(&rows, "Solid"), "Not closed (4 open edges)");
        assert!(rows.iter().all(|r| r.label != "Volume"));
        assert_eq!(value(&rows, "Hidden"), "1 face, 0 edges");
    }

    #[test]
    fn a_big_model_waits_for_the_check_and_the_button_asks_for_it() {
        // 101 x 101 squares cut into triangles: over the size of the automatic check.
        let mut vertices = std::collections::BTreeMap::new();
        let n = 101usize;
        let id = |x: usize, y: usize| kadakas_core::surface::VertexId((y * (n + 1) + x) as u32 + 1);
        for y in 0..=n {
            for x in 0..=n {
                vertices.insert(
                    id(x, y),
                    glam::DVec3::new(x as f64 * 10.0, y as f64 * 10.0, 0.0),
                );
            }
        }
        let mut edges = std::collections::BTreeMap::new();
        let mut faces = std::collections::BTreeMap::new();
        let mut seen = std::collections::HashSet::new();
        let (mut e, mut f) = (1u32, 1u32);
        let up = kadakas_core::surface::Plane::new(glam::DVec3::ZERO, glam::DVec3::Z).unwrap();
        for y in 0..n {
            for x in 0..n {
                let (a, b, c, d) = (id(x, y), id(x + 1, y), id(x + 1, y + 1), id(x, y + 1));
                for tri in [[a, b, c], [a, c, d]] {
                    for k in 0..3 {
                        let (p, q) = (tri[k], tri[(k + 1) % 3]);
                        let key = if p <= q { (p, q) } else { (q, p) };
                        if seen.insert(key) {
                            edges.insert(
                                kadakas_core::surface::EdgeId(100_000 + e),
                                kadakas_core::surface::Edge { v: [p, q] },
                            );
                            e += 1;
                        }
                    }
                    faces.insert(
                        kadakas_core::surface::FaceId(1_000_000 + f),
                        kadakas_core::surface::Face::new(up, tri.to_vec(), vec![]),
                    );
                    f += 1;
                }
            }
        }
        let mut ed = Editor::default();
        let oid = add(
            &mut ed,
            SurfaceModel::from_parts(vertices, edges, faces, 2_000_000),
        );
        let doc = ed.doc();
        let obj = doc.object(oid).unwrap();
        let mut st = State::default();
        let first = numbers(doc, 1, obj, &mut st).unwrap();
        assert_eq!(first.solid, SolidState::Unchecked);
        assert_eq!(
            value(&rows(&first, &kadakas_units::current(), None), "Solid"),
            "Not checked (large model)"
        );
        // The same numbers come back for the same revision.
        assert_eq!(numbers(doc, 1, obj, &mut st).unwrap(), first);
        // The person asks: the next frame has the answer.
        st.checked = Some(oid);
        let second = numbers(doc, 1, obj, &mut st).unwrap();
        assert!(matches!(second.solid, SolidState::Open { .. }));
    }

    #[test]
    fn the_panel_draws_the_numbers_and_the_open_in_modeler_button_asks_for_the_modeler() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let ctx = egui::Context::default();
        let mut st = State::default();
        let mut texts: Vec<String> = Vec::new();
        for with_modeler in [false, true] {
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let doc = ed.doc();
                object_info(ui, doc, 1, doc.object(id).unwrap(), &mut st, with_modeler);
            });
            texts = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect();
            assert!(texts.iter().any(|t| t == "Surface model"), "{texts:?}");
            assert!(texts.iter().any(|t| t == "Shed"));
            assert!(texts.iter().any(|t| t == "Closed solid"));
            assert!(
                texts
                    .iter()
                    .any(|t| t == "The geometry is edited in the 3D modeler."),
                "{texts:?}"
            );
            assert_eq!(
                texts.iter().any(|t| t == "Open in the 3D Modeler"),
                with_modeler
            );
        }
        assert!(!texts.is_empty());
    }

    #[test]
    fn a_group_and_an_instance_have_the_panel_too() {
        let mut ed = Editor::default();
        let a = add(&mut ed, box_model());
        let b = add(&mut ed, box_model());
        let g = ObjectId(
            ed.execute(&Command::Group {
                ids: vec![a, b],
                name: None,
            })
            .unwrap()
            .created[0],
        );
        let doc = ed.doc();
        assert!(applies(doc.object(g).unwrap()));
        let i = info::of_object(doc, doc.object(g).unwrap(), info::Check::Auto).unwrap();
        assert_eq!((i.models, i.faces), (2, 12));
        assert_eq!(
            kind_name(&doc.object(g).unwrap().shape),
            "Group of surface models"
        );
        let out = ed
            .execute(&Command::MakeComponent {
                name: "Block".into(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![a],
                replace: true,
            })
            .unwrap();
        let inst = ObjectId(out.created[1]);
        let doc = ed.doc();
        let i = info::of_object(doc, doc.object(inst).unwrap(), info::Check::Auto).unwrap();
        assert_eq!((i.models, i.faces, i.vertices), (1, 6, 8));
        assert_eq!(
            kind_name(&doc.object(inst).unwrap().shape),
            "Component instance"
        );
        let rows = rows(&i, &kadakas_units::current(), None);
        assert_eq!(value(&rows, "Solid"), "Closed solid");
        // Other objects have no such panel.
        let circle = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: glam::DVec2::ZERO,
                    radius: 10.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        assert!(!applies(ed.doc().object(circle).unwrap()));
        assert!(
            info::of_object(
                ed.doc(),
                ed.doc().object(circle).unwrap(),
                info::Check::Auto
            )
            .is_none()
        );
    }

    #[test]
    fn a_model_turned_in_and_a_model_drawn_as_less_than_it_is_say_so_in_the_rows() {
        let mut ed = Editor::default();
        let mut m = box_model();
        let all: Vec<kadakas_core::surface::FaceId> = m.faces().map(|(f, _)| f).collect();
        m.apply(&Edit::ReverseFaces { faces: all }).unwrap();
        let id = add(&mut ed, m);
        let doc = ed.doc();
        let info = info::of_object(doc, doc.object(id).unwrap(), info::Check::Auto).unwrap();
        let rows = rows(&info, &kadakas_units::current(), None);
        assert_eq!(value(&rows, "Solid"), "Closed, but the faces look inward");
        assert!(
            rows.iter()
                .all(|r| r.label != "Plan" && r.label != "Volume")
        );
        // A model that the plan draws as an outline says it in a row, the same words as the canvas.
        let big = crate::surface_draw::tests_support::big_model();
        let big = add(&mut ed, big);
        let doc = ed.doc();
        crate::area_ui::set_revision(None);
        let text = plan_text(doc, doc.object(big).unwrap()).expect("a note");
        assert_eq!(text, "Drawn as outline: 20,402 faces, limit 20,000");
        let info = info::of_object(doc, doc.object(big).unwrap(), info::Check::Never).unwrap();
        let rows = rows_with(&info, Some(&text));
        assert_eq!(value(&rows, "Plan"), text);
        assert_eq!(plan_text(doc, doc.object(id).unwrap()), None);
        // The note of a group is the note of its first member that is simplified.
        let g = ObjectId(
            ed.execute(&Command::Group {
                ids: vec![id, big],
                name: None,
            })
            .unwrap()
            .created[0],
        );
        let doc = ed.doc();
        assert_eq!(
            plan_text(doc, doc.object(g).unwrap()).as_deref(),
            Some(text.as_str())
        );
    }

    fn rows_with(info: &SurfaceInfo, plan: Option<&str>) -> Vec<Row> {
        rows(info, &kadakas_units::current(), plan)
    }

    #[test]
    fn the_texts_of_the_notes_are_in_estonian_too() {
        let d = |detail, faces, edges| kadakas_core::surface::plan::PlanDrawing {
            items: Vec::new(),
            detail,
            faces_total: faces,
            edges_total: edges,
            limits: kadakas_core::surface::plan::Limits::DEFAULT,
        };
        use kadakas_core::surface::plan::Detail;
        let said = |lang: &str| {
            kadakas_l10n::with_language(lang, || {
                [
                    simplified_text(&d(Detail::Outline, 34_000, 100)),
                    simplified_text(&d(Detail::Outline, 100, 40_000)),
                    simplified_text(&d(Detail::Box, 100, 100)),
                    simplified_text(&d(Detail::Full, 100, 100)),
                ]
            })
        };
        let en = said("en-US");
        assert_eq!(
            en[0].as_deref(),
            Some("Drawn as outline: 34,000 faces, limit 20,000")
        );
        assert_eq!(
            en[1].as_deref(),
            Some("Drawn as outline: 40,000 edges, limit 30,000")
        );
        assert_eq!(
            en[2].as_deref(),
            Some("Drawn as box: the outline has more than 20,000 lines")
        );
        assert_eq!(en[3], None);
        let et = said("et");
        assert_eq!(
            et[0].as_deref(),
            Some("Joonistatud kontuurina: 34,000 tahku, piir 20,000")
        );
        assert!(et[1].as_deref().unwrap().contains("serva"));
        assert!(et[2].as_deref().unwrap().starts_with("Joonistatud karbina"));
    }

    /// Waits for something that another process does. The bound is only a guard against a
    /// process that never does it.
    #[cfg(unix)]
    fn wait_until(mut done: impl FnMut() -> bool) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !done() {
            assert!(std::time::Instant::now() < end, "the modeler did not run");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Starts the modeler. Linux refuses to run a file while a thread of this process (another
    /// test that starts a program) still holds it open for writing, the error "text file busy".
    /// That passes as soon as the other thread has closed its copy, so the start is tried again.
    #[cfg(unix)]
    fn open_busy_safe(path: Option<&std::path::Path>, exe: &std::path::Path) -> String {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let said = open_in_modeler(path, false, exe, None, "");
            if !said.contains("busy") || std::time::Instant::now() > end {
                return said;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn open_in_modeler_says_what_it_needs_and_what_it_did() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kadakas-modeler-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("kadakas-app");
        let file = dir.join("a.kad");
        // The drawing was not saved, or has changes that are not saved.
        assert_eq!(
            open_in_modeler(None, false, &exe, None, ""),
            "Save the drawing first. The 3D modeler opens the saved file."
        );
        assert_eq!(
            open_in_modeler(Some(&file), true, &exe, None, ""),
            "Save your changes first. The 3D modeler opens the saved file."
        );
        // No modeler next to the program.
        assert_eq!(
            open_in_modeler(Some(&file), false, &exe, None, ""),
            "The 3D modeler is not installed."
        );
        // A modeler that starts.
        let modeler = dir.join(MODELER);
        // The script writes a side file and renames it, so a reader never sees a half-written file.
        std::fs::write(
            &modeler,
            "#!/bin/sh\necho \"$2\" > opened.tmp && mv opened.tmp opened.txt\n",
        )
        .unwrap();
        std::fs::set_permissions(&modeler, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            open_busy_safe(Some(&file), &exe),
            "Opening the drawing in the 3D modeler."
        );
        let opened = dir.join("opened.txt");
        wait_until(|| opened.exists());
        assert_eq!(
            std::fs::read_to_string(&opened).unwrap().trim(),
            file.to_string_lossy()
        );
        // A relative path is made whole first: the modeler has a working folder of its own.
        let rel = std::path::Path::new("relative-drawing.kad");
        assert_eq!(
            open_busy_safe(Some(rel), &exe),
            "Opening the drawing in the 3D modeler."
        );
        let want = std::env::current_dir().unwrap().join(rel);
        wait_until(|| {
            std::fs::read_to_string(&opened).is_ok_and(|t| t.trim() == want.to_string_lossy())
        });
        assert_eq!(
            std::fs::read_to_string(&opened).unwrap().trim(),
            want.to_string_lossy()
        );
        // A modeler that cannot run.
        std::fs::write(&modeler, "not a program").unwrap();
        std::fs::set_permissions(&modeler, std::fs::Permissions::from_mode(0o644)).unwrap();
        let said = open_in_modeler(Some(&file), false, &exe, None, "");
        assert!(said.starts_with("The 3D modeler did not start: "), "{said}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
