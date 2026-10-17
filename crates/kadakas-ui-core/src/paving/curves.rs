//! Create Objects from Shapes (PAT-06): loose lines, arcs and polylines become hardscape areas
//! with holes. The dialog asks `region.from_curves` for a dry run (the preview) and, on Create,
//! for the same call with the regions that the person left on. The reply is read into [`Found`]:
//! the regions, and the notes that an import report would have (gaps closed, dead ends, curves
//! left out).

use std::collections::BTreeMap;

use glam::DVec2;
use kadakas_core::model::{ObjectId, Vertex};
use kadakas_l10n::tr;
use serde_json::{Value, json};

use super::{Backend, field_xy};

/// What the regions become.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Make {
    #[default]
    Hardscape,
    LandscapeArea,
    Polyline,
}

impl Make {
    pub const ALL: [Make; 3] = [Make::Hardscape, Make::LandscapeArea, Make::Polyline];

    /// The value of `create`.
    pub fn id(self) -> &'static str {
        match self {
            Make::Hardscape => "hardscape",
            Make::LandscapeArea => "landscape_area",
            Make::Polyline => "polyline",
        }
    }

    pub fn label(self) -> String {
        match self {
            Make::Hardscape => tr!("uic-paving-make-hardscape"),
            Make::LandscapeArea => tr!("uic-paving-make-landscape-area"),
            Make::Polyline => tr!("uic-paving-make-polyline"),
        }
    }
}

/// What a region is, by how deep it lies in the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Area,
    Island,
    /// The curves leave it open whether this is an area or an island; see the doubt.
    Doubtful,
}

impl Role {
    pub fn label(self) -> String {
        match self {
            Role::Area => tr!("uic-paving-role-area"),
            Role::Island => tr!("uic-paving-role-island"),
            Role::Doubtful => tr!("uic-paving-role-doubtful"),
        }
    }
}

/// One region that the curves enclose.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub index: usize,
    pub outline: Vec<Vertex>,
    pub holes: Vec<Vec<Vertex>>,
    pub area_m2: f64,
    pub role: Role,
    pub doubt: Option<String>,
    pub inside: DVec2,
    pub lo: DVec2,
    pub hi: DVec2,
}

/// What a note is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    GapClosed,
    DeadEnd,
    Stub,
    Short,
    Duplicate,
    Sliver,
    Skipped,
    Warning,
}

/// A line of the report that the dialog shows. `at` is where to look.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub kind: NoteKind,
    pub text: String,
    pub at: Option<DVec2>,
}

/// A place on the drawing that the preview marks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: DVec2,
    pub kind: NoteKind,
}

/// The reply of `region.from_curves`.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub regions: Vec<Region>,
    pub notes: Vec<Note>,
    pub marks: Vec<Mark>,
    /// False when a curve ends open (a dead end that is no stub): some area may be missing.
    pub complete: bool,
    pub gaps: usize,
    pub dead_ends: usize,
}

fn count(v: &Value, key: &str) -> usize {
    v.get(key).and_then(Value::as_u64).unwrap_or(0) as usize
}

fn vertices(v: &Value) -> Result<Vec<Vertex>, String> {
    serde_json::from_value(v.clone()).map_err(|_| tr!("uic-paving-bad-reply"))
}

impl Found {
    /// Reads the reply of a dry run.
    pub fn parse(reply: &Value) -> Result<Found, String> {
        let list = reply
            .get("regions")
            .and_then(Value::as_array)
            .ok_or_else(|| tr!("uic-paving-bad-reply"))?;
        let mut regions = Vec::with_capacity(list.len());
        for (n, r) in list.iter().enumerate() {
            let doubt = r.get("doubt").and_then(Value::as_str).map(str::to_string);
            let role = match r.get("role").and_then(Value::as_str) {
                Some("island") => Role::Island,
                Some("island?") => Role::Doubtful,
                _ => Role::Area,
            };
            let holes = match r.get("holes").and_then(Value::as_array) {
                Some(h) => h.iter().map(vertices).collect::<Result<_, _>>()?,
                None => Vec::new(),
            };
            let bbox = r.get("bbox").ok_or_else(|| tr!("uic-paving-bad-reply"))?;
            regions.push(Region {
                index: r
                    .get("index")
                    .and_then(Value::as_u64)
                    .map_or(n, |i| i as usize),
                outline: vertices(
                    r.get("outline")
                        .ok_or_else(|| tr!("uic-paving-bad-reply"))?,
                )?,
                holes,
                area_m2: r.get("area_m2").and_then(Value::as_f64).unwrap_or(0.0),
                role,
                doubt,
                inside: field_xy(r, "inside").unwrap_or_default(),
                lo: field_xy(bbox, "lo").unwrap_or_default(),
                hi: field_xy(bbox, "hi").unwrap_or_default(),
            });
        }
        let mut notes = Vec::new();
        let mut marks = Vec::new();
        let gaps = reply.get("closed_gaps").cloned().unwrap_or(Value::Null);
        let n_gaps = count(&gaps, "count");
        if n_gaps > 0 {
            notes.push(Note {
                kind: NoteKind::GapClosed,
                text: tr!(
                    "uic-paving-note-gaps",
                    count = n_gaps,
                    total = kadakas_units::current()
                        .length_text_d(gaps["total_movement_mm"].as_f64().unwrap_or(0.0), 1)
                ),
                at: None,
            });
        }
        for g in gaps
            .get("listed")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(at) = field_xy(g, "at") {
                let size = g.get("size_mm").and_then(Value::as_f64).unwrap_or(0.0);
                notes.push(Note {
                    kind: NoteKind::GapClosed,
                    text: tr!(
                        "uic-paving-note-gap",
                        size = kadakas_units::current().length_text_d(size, 1)
                    ),
                    at: Some(at),
                });
                marks.push(Mark {
                    at,
                    kind: NoteKind::GapClosed,
                });
            }
        }
        let ends = reply.get("dead_ends").cloned().unwrap_or(Value::Null);
        let n_open = count(&ends, "open");
        let n_stubs = count(&ends, "stubs");
        if n_open > 0 {
            notes.push(Note {
                kind: NoteKind::DeadEnd,
                text: tr!("uic-paving-note-open-ends", count = n_open),
                at: None,
            });
        }
        if n_stubs > 0 {
            notes.push(Note {
                kind: NoteKind::Stub,
                text: tr!("uic-paving-note-stubs", count = n_stubs),
                at: None,
            });
        }
        for d in ends
            .get("listed")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(at) = field_xy(d, "at") {
                let open = d.get("kind").and_then(Value::as_str) == Some("open");
                let len = d.get("length_mm").and_then(Value::as_f64).unwrap_or(0.0);
                let units = kadakas_units::current();
                let text = match d
                    .get("nearest")
                    .and_then(|n| n.get("distance_mm"))
                    .and_then(Value::as_f64)
                {
                    Some(dist) => tr!(
                        "uic-paving-note-dead-end-near",
                        length = units.length_text_d(len, 1),
                        distance = units.length_text_d(dist, 1)
                    ),
                    None => tr!(
                        "uic-paving-note-dead-end",
                        length = units.length_text_d(len, 1)
                    ),
                };
                let kind = if open {
                    NoteKind::DeadEnd
                } else {
                    NoteKind::Stub
                };
                notes.push(Note {
                    kind,
                    text,
                    at: Some(at),
                });
                marks.push(Mark { at, kind });
            }
        }
        let short = count(reply, "ignored_short");
        if short > 0 {
            notes.push(Note {
                kind: NoteKind::Short,
                text: tr!("uic-paving-note-short", count = short),
                at: None,
            });
        }
        let dup = count(reply, "duplicates");
        if dup > 0 {
            notes.push(Note {
                kind: NoteKind::Duplicate,
                text: tr!("uic-paving-note-duplicates", count = dup),
                at: None,
            });
        }
        let slivers = count(reply, "dropped_slivers");
        if slivers > 0 {
            notes.push(Note {
                kind: NoteKind::Sliver,
                text: tr!("uic-paving-note-slivers", count = slivers),
                at: None,
            });
        }
        for s in reply
            .get("dropped_sliver_list")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(at) = field_xy(s, "at") {
                marks.push(Mark {
                    at,
                    kind: NoteKind::Sliver,
                });
            }
        }
        for s in reply
            .get("skipped")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            notes.push(Note {
                kind: NoteKind::Skipped,
                text: tr!(
                    "uic-paving-note-skipped",
                    reason = s.get("reason").and_then(Value::as_str).unwrap_or("")
                ),
                at: None,
            });
        }
        // The notes of the API are for agents and stay in English; they go inside a sentence.
        for w in reply
            .get("warnings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            notes.push(Note {
                kind: NoteKind::Warning,
                text: tr!("uic-paving-note-api", text = w),
                at: None,
            });
        }
        Ok(Found {
            regions,
            notes,
            marks,
            complete: reply
                .get("complete")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            gaps: n_gaps,
            dead_ends: n_open + n_stubs,
        })
    }
}

/// The form of the dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct CurvesForm {
    /// The largest gap to close, in mm.
    pub tolerance: f64,
    pub make: Make,
    /// The name of a hardscape style for the new hardscapes.
    pub style: Option<String>,
    /// Take the curves away when all their pieces are in the regions that are made.
    pub delete_source: bool,
    /// The regions the person switched on or off, by index. The rest follow `default_on`.
    pub switched: BTreeMap<usize, bool>,
}

impl Default for CurvesForm {
    fn default() -> Self {
        CurvesForm {
            tolerance: 1.0,
            make: Make::Hardscape,
            style: None,
            delete_source: false,
            switched: BTreeMap::new(),
        }
    }
}

impl CurvesForm {
    /// The parameters of the preview: the regions only, nothing is made.
    pub fn preview_params(&self, ids: &[ObjectId]) -> Value {
        json!({"ids": ids, "tolerance": self.tolerance, "dry_run": true})
    }

    /// Is this region made? Areas are, as with the call of an agent. Islands are not (they are
    /// holes of the areas), and neither are the doubtful ones; every region of a polyline is. The
    /// person can switch any of them.
    pub fn is_on(&self, r: &Region) -> bool {
        self.switched
            .get(&r.index)
            .copied()
            .unwrap_or_else(|| self.make == Make::Polyline || r.role == Role::Area)
    }

    pub fn toggle(&mut self, r: &Region) {
        let on = self.is_on(r);
        self.switched.insert(r.index, !on);
    }

    /// The indexes of the regions that are made.
    pub fn chosen(&self, found: &Found) -> Vec<usize> {
        found
            .regions
            .iter()
            .filter(|r| self.is_on(r))
            .map(|r| r.index)
            .collect()
    }

    /// The parameters of the real call: the very regions of the preview.
    pub fn commit_params(&self, ids: &[ObjectId], found: &Found) -> Value {
        let mut p = json!({
            "ids": ids,
            "tolerance": self.tolerance,
            "create": self.make.id(),
            "regions": self.chosen(found),
            "delete_source": self.delete_source,
        });
        if self.make == Make::Hardscape
            && let Some(s) = &self.style
        {
            p["style"] = json!(s);
        }
        p
    }
}

/// What the real call made.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub created: Vec<ObjectId>,
    /// Source objects that stayed, with the reason in the words of the API.
    pub kept: usize,
}

/// The dialog Create Objects from Shapes.
#[derive(Clone, Debug)]
pub struct CurvesDialog {
    pub form: CurvesForm,
    /// The shapes of the selection when the dialog opened.
    pub ids: Vec<ObjectId>,
    pub found: Result<Found, String>,
    /// The tolerance and the revision of the last preview; a change of either asks for a new one.
    seen: (u64, f64),
}

impl CurvesDialog {
    /// Opens the dialog on the selection. `Err`: nothing usable is selected.
    pub fn open(be: &dyn Backend) -> Result<CurvesDialog, String> {
        let doc = be.document();
        let ids: Vec<ObjectId> = be
            .selection()
            .iter()
            .copied()
            .filter(|id| {
                doc.object(*id)
                    .is_some_and(|o| kadakas_core::regions::curves_of(&o.shape).is_ok())
            })
            .collect();
        if ids.is_empty() {
            return Err(tr!("uic-paving-no-shapes"));
        }
        let mut d = CurvesDialog {
            form: CurvesForm::default(),
            ids,
            found: Err(String::new()),
            seen: (0, f64::NAN),
        };
        d.refresh(be);
        Ok(d)
    }

    /// True when the preview is not of the document and the tolerance as they are now.
    pub fn stale(&self, be: &dyn Backend) -> bool {
        self.seen.0 != be.revision() || self.seen.1.to_bits() != self.form.tolerance.to_bits()
    }

    /// Makes the preview again if something changed.
    pub fn refresh(&mut self, be: &dyn Backend) {
        if !self.stale(be) {
            return;
        }
        let tolerance_changed = self.seen.1.to_bits() != self.form.tolerance.to_bits();
        self.seen = (be.revision(), self.form.tolerance);
        let tol = self.form.tolerance;
        let params = self.form.preview_params(&self.ids);
        self.found = if tol > 0.0 && tol <= 1000.0 {
            be.rehearse("region.from_curves", params)
                .and_then(|(reply, _)| Found::parse(&reply))
        } else {
            Err(tr!("uic-paving-bad-tolerance"))
        };
        if tolerance_changed {
            // The regions have other numbers now; the switches were for the old ones.
            self.form.switched.clear();
        }
    }

    /// Makes the objects. The call is one undo step and selects what it made.
    pub fn commit(&mut self, be: &mut dyn Backend) -> Result<Made, String> {
        let found = self.found.as_ref().map_err(|e| e.clone())?;
        if self.form.chosen(found).is_empty() {
            return Err(tr!("uic-paving-no-regions"));
        }
        let params = self.form.commit_params(&self.ids, found);
        let reply = be.call("region.from_curves", params)?;
        let created = reply
            .get("created")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| serde_json::from_value(c.get("id")?.clone()).ok())
            .collect();
        let kept = reply
            .get("kept_sources")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok(Made { created, kept })
    }
}

#[cfg(test)]
mod tests;
