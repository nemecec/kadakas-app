//! Agent API for markup on the view (REV-11, stage 1): `markup.list`, `markup.get`,
//! `markup.create`, `markup.update`, `markup.delete`, `markup.clear` and `markup.render`.
//!
//! The model is in `kadakas_core::markup` (review data of the document, saved in the `.kad` file,
//! not part of the model). This module turns calls into commands, so every change is one undo step
//! and one change set. A markup that an agent makes has the origin `agent`. Deleting or clearing
//! the work of somebody else needs the consent of the person (`confirm`), as for issues.
//!
//! `markup.get` is the call that lets an agent see what the person drew without a screenshot round
//! trip: the marks as JSON, the model at the stored viewpoint without and with the marks, and for
//! every mark what is under it, found by picking (`pick.rs`), and the surface or ground point
//! behind it.

use serde::Deserialize;
use serde_json::{Value, json};

use kadakas_core::change::Origin;
use kadakas_core::command::Command;
use kadakas_core::issue::{IssueId, ViewSpec};
use kadakas_core::markup::{
    self as mk, Mark, MarkKind, Markup, MarkupId, MarkupPatch, MarkupViewpoint, Rgb, canvas_of,
};
use kadakas_review::issues::{capture_visibility, time};

use crate::errors::{REASON_CONFIRM_NEEDED, bad};
use crate::review_api::{default_now, session_author};
use crate::{RpcError, Session};

mod pick;
mod render;

pub const MARKUP_LIST: (&str, &str) = (
    "markup.list",
    "List the markups of the document (REV-11). A markup is ink that a person or an agent drew on a plan or 3D view. params: {issue? (only markups of this issue), origin? (user or agent), limit? (default 200, at most 1000), offset?}. Returns {markups: [{id, name, author, origin, created (ISO 8601), created_ms, mode (plan or 3d), canvas, marks, points, issue, note}], count, total, offset, next_offset, unreadable (entries of the file that this version cannot read and keeps as they are)}.",
);
pub const MARKUP_GET: (&str, &str) = (
    "markup.get",
    "One markup with everything an agent needs to see what was drawn. params: {id, render? (default true: add the two pictures), width? (pixels of the pictures, default 800), samples? (picks per mark, default 25, at most 100), pick? (default true: find what is under each mark)}. Returns {id, name, author, origin, created, created_ms, note, issue, viewpoint: {mode, view, fov, canvas (points), layers, classes, visibility: {matches, layers_differ, classes_differ}}, marks: [{id, kind, color, width, points (0 to 1 of the canvas, y down), points_canvas (points of the stored canvas), text, bbox: {relative: [x0, y0, x1, y1], canvas: [x0, y0, x1, y1]}, closed, under, hint3d}], clean: {png_base64, width, height} (the model at the stored viewpoint, no marks), with_marks: {png_base64, width, height}, notes}. `under` lists what the mark points at. A plan gives object ids found by hit testing along a stroke or inside a closed shape. A 3D view casts a ray for each sample like mesh.pick and gives {models: [{model, faces, hits}], objects: [{id, kind, hits}], ids}. An arrow gives {tip, tail, ids}. `hint3d` is the first surface or ground point behind the arrow tip, the center of a shape, the first point of a stroke or the place of a note, in drawing coordinates: {at, kind (surface, object, ground, plan, none), point, model?, face?, id?}. Picking uses the layers and classes that show now and skips grey ones. A call makes at most 5000 samples. `truncated` is true when that budget ran out.",
);
pub const MARKUP_CREATE: (&str, &str) = (
    "markup.create",
    "Draw a markup. One undo step. The origin is the origin of the session (agent for an agent). params: {marks: [{kind (pen, highlighter, arrow, rectangle, ellipse or note), points (0 to 1 of the canvas width and height, y down) or points_canvas (points of the canvas), color? (#rrggbb, default red), width? (points, default 3), text? (a note needs it)}], viewpoint? {view? ({kind: plan, center: [x, y], zoom} or {kind: camera, target: [x, y, z], yaw, pitch, distance, perspective?}, default the current view), fov? (degrees), canvas? ([width, height] in points, default [1200, 800]), capture_visibility? (default true)}, name?, note?, issue? (an issue ID), author?, now_ms?}. An arrow, a rectangle and an ellipse have two points. A note has one. A pen or highlighter stroke has 1 to 1000. At most 200 marks and 20000 points in a markup, 2000 markups and 400000 points in a document. A value over a limit is refused and nothing is cut. Returns {id, marks, markup: summary}.",
);
pub const MARKUP_UPDATE: (&str, &str) = (
    "markup.update",
    "Change the name, the note or the issue of a markup. One undo step. params: {id, name?, note?, issue? (an issue ID, or null to take the markup off its issue)}. A markup of an issue goes into the BCF snapshot of that issue. Returns {id, changed, markup: summary}.",
);
pub const MARKUP_DELETE: (&str, &str) = (
    "markup.delete",
    "Delete markups with their marks. One undo step. The work of somebody else is never deleted silently. A markup of another origin or author needs {confirm: true}, which the caller sends only after the person said yes. Without it the call fails with a conflict error (reason confirm_needed) that lists the markups and why, and nothing is deleted. params: {id} or {ids}, confirm?. Returns {deleted: [IDs], count}.",
);
pub const MARKUP_CLEAR: (&str, &str) = (
    "markup.clear",
    "Remove all marks of a markup and keep the markup with its viewpoint and note. One undo step. The consent rule of markup.delete holds. params: {id, confirm?}. Returns {id, removed (marks)}.",
);
pub const MARKUP_RENDER: (&str, &str) = (
    "markup.render",
    "A picture of a markup: the model at its stored viewpoint, with the marks drawn on it (default) or without. params: {id, with_marks? (default true), width? (pixels, default 800)}. Returns {id, with_marks, png_base64, width, height}. The same pictures are in markup.get.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "model": "A markup is review data like an issue. It is saved in the file and it is not part of the model. It is in no quantity, no IFC and no DXF export, and the object methods do not list it. A markup has a viewpoint and a list of marks. The viewpoint is a plan view (center, zoom) or a 3D camera (target, yaw, pitch, distance, projection, field of view), the visible layers and classes, and the size of the canvas in points. A mark is a pen stroke, a highlighter stroke, an arrow, a rectangle, an ellipse or a text note, with a colour, a width and points. Points are relative to the view: x and y from 0 to 1 of the width and the height of the canvas, y down. So a mark scales with the window. A mark has a number that goes up, so the order of creation shows.",
        "rule": "A comment about something that exists is marked on the model (stage 2, planned). A comment about something that is not there but should be is inked on the live view. This is stage 1. Use the hint3d of a mark to find the place of 'a tree here'.",
        "versions": "A markup is not part of a named version. A restore of a version leaves the markups as they are, as it leaves the issues.",
        "bounds": {
            "markups": mk::MAX_MARKUPS,
            "marks_per_markup": mk::MAX_MARKS,
            "points_per_mark": mk::MAX_POINTS,
            "points_per_markup": mk::MAX_POINTS_PER_MARKUP,
            "points_per_document": mk::MAX_POINTS_TOTAL,
            "note_text_chars": mk::MAX_MARK_TEXT_CHARS,
            "markup_note_chars": mk::MAX_NOTE_CHARS,
            "width_points": [mk::MIN_WIDTH, mk::MAX_WIDTH],
            "samples_per_mark": pick::MAX_SAMPLES,
            "samples_per_call": pick::MAX_RAYS,
            "note": "A value over a bound is refused with a message. Nothing is cut. The numbers come from measurements of the size and the time of the data (docs/modeler.md, section Markup).",
        },
        "consent": "markup.delete and markup.clear never remove the work of somebody else silently. They need confirm: true after the person said yes.",
        "origin": "A markup made through the API has the origin agent. The app makes user.",
        "events": "Every change is an events.change with the kind `markups` (document.markups is true). The op is the command (add_markup, add_mark, delete_markup, ...).",
        "pictures": "markup.get gives two pictures: the model at the stored viewpoint without marks (clean) and with the marks drawn on it (with_marks). markup.render gives one of them. A plan viewpoint is drawn from above with a parallel camera that shows the part of the drawing the canvas showed. The marks are painted with the same geometry as the app draws on the canvas.",
        "picking": "Picking follows the rules of the pointer (AGR-10). Only what is displayed is met. Grey layers and classes are skipped. Locked ones are met. The layers and classes that show now decide. viewpoint.visibility lists the ones that differ from the time of the markup.",
        "bcf": "A markup that belongs to an issue goes into the BCF file of that issue as one more viewpoint, with the picture that has the marks as its snapshot. A BCF import keeps a snapshot as a picture. It does not read marks from images.",
        "privacy": "The markup is data of the file. It goes to whoever gets the file. Nothing is sent anywhere.",
        "examples": [
            {"method": "markup.list", "params": {}},
            {"method": "markup.get", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}},
            {"method": "markup.get", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc", "render": false}},
            {"method": "markup.create", "params": {"note": "Add a hedge here.", "marks": [{"kind": "arrow", "points": [[0.2, 0.2], [0.5, 0.6]], "color": "#d62828"}, {"kind": "note", "points": [[0.2, 0.15]], "text": "Hedge"}]}},
            {"method": "markup.update", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc", "issue": "0190f1c2-7a3b-7c1d-8e2f-123456789abd"}},
            {"method": "markup.render", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc", "with_marks": true}},
            {"method": "markup.clear", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}},
            {"method": "markup.delete", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}}
        ],
    })
}

pub fn knows(method: &str) -> bool {
    method.starts_with("markup.")
}

fn parse<T: for<'de> Deserialize<'de>>(p: &Value) -> Result<T, RpcError> {
    let p = if p.is_null() { json!({}) } else { p.clone() };
    serde_json::from_value(p).map_err(RpcError::invalid_params)
}

fn find(doc: &kadakas_core::model::Document, id: MarkupId) -> Result<&Markup, RpcError> {
    doc.markups
        .get(id)
        .ok_or_else(|| RpcError::not_found(format!("markup {id} does not exist")))
}

fn same_author(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    !a.is_empty() && !b.is_empty() && a.to_lowercase() == b.to_lowercase()
}

/// Why changing the marks of a markup needs a yes from the person, or `None`.
fn consent_reason(m: &Markup, me: Origin, my_author: &str) -> Option<String> {
    if m.origin != me {
        return Some(format!(
            "the markup was made by origin {}, not by {}",
            m.origin.name(),
            me.name()
        ));
    }
    if !same_author(&m.author, my_author) {
        return Some(format!(
            "the markup was made by {}, not by {}",
            m.author.trim(),
            my_author.trim()
        ));
    }
    None
}

fn summary(m: &Markup) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "author": m.author,
        "origin": m.origin.name(),
        "created": time::iso(m.time),
        "created_ms": m.time,
        "mode": m.viewpoint.mode().name(),
        "canvas": m.viewpoint.canvas,
        "marks": m.marks.len(),
        "points": m.point_count(),
        "issue": m.issue,
        "note": m.note,
    })
}

fn viewpoint_json(doc: &kadakas_core::model::Document, vp: &MarkupViewpoint) -> Value {
    let mut v = serde_json::to_value(vp).unwrap_or(Value::Null);
    v["mode"] = json!(vp.mode().name());
    v["fov"] = json!(vp.fov_degrees());
    v["visibility"] = pick::visibility_differences(doc, vp);
    v
}

/// A mark as JSON, with the coordinates in both forms and the box around it.
fn mark_json(m: &Mark, canvas: [f64; 2]) -> Value {
    let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
    v["points_canvas"] = json!(
        m.points
            .iter()
            .map(|p| canvas_of(*p, canvas))
            .collect::<Vec<_>>()
    );
    v["known"] = json!(m.kind.is_known());
    v["closed"] = json!(m.is_closed(canvas));
    if let Some((lo, hi)) = m.bounds() {
        let (clo, chi) = (canvas_of(lo, canvas), canvas_of(hi, canvas));
        v["bbox"] = json!({
            "relative": [lo[0], lo[1], hi[0], hi[1]],
            "canvas": [clo[0], clo[1], chi[0], chi[1]],
        });
    }
    v
}

/// The marks of a call.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkP {
    kind: String,
    color: Option<Rgb>,
    width: Option<f64>,
    points: Option<Vec<[f64; 2]>>,
    points_canvas: Option<Vec<[f64; 2]>>,
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewpointP {
    view: Option<ViewSpec>,
    fov: Option<f64>,
    canvas: Option<[f64; 2]>,
    capture_visibility: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateP {
    marks: Vec<MarkP>,
    viewpoint: Option<ViewpointP>,
    name: Option<String>,
    note: Option<String>,
    issue: Option<IssueId>,
    author: Option<String>,
    now_ms: Option<i64>,
}

impl MarkP {
    fn into_mark(self, canvas: [f64; 2]) -> Result<Mark, RpcError> {
        let points = match (self.points, self.points_canvas) {
            (Some(_), Some(_)) => return Err(bad("give points or points_canvas, not both")),
            (Some(p), None) => p,
            (None, Some(c)) => c
                .into_iter()
                .map(|p| [p[0] / canvas[0], p[1] / canvas[1]])
                .collect(),
            (None, None) => return Err(bad("a mark needs points")),
        };
        let kind = MarkKind::from_name(&self.kind);
        if !kind.is_known() {
            return Err(bad(format!(
                "kind {} is not one of pen, highlighter, arrow, rectangle, ellipse, note",
                self.kind
            )));
        }
        let mut m = Mark::new(kind, points);
        if let Some(c) = self.color {
            m.color = c;
        }
        if let Some(w) = self.width {
            m.width = w;
        }
        m.text = self.text.unwrap_or_default();
        Ok(m)
    }
}

const LIST_LIMIT: usize = 200;
const LIST_LIMIT_MAX: usize = 1000;

impl Session {
    /// The method of a `markup.*` call. `None`: not one of ours.
    pub(crate) fn handle_markup(
        &mut self,
        method: &str,
        p: Value,
    ) -> Option<Result<Value, RpcError>> {
        if !knows(method) {
            return None;
        }
        Some(match method {
            "markup.list" => self.markup_list(&p),
            "markup.get" => self.markup_get(&p),
            "markup.create" => self.markup_create(&p),
            "markup.update" => self.markup_update(&p),
            "markup.delete" => self.markup_delete(&p),
            "markup.clear" => self.markup_clear(&p),
            "markup.render" => self.markup_render(&p),
            _ => Err(RpcError::method_not_found(method)),
        })
    }

    fn markup_list(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            issue: Option<IssueId>,
            origin: Option<String>,
            limit: Option<u64>,
            offset: Option<u64>,
        }
        let q: P = parse(p)?;
        let origin = match q.origin {
            None => None,
            Some(t) => Some(
                Origin::parse(&t).ok_or_else(|| bad("origin is user, agent, script or remote"))?,
            ),
        };
        let limit = match q.limit {
            None => LIST_LIMIT,
            Some(0) => return Err(bad("limit is a whole number of 1 or more")),
            Some(n) => (n as usize).min(LIST_LIMIT_MAX),
        };
        let offset = q.offset.unwrap_or(0) as usize;
        let doc = self.editor.doc();
        let list: Vec<&Markup> = doc
            .markups
            .iter()
            .filter(|m| q.issue.is_none_or(|i| m.issue == Some(i)))
            .filter(|m| origin.is_none_or(|o| m.origin == o))
            .collect();
        let total = list.len();
        let shown: Vec<Value> = list
            .iter()
            .skip(offset)
            .take(limit)
            .map(|m| summary(m))
            .collect();
        let next = offset.saturating_add(shown.len());
        Ok(json!({
            "markups": shown,
            "count": shown.len(),
            "total": total,
            "offset": offset,
            "next_offset": (next < total).then_some(next),
            "unreadable": doc.markups.unreadable().len(),
        }))
    }

    fn markup_get(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            id: MarkupId,
            render: Option<bool>,
            width: Option<u32>,
            samples: Option<usize>,
            pick: Option<bool>,
        }
        let q: P = parse(p)?;
        let samples = q.samples.unwrap_or(pick::DEFAULT_SAMPLES);
        if samples == 0 || samples > pick::MAX_SAMPLES {
            return Err(bad(format!("samples is 1 to {}", pick::MAX_SAMPLES)));
        }
        let markup = find(self.editor.doc(), q.id)?.clone();
        let vp = markup.viewpoint.clone();
        let canvas = vp.canvas;
        let mut notes: Vec<String> = Vec::new();
        let mut out = summary(&markup);
        out["viewpoint"] = viewpoint_json(self.editor.doc(), &vp);
        let mut marks: Vec<Value> = markup.marks.iter().map(|m| mark_json(m, canvas)).collect();
        let mut truncated = false;
        if q.pick.unwrap_or(true) {
            let frame = self.markup_frame(&vp);
            let index = matches!(frame, pick::Frame::Plan { .. }).then(|| self.plan_index());
            let mut budget = pick::Budget::new();
            for (m, j) in markup.marks.iter().zip(marks.iter_mut()) {
                if !m.kind.is_known() {
                    notes.push(format!(
                        "mark {} has the kind {}, which this version does not know: it is not picked",
                        m.id,
                        m.kind.name()
                    ));
                    continue;
                }
                let (under, hint) =
                    self.mark_picks(&frame, index.as_ref(), m, samples, &mut budget);
                j["under"] = under;
                j["hint3d"] = hint;
            }
            truncated = budget.truncated;
        }
        out["marks"] = Value::Array(marks);
        if truncated {
            out["truncated"] = json!(true);
            notes.push(format!(
                "the budget of {} samples for one call ran out: later marks have fewer or no samples. Ask for one markup at a time or lower samples",
                pick::MAX_RAYS
            ));
        }
        if q.render.unwrap_or(true) {
            let width = q.width.unwrap_or(render::DEFAULT_WIDTH);
            let clean = self.markup_clean(&vp, width)?;
            out["clean"] = clean.json();
            match self.markup_marked(&markup, &clean) {
                Ok(marked) => out["with_marks"] = marked.json(),
                Err(e) => notes.push(format!(
                    "the picture with the marks is missing: {}",
                    e.message
                )),
            }
        }
        out["notes"] = json!(notes);
        Ok(out)
    }

    fn markup_render(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            id: MarkupId,
            with_marks: Option<bool>,
            width: Option<u32>,
        }
        let q: P = parse(p)?;
        let markup = find(self.editor.doc(), q.id)?.clone();
        let with_marks = q.with_marks.unwrap_or(true);
        let clean =
            self.markup_clean(&markup.viewpoint, q.width.unwrap_or(render::DEFAULT_WIDTH))?;
        let picture = if with_marks {
            self.markup_marked(&markup, &clean)?
        } else {
            clean
        };
        let mut out = picture.json();
        out["id"] = json!(markup.id);
        out["with_marks"] = json!(with_marks);
        Ok(out)
    }

    fn markup_create(&mut self, p: &Value) -> Result<Value, RpcError> {
        let q: CreateP = parse(p)?;
        let vp = q.viewpoint.unwrap_or(ViewpointP {
            view: None,
            fov: None,
            canvas: None,
            capture_visibility: None,
        });
        let canvas = vp.canvas.unwrap_or([1200.0, 800.0]);
        if !canvas.iter().all(|c| c.is_finite() && *c > 0.0) {
            return Err(bad("canvas is [width, height] in points, above 0"));
        }
        if let Some(i) = q.issue
            && self.editor.doc().issues.get(i).is_none()
        {
            return Err(RpcError::not_found(format!("issue {i} does not exist")));
        }
        let view = match vp.view {
            Some(v) => v,
            None => self.current_view(&[]),
        };
        let (layers, classes) = if vp.capture_visibility.unwrap_or(true) {
            capture_visibility(self.editor.doc())
        } else {
            Default::default()
        };
        let marks = q
            .marks
            .into_iter()
            .map(|m| m.into_mark(canvas))
            .collect::<Result<Vec<_>, _>>()?;
        let mut markup = Markup::new(MarkupViewpoint {
            view,
            fov: vp.fov,
            canvas,
            layers,
            classes,
        });
        markup.name = q
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        markup.note = q.note.unwrap_or_default();
        markup.issue = q.issue;
        markup.origin = self.editor.origin();
        markup.author = match q.author {
            Some(a) => a.trim().to_string(),
            None => session_author(self),
        };
        markup.time = q.now_ms.unwrap_or_else(|| default_now() as i64);
        markup.marks = marks;
        let n = markup.marks.len();
        let out = self
            .editor
            .execute(&Command::AddMarkup {
                markup: Box::new(markup),
            })
            .map_err(RpcError::from)?;
        let id = MarkupId(out.created[0]);
        Ok(json!({
            "id": id,
            "marks": n,
            "markup": summary(find(self.editor.doc(), id)?),
        }))
    }

    fn markup_update(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            id: MarkupId,
            name: Option<String>,
            note: Option<String>,
            #[serde(default, deserialize_with = "nullable")]
            issue: Option<Option<IssueId>>,
        }
        fn nullable<'de, D: serde::Deserializer<'de>>(
            d: D,
        ) -> Result<Option<Option<IssueId>>, D::Error> {
            Option::<IssueId>::deserialize(d).map(Some)
        }
        let q: P = parse(p)?;
        find(self.editor.doc(), q.id)?;
        if let Some(Some(i)) = q.issue
            && self.editor.doc().issues.get(i).is_none()
        {
            return Err(RpcError::not_found(format!("issue {i} does not exist")));
        }
        let depth = self.editor.undo_depth();
        let patch = MarkupPatch {
            name: q.name,
            note: q.note,
            issue: q.issue,
        };
        self.editor
            .execute(&Command::ModifyMarkup {
                id: q.id,
                patch: Box::new(patch),
            })
            .map_err(RpcError::from)?;
        Ok(json!({
            "id": q.id,
            "changed": self.editor.undo_depth() != depth,
            "markup": summary(find(self.editor.doc(), q.id)?),
        }))
    }

    fn check_markup_consent(&self, ids: &[MarkupId], confirm: bool) -> Result<(), RpcError> {
        let me = self.editor.origin();
        let my_author = session_author(self);
        let doc = self.editor.doc();
        let mut ask = Vec::new();
        for id in ids {
            let m = find(doc, *id)?;
            if let Some(why) = consent_reason(m, me, &my_author) {
                ask.push(json!({
                    "id": id, "name": m.name, "author": m.author,
                    "origin": m.origin.name(), "marks": m.marks.len(), "why": why,
                }));
            }
        }
        if ask.is_empty() || confirm {
            return Ok(());
        }
        Err(RpcError::conflict(
            "these markups were made by somebody else: nothing was changed. Ask the person, then call again with confirm: true",
        )
        .with_reason(REASON_CONFIRM_NEEDED)
        .with_details(json!({"markups": ask, "ask_the_person": true})))
    }

    fn markup_delete(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            id: Option<MarkupId>,
            ids: Option<Vec<MarkupId>>,
            confirm: Option<bool>,
        }
        let q: P = parse(p)?;
        let ids = match (q.id, q.ids) {
            (Some(i), None) => vec![i],
            (None, Some(l)) => l,
            _ => return Err(bad("give id or ids")),
        };
        self.check_markup_consent(&ids, q.confirm.unwrap_or(false))?;
        let commands = ids
            .iter()
            .map(|id| Command::DeleteMarkup { id: *id })
            .collect();
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        Ok(json!({"deleted": ids, "count": ids.len()}))
    }

    fn markup_clear(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            id: MarkupId,
            confirm: Option<bool>,
        }
        let q: P = parse(p)?;
        let removed = find(self.editor.doc(), q.id)?.marks.len();
        self.check_markup_consent(&[q.id], q.confirm.unwrap_or(false))?;
        self.editor
            .execute(&Command::ClearMarkup { markup: q.id })
            .map_err(RpcError::from)?;
        Ok(json!({"id": q.id, "removed": removed}))
    }
}

/// The PNG of a markup with its marks, for the BCF snapshot of its issue. The picture is at most
/// `max_side` pixels on the longer side.
impl Session {
    pub(crate) fn markup_snapshot_png(
        &mut self,
        markup: &Markup,
        max_side: u32,
    ) -> Result<Vec<u8>, RpcError> {
        let c = markup.viewpoint.canvas;
        let scale = (f64::from(max_side) / c[0].max(c[1])).min(1.0);
        let width = ((c[0] * scale).round() as u32).max(16);
        let clean = self.markup_clean(&markup.viewpoint, width)?;
        let marked = self.markup_marked(markup, &clean)?;
        Ok(marked.png)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
