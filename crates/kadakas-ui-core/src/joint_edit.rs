//! The form logic of the joint pattern editor of a hardscape style (HRD-08, PAT-05). A UI program
//! opens a [`JointEditor`] on a hardscape style, lets the person pick a bond or a module (a
//! preset with its numbers, or the closed shapes of the selection), shows a small preview, and
//! saves with one `ModifyResource` command, which is one undo step. The crate has no egui.

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::hardscape::HardscapeStyle;
use kadakas_core::hardscape::style::{JointKind, JointPattern};
use kadakas_core::layout::module::{self, PRESETS};
use kadakas_core::layout::object::outline_of;
use kadakas_core::layout::{Limits, ModuleUnit, Placement, Region};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_l10n::tr;

/// Where the module of the draft comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum ModuleSource {
    /// A preset. The numbers are in `JointEditor::numbers`.
    Preset(&'static str),
    /// The shapes that were taken from the drawing.
    Shapes,
}

/// A unit of the preview: its outline and its unit type (an index).
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewUnit {
    pub points: Vec<DVec2>,
    pub kind: usize,
    pub whole: bool,
}

/// The draft of the joint pattern of a hardscape style.
#[derive(Clone, Debug, PartialEq)]
pub struct JointEditor {
    id: ResourceId,
    saved: Option<JointPattern>,
    /// The pattern being edited. For a module, the shapes and the repeat vectors are in here.
    pub draft: JointPattern,
    /// The source of the module, when the kind is a module.
    pub source: Option<ModuleSource>,
    /// The numbers of the preset: side or across flats, length, width, notch (mm).
    pub numbers: [f64; 4],
    /// Use the distance across the flats for a hexagon, instead of the side.
    pub across_flats: bool,
}

/// What can go wrong with the draft.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem(pub String);

impl JointEditor {
    /// Opens the editor on a hardscape style. `None` if the resource is not one.
    pub fn open(doc: &Document, id: ResourceId) -> Option<JointEditor> {
        let style = doc.resources.hardscape_style(id)?;
        let saved = style.joints.clone();
        let draft = saved.clone().unwrap_or_default();
        let source = (draft.kind == JointKind::Module).then_some(ModuleSource::Shapes);
        Some(JointEditor {
            id,
            saved,
            draft,
            source,
            numbers: [100.0, 200.0, 165.0, 25.0],
            across_flats: false,
        })
    }

    pub fn id(&self) -> ResourceId {
        self.id
    }

    /// True if the draft differs from what the style has.
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.draft)
    }

    /// Takes one of the four bonds.
    pub fn set_bond(&mut self, kind: JointKind) {
        self.draft.kind = kind;
        if kind != JointKind::Module {
            self.source = None;
            self.draft.units.clear();
            self.draft.a = None;
            self.draft.b = None;
        }
    }

    /// Takes a preset with its default numbers.
    pub fn choose_preset(&mut self, name: &'static str) -> Result<(), Problem> {
        self.numbers = match name {
            "hexagon" => [100.0, self.numbers[1], self.numbers[2], self.numbers[3]],
            "octagon_square" => [100.0, self.numbers[1], self.numbers[2], self.numbers[3]],
            _ => [self.numbers[0], 200.0, 165.0, 25.0],
        };
        self.source = Some(ModuleSource::Preset(name));
        self.rebuild_preset()
    }

    /// Builds the module again from the numbers of the preset.
    pub fn rebuild_preset(&mut self) -> Result<(), Problem> {
        let Some(ModuleSource::Preset(name)) = self.source.clone() else {
            return Ok(());
        };
        let n = self.numbers;
        let across = self.across_flats;
        let num = move |k: &str| match k {
            "side" if !across => Some(n[0]),
            "across_flats" if across => Some(n[0]),
            "length" => Some(n[1]),
            "width" => Some(n[2]),
            "notch" => Some(n[3]),
            _ => None,
        };
        let m = module::preset(name, &num).map_err(Problem)?;
        self.set_module(m);
        Ok(())
    }

    fn set_module(&mut self, m: kadakas_core::layout::Module) {
        self.draft.kind = JointKind::Module;
        self.draft.units = m.units;
        self.draft.a = Some(m.a);
        self.draft.b = Some(m.b);
    }

    /// Takes the closed shapes with these IDs as the unit shapes. Without `a` and `b` the lattice
    /// is the box of the shapes (a stack). Returns the note about that.
    pub fn from_shapes(
        &mut self,
        doc: &Document,
        ids: &[ObjectId],
        a: Option<[f64; 2]>,
        b: Option<[f64; 2]>,
    ) -> Result<Option<String>, Problem> {
        let mut units = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let o = doc
                .object(*id)
                .ok_or_else(|| Problem(tr!("uic-joint-edit-object-gone", id = id)))?;
            let (outline, _) = outline_of(o).map_err(Problem)?;
            units.push(ModuleUnit {
                name: format!("unit {}", i + 1),
                outline,
            });
        }
        let (m, note) = module::from_outlines(units, a, b).map_err(Problem)?;
        m.compile(0.0).map_err(Problem)?;
        self.set_module(m);
        self.source = Some(ModuleSource::Shapes);
        Ok(note)
    }

    /// The joint pattern of the draft, checked.
    pub fn joints(&self) -> Result<JointPattern, Problem> {
        let mut j = self.draft.clone();
        if j.kind == JointKind::Module && j.units.is_empty() {
            return Err(Problem(tr!("uic-joint-edit-pick-preset")));
        }
        j.layout().validate().map_err(Problem)?;
        if j.kind != JointKind::Module && (j.unit_length < 10.0 || j.unit_width < 10.0) {
            return Err(Problem(tr!("uic-joint-edit-unit-size")));
        }
        if !j.angle.is_finite() {
            return Err(Problem(tr!("uic-joint-edit-angle")));
        }
        j.spacing = j.spacing.max(0.0);
        Ok(j)
    }

    /// The command that saves the draft: one `ModifyResource`. `Ok(None)` when nothing changed.
    pub fn save_command(&self, doc: &Document) -> Result<Option<Command>, Problem> {
        let joints = self.joints()?;
        if !self.is_dirty() {
            return Ok(None);
        }
        let mut style: HardscapeStyle = doc
            .resources
            .hardscape_style(self.id)
            .cloned()
            .ok_or_else(|| Problem(tr!("uic-joint-edit-style-gone")))?;
        style.joints = Some(joints);
        Ok(Some(Command::ModifyResource {
            id: self.id,
            name: None,
            body: Some(ResourceBody::HardscapeStyle(Box::new(style))),
        }))
    }

    /// Call after the save command worked.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.draft.clone());
    }

    /// A few units of the draft in a small rectangle, for the preview. The rectangle is about five
    /// repeats wide. Returns the size of the rectangle and the units.
    pub fn preview(&self) -> Result<(DVec2, Vec<PreviewUnit>), Problem> {
        let j = self.joints()?;
        let pattern = j.layout();
        let comp = pattern.compile().map_err(Problem)?;
        let size = (comp.a.abs() + comp.b.abs()) * 2.5;
        let size = size.max(DVec2::splat(20.0));
        let region = Region::from_rings(
            &[
                DVec2::ZERO,
                DVec2::new(size.x, 0.0),
                size,
                DVec2::new(0.0, size.y),
            ],
            &[],
        )
        .map_err(Problem)?;
        let place = Placement {
            origin: Some(DVec2::ZERO),
            angle: 0.0,
        };
        let mut out = Vec::new();
        kadakas_core::layout::for_each_unit(
            &region,
            &pattern,
            &place,
            &Limits::new(2000),
            &mut |u| {
                out.push(PreviewUnit {
                    points: u.rings[0].points.clone(),
                    kind: u.kind,
                    whole: u.whole,
                })
            },
        )
        .map_err(|e| Problem(e.to_string()))?;
        Ok((size, out))
    }
}

/// The names of the presets, for the picker.
pub fn preset_names() -> [&'static str; 3] {
    PRESETS
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Editor;
    use kadakas_core::curve::corners;
    use kadakas_core::model::{Attributes, Shape};

    fn doc_with_style() -> (Editor, ResourceId) {
        let mut ed = Editor::new(Document::with_sequential_ids());
        let id = ResourceId(
            ed.execute(&Command::AddResource {
                name: "P".into(),
                body: ResourceBody::HardscapeStyle(Box::default()),
            })
            .unwrap()
            .created[0],
        );
        (ed, id)
    }

    #[test]
    fn a_preset_makes_a_module_and_saves_as_one_step() {
        let (mut ed, id) = doc_with_style();
        let mut e = JointEditor::open(ed.doc(), id).unwrap();
        assert!(!e.is_dirty() || e.saved.is_none());
        for name in preset_names() {
            e.choose_preset(name).unwrap();
            assert_eq!(e.draft.kind, JointKind::Module);
            assert!(!e.draft.units.is_empty());
            let (size, units) = e.preview().unwrap();
            assert!(size.x > 0.0 && units.len() > 3, "{name}");
            assert!(units.iter().any(|u| u.whole));
        }
        e.choose_preset("hexagon").unwrap();
        e.across_flats = true;
        e.numbers[0] = 200.0;
        e.rebuild_preset().unwrap();
        e.draft.spacing = 4.0;
        let cmd = e.save_command(ed.doc()).unwrap().unwrap();
        ed.execute(&cmd).unwrap();
        let s = ed.doc().resources.hardscape_style(id).unwrap();
        let j = s.joints.as_ref().unwrap();
        assert_eq!(j.kind, JointKind::Module);
        assert_eq!(j.units[0].name, "hexagon");
        e.mark_saved();
        assert!(e.save_command(ed.doc()).unwrap().is_none());
        // A bad number is a problem, not a panic.
        e.numbers[0] = 1.0;
        assert!(e.rebuild_preset().is_err());
        // One undo.
        assert!(ed.undo());
        assert!(
            ed.doc()
                .resources
                .hardscape_style(id)
                .unwrap()
                .joints
                .is_none()
        );
    }

    #[test]
    fn bonds_and_shapes_from_the_selection() {
        let (mut ed, id) = doc_with_style();
        let mut e = JointEditor::open(ed.doc(), id).unwrap();
        e.set_bond(JointKind::Herringbone);
        e.draft.unit_length = 200.0;
        e.draft.unit_width = 100.0;
        let (_, units) = e.preview().unwrap();
        assert!(
            units
                .iter()
                .filter(|u| u.whole)
                .all(|u| u.points.len() == 4)
        );
        e.draft.unit_length = 5.0;
        assert!(e.joints().is_err());
        e.draft.unit_length = 200.0;
        // Two shapes from the drawing.
        fn add(ed: &mut Editor, pts: &[DVec2]) -> ObjectId {
            ObjectId(
                ed.execute(&Command::AddObject {
                    shape: Shape::Polyline {
                        points: corners(pts),
                        closed: true,
                    },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
            )
        }
        let a = add(
            &mut ed,
            &[
                DVec2::ZERO,
                DVec2::new(100.0, 0.0),
                DVec2::new(100.0, 100.0),
                DVec2::new(0.0, 100.0),
            ],
        );
        let b = add(
            &mut ed,
            &[
                DVec2::new(100.0, 0.0),
                DVec2::new(200.0, 0.0),
                DVec2::new(200.0, 100.0),
                DVec2::new(100.0, 100.0),
            ],
        );
        let note = e.from_shapes(ed.doc(), &[a, b], None, None).unwrap();
        assert!(note.unwrap().contains("stack"));
        assert_eq!(e.source, Some(ModuleSource::Shapes));
        assert_eq!(e.draft.units.len(), 2);
        assert!(e.preview().is_ok());
        // Shapes that overlap are refused with the reason.
        let c = add(
            &mut ed,
            &[
                DVec2::new(50.0, 0.0),
                DVec2::new(150.0, 0.0),
                DVec2::new(150.0, 100.0),
                DVec2::new(50.0, 100.0),
            ],
        );
        let err = e
            .from_shapes(ed.doc(), &[a, c], Some([200.0, 0.0]), Some([0.0, 200.0]))
            .unwrap_err();
        assert!(err.0.contains("overlap"), "{}", err.0);
        assert!(
            e.from_shapes(ed.doc(), &[ObjectId::from_num(999)], None, None)
                .is_err()
        );
        // Back to a bond clears the module.
        e.set_bond(JointKind::Stack);
        assert!(e.draft.units.is_empty() && e.source.is_none());
        // A module without shapes cannot be saved.
        e.set_bond(JointKind::Module);
        assert!(e.joints().is_err());
        assert!(JointEditor::open(ed.doc(), ResourceId::from_num(999)).is_none());
        assert_eq!(e.id(), id);
    }
}
