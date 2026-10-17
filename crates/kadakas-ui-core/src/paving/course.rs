//! Courses (PAT-08): a row of units along an edge of a hardscape (a soldier course along the
//! kerb, a border round an island). The form becomes the parameters of `hardscape.course`; the
//! preview is the same call on a copy of the document, read with `layout::plan::groups_of`.

use glam::DVec2;
use kadakas_core::hardscape;
use kadakas_core::layout::course::{Corner, Side};
use kadakas_core::layout::plan::{self, Body, CourseDef, Planned};
use kadakas_core::layout::{self, bond};
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_l10n::tr;
use serde_json::{Value, json};

use super::Backend;

/// The edge a course follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Along {
    Outline,
    /// Hole n of the hardscape (counted from 0).
    Hole(usize),
}

impl Along {
    /// The value of `along`.
    pub fn id(self) -> String {
        match self {
            Along::Outline => "outline".into(),
            Along::Hole(n) => format!("hole:{n}"),
        }
    }

    pub fn parse(s: &str) -> Option<Along> {
        if s == "outline" {
            return Some(Along::Outline);
        }
        s.strip_prefix("hole:")?.parse().ok().map(Along::Hole)
    }

    pub fn label(self) -> String {
        match self {
            Along::Outline => tr!("uic-paving-along-outline"),
            Along::Hole(n) => tr!("uic-paving-along-hole", number = n + 1),
        }
    }
}

/// Where the row lies against the edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placing {
    /// On the side of the paving: inside an outline, outside a hole.
    #[default]
    OnPaving,
    /// On the other side of the edge.
    Opposite,
    /// The edge runs along the middle of the row.
    Centered,
}

impl Placing {
    pub const ALL: [Placing; 3] = [Placing::OnPaving, Placing::Opposite, Placing::Centered];

    pub fn label(self) -> String {
        match self {
            Placing::OnPaving => tr!("uic-paving-placing-paving"),
            Placing::Opposite => tr!("uic-paving-placing-opposite"),
            Placing::Centered => tr!("uic-paving-placing-centered"),
        }
    }

    /// The value of `side`. The ring of an outline runs counterclockwise (the paving is on the
    /// left, "inside"); a hole is read the other way (the paving is "outside" it).
    fn side(self, along: Along) -> &'static str {
        match (self, along) {
            (Placing::Centered, _) => "center",
            (Placing::OnPaving, Along::Outline) | (Placing::Opposite, Along::Hole(_)) => "inside",
            _ => "outside",
        }
    }

    fn of(side: Side, along: Along) -> Placing {
        match (side, along) {
            (Side::Center, _) => Placing::Centered,
            (Side::Left, Along::Outline) | (Side::Right, Along::Hole(_)) => Placing::OnPaving,
            _ => Placing::Opposite,
        }
    }
}

/// How the unit lies in the row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// The long side across the row (units stand on their ends, along a kerb).
    #[default]
    Soldier,
    /// The long side along the row.
    Stretcher,
}

impl Orientation {
    pub const ALL: [Orientation; 2] = [Orientation::Soldier, Orientation::Stretcher];

    pub fn id(self) -> &'static str {
        match self {
            Orientation::Soldier => "soldier",
            Orientation::Stretcher => "stretcher",
        }
    }

    pub fn label(self) -> String {
        match self {
            Orientation::Soldier => tr!("uic-paving-orientation-soldier"),
            Orientation::Stretcher => tr!("uic-paving-orientation-stretcher"),
        }
    }
}

/// The label of a corner rule.
pub fn corner_label(c: Corner) -> String {
    match c {
        Corner::Miter => tr!("uic-paving-corner-miter"),
        Corner::Butt => tr!("uic-paving-corner-butt"),
        Corner::CornerUnit => tr!("uic-paving-corner-unit"),
    }
}

/// The form of one course.
#[derive(Clone, Debug, PartialEq)]
pub struct CourseForm {
    /// The course exists already: its name stays, and Apply replaces it.
    pub existing: bool,
    pub name: String,
    pub along: Along,
    pub placing: Placing,
    /// Moves the row away from the edge (mm).
    pub offset: f64,
    pub orientation: Orientation,
    /// The size along the row (mm).
    pub unit_length: f64,
    /// The size across the row (mm).
    pub unit_width: f64,
    pub gap: f64,
    pub corner: Corner,
    /// Start the field at the edge of the course, so that its first row is whole.
    pub align_field: bool,
}

/// The stone of the style of a hardscape: long side, short side and gap, if it has a bond.
pub fn stone(doc: &Document, id: ObjectId) -> Option<(f64, f64, f64)> {
    let o = doc.object(id)?;
    let (_, style) = hardscape::data::style_of(doc, o)?;
    let j = style.joints?;
    j.module().is_none().then(|| {
        (
            j.unit_length.max(j.unit_width),
            j.unit_length.min(j.unit_width),
            j.spacing,
        )
    })
}

impl CourseForm {
    /// A new course along the outline with the stone of the style, soldier.
    pub fn new(doc: &Document, id: ObjectId) -> CourseForm {
        let (long, short, gap) = stone(doc, id).unwrap_or((200.0, 100.0, 5.0));
        let n = doc
            .object(id)
            .and_then(plan::data)
            .map_or(0, |p| p.courses.len());
        CourseForm {
            existing: false,
            name: tr!("uic-paving-course-name", number = n + 1),
            along: Along::Outline,
            placing: Placing::OnPaving,
            offset: 0.0,
            orientation: Orientation::Soldier,
            unit_length: short,
            unit_width: long,
            gap,
            corner: Corner::Miter,
            align_field: false,
        }
    }

    /// The form of a course that follows an edge. `None` for a course along a drawn line or a
    /// path of its own: those are made and removed with the API.
    pub fn from_def(def: &CourseDef) -> Option<CourseForm> {
        let along = Along::parse(def.edge.as_deref()?)?;
        Some(CourseForm {
            existing: true,
            name: def.name.clone(),
            along,
            placing: Placing::of(def.side, along),
            offset: def.offset,
            orientation: if def.along <= def.depth {
                Orientation::Soldier
            } else {
                Orientation::Stretcher
            },
            unit_length: def.along,
            unit_width: def.depth,
            gap: def.gap,
            corner: def.corner,
            align_field: false,
        })
    }

    /// Turns the unit by 90 degrees: the two sizes change places.
    pub fn set_orientation(&mut self, o: Orientation) {
        if self.orientation != o {
            self.orientation = o;
            std::mem::swap(&mut self.unit_length, &mut self.unit_width);
        }
    }

    /// The parameters of `hardscape.course`. The unit type has the name of the stone when the
    /// sizes are those of the stone of the style, so that the course and the field are one row in
    /// the counts; other sizes are a unit type of their own.
    pub fn params(&self, doc: &Document, id: ObjectId) -> Value {
        let mut p = json!({
            "id": id,
            "name": self.name.trim(),
            "along": self.along.id(),
            "side": self.placing.side(self.along),
            "offset": self.offset,
            "orientation": self.orientation.id(),
            "unit_length": self.unit_length,
            "unit_width": self.unit_width,
            "gap": self.gap,
            "corner": self.corner.id(),
            "align_field": self.align_field,
        });
        let long = self.unit_length.max(self.unit_width);
        let short = self.unit_length.min(self.unit_width);
        let same = stone(doc, id).is_some_and(|(l, s, _)| l == long && s == short);
        if !same {
            p["unit"] = json!(bond::size_name(long, short));
        }
        p
    }

    /// The edges that a course can follow: the outline and the holes.
    pub fn edges(doc: &Document, id: ObjectId) -> Vec<Along> {
        let Some(o) = doc
            .object(id)
            .filter(|o| matches!(o.shape, Shape::Hardscape { .. }))
        else {
            return Vec::new();
        };
        let holes = hardscape::data(o).unwrap_or_default().holes.len();
        std::iter::once(Along::Outline)
            .chain((0..holes).map(Along::Hole))
            .collect()
    }
}

/// A course of the hardscape, for the list.
#[derive(Clone, Debug, PartialEq)]
pub struct CourseRow {
    pub name: String,
    pub units: usize,
    pub corners: usize,
    pub length_m: f64,
    /// Narrowest and widest joint (mm), when the row has joints.
    pub joints: Option<(f64, f64)>,
    /// The course follows an edge, so the form edits it.
    pub editable: bool,
}

/// The courses of a hardscape as they are laid. `Ok(vec![])` without a pattern.
pub fn rows(doc: &Document, id: ObjectId, max_units: usize) -> Result<Vec<CourseRow>, String> {
    let o = doc.object(id).ok_or_else(|| tr!("uic-paving-no-object"))?;
    let p = plan::data(o).unwrap_or_default();
    if p.courses.is_empty() {
        return Ok(Vec::new());
    }
    let Some(planned) = plan::groups_of(doc, o, layout::TOLERANCE, max_units)? else {
        return Ok(Vec::new());
    };
    Ok(planned
        .courses
        .iter()
        .map(|c| CourseRow {
            name: c.name.clone(),
            units: c.units,
            corners: c.corners,
            length_m: c.length / 1000.0,
            joints: (c.joints.count > 0).then_some((c.joints.narrowest, c.joints.widest)),
            editable: p
                .courses
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(&c.name))
                .is_some_and(|d| d.edge.is_some()),
        })
        .collect())
}

/// The definition of a course by name.
pub fn def_of(doc: &Document, id: ObjectId, name: &str) -> Option<CourseDef> {
    plan::data(doc.object(id)?)?
        .courses
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
}

/// The polygons of the units of the course `name` in a laid plan.
pub fn unit_outlines(planned: &Planned, name: &str) -> Vec<Vec<DVec2>> {
    planned
        .groups
        .iter()
        .filter(|g| g.name.eq_ignore_ascii_case(name))
        .filter_map(|g| match &g.body {
            Body::Course(c) => Some(c.pieces.iter().map(|p| p.poly.clone())),
            _ => None,
        })
        .flatten()
        .collect()
}

/// What a laid course looks like, for the preview and the readout.
#[derive(Clone, Debug, PartialEq)]
pub struct CoursePreview {
    pub units: Vec<Vec<DVec2>>,
    pub row: Option<CourseRow>,
    /// The notes of the API (a joint wider than the gap, a corner that does not fit).
    pub notes: Vec<String>,
}

/// The editor of one course in Object Info.
#[derive(Clone, Debug)]
pub struct CourseEditor {
    pub target: ObjectId,
    pub form: CourseForm,
    pub preview: Result<CoursePreview, String>,
    seen: Option<(u64, CourseForm)>,
}

impl CourseEditor {
    /// A new course. `Err` if the hardscape has no joint pattern.
    pub fn new_course(doc: &Document, target: ObjectId) -> Result<CourseEditor, String> {
        Self::check(doc, target)?;
        Ok(CourseEditor {
            target,
            form: CourseForm::new(doc, target),
            preview: Err(String::new()),
            seen: None,
        })
    }

    /// The editor of an existing course. `Err` if it does not follow an edge.
    pub fn edit(doc: &Document, target: ObjectId, name: &str) -> Result<CourseEditor, String> {
        Self::check(doc, target)?;
        let form = def_of(doc, target, name)
            .and_then(|d| CourseForm::from_def(&d))
            .ok_or_else(|| tr!("uic-paving-course-not-editable"))?;
        Ok(CourseEditor {
            target,
            form,
            preview: Err(String::new()),
            seen: None,
        })
    }

    fn check(doc: &Document, target: ObjectId) -> Result<(), String> {
        let o = doc
            .object(target)
            .ok_or_else(|| tr!("uic-paving-no-object"))?;
        if !matches!(o.shape, Shape::Hardscape { .. }) {
            return Err(tr!("uic-paving-not-a-hardscape"));
        }
        if layout::object::pattern_of(doc, o).is_none() {
            return Err(tr!("uic-paving-no-pattern"));
        }
        Ok(())
    }

    /// Makes the preview again if the form or the document changed.
    pub fn refresh(&mut self, be: &dyn Backend) {
        if self
            .seen
            .as_ref()
            .is_some_and(|(r, f)| *r == be.revision() && *f == self.form)
        {
            return;
        }
        self.seen = Some((be.revision(), self.form.clone()));
        if self.form.name.trim().is_empty() {
            self.preview = Err(tr!("uic-paving-course-needs-name"));
            return;
        }
        let params = self.form.params(be.document(), self.target);
        self.preview = be
            .rehearse("hardscape.course", params)
            .and_then(|(reply, copy)| {
                let o = copy
                    .object(self.target)
                    .ok_or_else(|| tr!("uic-paving-no-object"))?;
                let planned = plan::groups_of(&copy, o, layout::TOLERANCE, be.pattern_units())?
                    .ok_or_else(|| tr!("uic-paving-no-pattern"))?;
                let row = rows(&copy, self.target, be.pattern_units())?
                    .into_iter()
                    .find(|r| r.name.eq_ignore_ascii_case(self.form.name.trim()));
                Ok(CoursePreview {
                    units: unit_outlines(&planned, self.form.name.trim()),
                    row,
                    notes: reply
                        .get("notes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                })
            });
    }

    /// Stores the course (one undo step).
    pub fn apply(&self, be: &mut dyn Backend) -> Result<(), String> {
        let params = self.form.params(be.document(), self.target);
        be.call("hardscape.course", params).map(|_| ())
    }
}

/// Removes a course (one undo step).
pub fn remove(be: &mut dyn Backend, target: ObjectId, name: &str) -> Result<(), String> {
    be.call(
        "hardscape.course_remove",
        json!({"id": target, "names": [name]}),
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests;
