//! Zones (PAT-07): split lines over a hardscape, the preview of the zones, and the settings of a
//! zone (pattern, origin, angle, alignment).
//!
//! The lines go to `hardscape.split` as the API takes them: the ID of a drawn line, arc or
//! polyline (it is followed when it moves) or a list of vertices. The preview runs that call on a
//! copy of the document and reads the zones from the plan of the copy with the same code that the
//! report and the export use (`layout::plan::groups_of`).

use glam::DVec2;
use kadakas_core::command::Command;
use kadakas_core::curve::{self, Vertex};
use kadakas_core::layout::plan::{self, Planned, Role, ZoneSpec, ZoneUnits};
use kadakas_core::layout::{self, Bond, Pattern};
use kadakas_core::model::{Document, Object, ObjectId, Shape};
use kadakas_l10n::tr;
use serde_json::{Value, json};

use super::Backend;

/// A split line of the draft.
#[derive(Clone, Debug, PartialEq)]
pub enum DraftLine {
    /// A drawn line, arc or polyline. The zones follow it when it moves.
    Object(ObjectId),
    /// A line that was drawn for the split.
    Path(Vec<Vertex>),
}

/// The draft of the split lines of one hardscape.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitDraft {
    pub target: ObjectId,
    pub lines: Vec<DraftLine>,
    /// The points of the line that is being drawn.
    pub open: Vec<DVec2>,
    /// Keep the zones tied to the drawn lines that were picked.
    pub follow: bool,
    /// A line end this close to an edge joins it (mm).
    pub tolerance: f64,
    pub zone_units: ZoneUnits,
}

impl SplitDraft {
    /// A draft with the split lines that the hardscape has already. `None` if the object is not
    /// a hardscape.
    pub fn of(doc: &Document, target: ObjectId) -> Option<SplitDraft> {
        let o = doc.object(target)?;
        if !matches!(o.shape, Shape::Hardscape { .. }) {
            return None;
        }
        let p = plan::data(o).unwrap_or_default();
        let follow = p.splits.iter().any(|s| s.source.is_some()) || p.splits.is_empty();
        let lines = p
            .splits
            .iter()
            .map(|s| match s.source.filter(|id| doc.object(*id).is_some()) {
                Some(id) => DraftLine::Object(id),
                None => DraftLine::Path(s.points.clone()),
            })
            .collect();
        Some(SplitDraft {
            target,
            lines,
            open: Vec::new(),
            follow,
            tolerance: p.tolerance,
            zone_units: p.zone_units,
        })
    }

    /// A click of the line that is being drawn.
    pub fn click(&mut self, p: DVec2) {
        if self.open.last() != Some(&p) {
            self.open.push(p);
        }
    }

    /// Takes the last point of the line back.
    pub fn back(&mut self) -> bool {
        self.open.pop().is_some()
    }

    /// Ends the line that is being drawn. `false` if it has fewer than two points.
    pub fn finish_line(&mut self) -> bool {
        if self.open.len() < 2 {
            return false;
        }
        let pts = std::mem::take(&mut self.open);
        self.lines
            .push(DraftLine::Path(curve::corners(&pts).to_vec()));
        true
    }

    /// Adds the drawn lines, arcs and polylines among these objects. Returns how many were added.
    pub fn add_objects(&mut self, doc: &Document, ids: &[ObjectId]) -> usize {
        let mut n = 0;
        for id in ids {
            if *id == self.target || self.lines.contains(&DraftLine::Object(*id)) {
                continue;
            }
            if doc
                .object(*id)
                .is_some_and(|o| plan::path_of(&o.shape).is_some())
            {
                self.lines.push(DraftLine::Object(*id));
                n += 1;
            }
        }
        n
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.lines.len() {
            self.lines.remove(i);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The parameters of `hardscape.split`. The zone settings that the object has stay: the
    /// call keeps them when `zones` is left out.
    pub fn params(&self) -> Value {
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| match l {
                DraftLine::Object(id) => json!(id),
                DraftLine::Path(p) => json!(p),
            })
            .collect();
        json!({
            "id": self.target,
            "lines": lines,
            "follow": self.follow,
            "tolerance": self.tolerance,
            "zone_units": match self.zone_units {
                ZoneUnits::Cut => "cut",
                ZoneUnits::Centroid => "centroid",
            },
        })
    }

    /// The points of every line, for drawing: the lines of the draft and the one being drawn.
    pub fn polylines(&self, doc: &Document) -> Vec<Vec<DVec2>> {
        let mut out: Vec<Vec<DVec2>> = self
            .lines
            .iter()
            .filter_map(|l| match l {
                DraftLine::Object(id) => doc
                    .object(*id)
                    .and_then(|o| plan::path_of(&o.shape))
                    .map(|(p, closed)| curve::flatten(&p, closed, 1.0)),
                DraftLine::Path(p) => Some(curve::flatten(p, false, 1.0)),
            })
            .collect();
        if !self.open.is_empty() {
            out.push(self.open.clone());
        }
        out
    }
}

/// One zone with its outline.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneShape {
    pub index: usize,
    pub name: String,
    pub area_m2: f64,
    pub inside: DVec2,
    /// Shapes, each the outer ring and then its holes.
    pub shapes: Vec<Vec<Vec<DVec2>>>,
    /// The index of its setting in the plan.
    pub spec: Option<usize>,
    pub has_setting: bool,
}

/// The zones of a hardscape as the report lays them. `Ok(vec![])` if there is no split. `Err`:
/// the reason the plan cannot be laid.
pub fn zone_shapes(
    doc: &Document,
    id: ObjectId,
    max_units: usize,
) -> Result<Vec<ZoneShape>, String> {
    let Some(o) = doc.object(id) else {
        return Err(tr!("uic-paving-no-object"));
    };
    let Some(planned) = plan::groups_of(doc, o, layout::TOLERANCE, max_units)? else {
        return Ok(Vec::new());
    };
    Ok(shapes_of(&planned))
}

fn shapes_of(planned: &Planned) -> Vec<ZoneShape> {
    if planned.zones.len() < 2 {
        return Vec::new();
    }
    let groups = planned.groups.iter().filter(|g| g.role != Role::Course);
    planned
        .zones
        .iter()
        .zip(groups)
        .enumerate()
        .map(|(index, (z, g))| {
            // A zone whose units go by their center shares the lattice; its own shape is in the owner.
            let region = match &g.owner {
                Some(o) => o.zones.get(o.index).unwrap_or(&g.region),
                None => &g.region,
            };
            ZoneShape {
                index,
                name: z.name.clone(),
                area_m2: z.area / 1e6,
                inside: z.inside,
                shapes: region.shapes.clone(),
                spec: z.spec,
                has_setting: z.has_setting,
            }
        })
        .collect()
}

/// What `hardscape.split` says about the zones, read from its reply.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitReport {
    pub zones: usize,
    pub area_m2: f64,
    pub zones_area_m2: f64,
    /// Whole units of the lattice that a zone edge cut, and the pieces it added.
    pub edge_cuts: Option<(usize, usize)>,
    /// The notes of the API (a line that stops short, a line that cuts nothing).
    pub notes: Vec<String>,
}

impl SplitReport {
    pub fn parse(reply: &Value) -> SplitReport {
        let cuts = reply.get("zone_edge_cuts").filter(|c| !c.is_null());
        SplitReport {
            zones: reply
                .get("zones")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            area_m2: reply.get("area_m2").and_then(Value::as_f64).unwrap_or(0.0),
            zones_area_m2: reply
                .get("zones_area_m2")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            edge_cuts: cuts.map(|c| {
                (
                    c["whole_units_lost"].as_u64().unwrap_or(0) as usize,
                    c["extra_pieces"].as_u64().unwrap_or(0) as usize,
                )
            }),
            notes: reply
                .get("notes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        }
    }
}

/// The dialog that splits a hardscape into zones.
#[derive(Clone, Debug)]
pub struct SplitDialog {
    pub draft: SplitDraft,
    pub zones: Result<Vec<ZoneShape>, String>,
    pub report: Option<SplitReport>,
    seen: Option<(u64, SplitDraft)>,
}

impl SplitDialog {
    /// Opens the dialog on a hardscape with a joint pattern.
    pub fn open(be: &dyn Backend, target: ObjectId) -> Result<SplitDialog, String> {
        let doc = be.document();
        let Some(o) = doc.object(target) else {
            return Err(tr!("uic-paving-no-object"));
        };
        if !matches!(o.shape, Shape::Hardscape { .. }) {
            return Err(tr!("uic-paving-not-a-hardscape"));
        }
        if layout::object::pattern_of(doc, o).is_none() {
            return Err(tr!("uic-paving-no-pattern"));
        }
        let draft = SplitDraft::of(doc, target).ok_or_else(|| tr!("uic-paving-not-a-hardscape"))?;
        let mut d = SplitDialog {
            draft,
            zones: Ok(Vec::new()),
            report: None,
            seen: None,
        };
        d.refresh(be);
        Ok(d)
    }

    /// Makes the preview again if the draft or the document changed.
    pub fn refresh(&mut self, be: &dyn Backend) {
        if self
            .seen
            .as_ref()
            .is_some_and(|(r, d)| *r == be.revision() && *d == self.draft)
        {
            return;
        }
        self.seen = Some((be.revision(), self.draft.clone()));
        if self.draft.is_empty() {
            self.zones = Ok(Vec::new());
            self.report = None;
            return;
        }
        match be.rehearse("hardscape.split", self.draft.params()) {
            Ok((reply, copy)) => {
                self.report = Some(SplitReport::parse(&reply));
                self.zones = zone_shapes(&copy, self.draft.target, be.pattern_units());
            }
            Err(e) => {
                self.report = None;
                self.zones = Err(e);
            }
        }
    }

    /// Stores the split lines (one undo step). The zone count is in the reply.
    pub fn commit(&mut self, be: &mut dyn Backend) -> Result<SplitReport, String> {
        if self.draft.is_empty() {
            return Err(tr!("uic-paving-no-lines"));
        }
        let reply = be.call("hardscape.split", self.draft.params())?;
        Ok(SplitReport::parse(&reply))
    }
}

// ---------------------------------------------------------------- the settings of a zone

/// Which pattern a zone lays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternChoice {
    /// The pattern of the hardscape, cut at the zone edge.
    Field,
    /// One of the four bonds, with sizes of its own.
    Bond(Bond),
    /// A module or generator pattern that the API gave the zone. It stays as it is.
    Kept,
}

/// The settings of one zone, as the form shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneSettings {
    pub name: String,
    pub pattern: PatternChoice,
    pub unit_length: f64,
    pub unit_width: f64,
    pub gap: f64,
    pub origin: Option<DVec2>,
    pub angle: Option<f64>,
    /// Start at vertex n of the outline of the zone.
    pub align_to_edge: Option<usize>,
    /// Start at the first point of split line n.
    pub align_to_split: Option<usize>,
}

impl ZoneSettings {
    /// The settings of a zone: its setting if it has one, else the pattern of the field.
    pub fn read(doc: &Document, o: &Object, zone: &ZoneShape) -> ZoneSettings {
        let plan = plan::data(o).unwrap_or_default();
        let spec = zone.spec.and_then(|i| plan.zones.get(i));
        let field = layout::object::pattern_of(doc, o).map(|(p, _)| p);
        let base = |f: fn(&Pattern) -> f64, d: f64| field.as_ref().map_or(d, f);
        let own = spec.and_then(|s| s.pattern.as_ref());
        let pick = |f: fn(&Pattern) -> f64, d: f64| own.map_or(base(f, d), f);
        ZoneSettings {
            name: spec
                .and_then(|s| s.name.clone())
                .unwrap_or_else(|| zone.name.clone()),
            pattern: match own {
                None => PatternChoice::Field,
                Some(p) if matches!(p.bond, Bond::Module | Bond::Generator) => PatternChoice::Kept,
                Some(p) => PatternChoice::Bond(p.bond),
            },
            unit_length: pick(|p| p.unit_length, 200.0),
            unit_width: pick(|p| p.unit_width, 100.0),
            gap: pick(|p| p.gap, 5.0),
            origin: spec.and_then(|s| s.origin),
            angle: spec.and_then(|s| s.angle),
            align_to_edge: spec.and_then(|s| s.align_to_edge),
            align_to_split: spec.and_then(|s| s.align_to_split),
        }
    }

    /// Aligns the pattern to an edge of the zone. The origin and the angle go: the edge gives them.
    pub fn align_edge(&mut self, n: usize) {
        self.align_to_edge = Some(n);
        self.align_to_split = None;
        self.origin = None;
        self.angle = None;
    }

    /// Aligns the pattern to a split line.
    pub fn align_split(&mut self, n: usize) {
        self.align_to_split = Some(n);
        self.align_to_edge = None;
        self.origin = None;
        self.angle = None;
    }

    fn write(&self, spec: &mut ZoneSpec) {
        let name = self.name.trim();
        spec.name = (!name.is_empty()).then(|| name.to_string());
        match self.pattern {
            PatternChoice::Field => spec.pattern = None,
            PatternChoice::Bond(b) => {
                spec.pattern = Some(Pattern::new(b, self.unit_length, self.unit_width, self.gap))
            }
            PatternChoice::Kept => {
                if let Some(p) = &mut spec.pattern {
                    p.gap = self.gap;
                }
            }
        }
        spec.origin = self.origin;
        spec.angle = self.angle;
        spec.align_to_edge = self.align_to_edge;
        spec.align_to_split = self.align_to_split;
    }
}

/// The command that stores the settings of a zone in the plan of the hardscape: the same data
/// that `hardscape.split` writes (`SetData` of the plan), one undo step. The result is laid before
/// the command is given, so a setting that cannot be laid is refused here with the reason.
pub fn set_zone_command(
    doc: &Document,
    id: ObjectId,
    zone: &ZoneShape,
    settings: &ZoneSettings,
    max_units: usize,
) -> Result<Command, String> {
    let o = doc.object(id).ok_or_else(|| tr!("uic-paving-no-object"))?;
    let mut p = plan::data(o).unwrap_or_default();
    let i = match zone.spec.filter(|i| *i < p.zones.len()) {
        Some(i) => i,
        None => {
            p.zones.push(ZoneSpec {
                seed: zone.inside,
                ..Default::default()
            });
            p.zones.len() - 1
        }
    };
    settings.write(&mut p.zones[i]);
    store_command(doc, o, p, max_units)
}

/// The command for a plan after a check: the plan is valid and can be laid.
pub(crate) fn store_command(
    doc: &Document,
    o: &Object,
    p: plan::Plan,
    max_units: usize,
) -> Result<Command, String> {
    p.validate()?;
    let value = serde_json::to_value(&p).map_err(|e| e.to_string())?;
    let mut probe = o.clone();
    probe.data.insert(plan::KEY.into(), value.clone());
    plan::groups_of(doc, &probe, layout::TOLERANCE, max_units)?;
    Ok(Command::SetData {
        id: o.id,
        set: [(plan::KEY.to_string(), value)].into_iter().collect(),
        remove: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
