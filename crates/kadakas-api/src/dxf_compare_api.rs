//! `dxf.compare` (IOP-24): compare two DXF or DWG files without importing them.
//!
//! The files go through the same code as `import.file` and `import.update`: the readers of
//! `update_api` (path policy, size limits, the DWG converter), the matching engine of
//! `kadakas_io::dxf::revision`, and `kadakas_io::dxf::compare`, which imports each file into a
//! scratch document and maps the changes to objects. The answer is the JSON diff. With `render` it
//! also has an overlay picture: the new file as a temporary document, the changes tinted by
//! `CompareOverlay::from_changes` (added green, changed orange), the objects of the old file that
//! are gone as red ghosts, drawn by the renderers of REV-03 (the SVG of the plan, and the 3D
//! renderer in top view for PNG and PDF). With `revision_clouds` the picture has revision clouds
//! (ANN-10) around the clusters of changes. The document of the session is not touched.
//!
//! Agents can then file review issues on the changes (REV-04). That does not exist yet: the
//! plan is `issue.create` with `objects` empty, a `title` made from the change (`entity`,
//! `status`, `handle`, `layer`), the point of the change in `anchor` (the box of the cloud or of
//! `object_ids`) and the change ID in a label, so the issue and the diff can be matched.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use glam::DVec2;
use kadakas_core::annot::{Annot, RevisionCloud, draw::cloud_outline};
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Attributes, Color, Document, Object, ObjectId, Shape};
use kadakas_core::svg_overlay::{SvgGhost, SvgLegendRow, SvgLook, SvgOverlay, SvgPattern, SvgTint};
use kadakas_io::dxf::compare::{self, Compared};
use kadakas_io::dxf::update::Kind;
use kadakas_review::overlay::clouds::{self, Clusters};
use kadakas_review::overlay::{
    Change, CompareOverlay, CompareStyle, GhostReport, Status, SvgCompare,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::compare_api::{CompareSpec, PngRequest, ShownDoc};
use crate::errors::bad;
use crate::{RpcError, Session, params};

pub(crate) const DXF_COMPARE: (&str, &str) = (
    "dxf.compare",
    "Compare two DXF or DWG files without importing them (IOP-24). params: {base (the old file), new, options? {tolerance? (units of the drawing), layers? (only entities on these DXF layers), limit? (most changes listed, default 200, at most 1000), ...import options such as hatches}, render? {format: png|svg|pdf, revision_clouds? (draw revision clouds around the clusters of changes), width?, height?, palette?, unchanged?, dark?, ghosts?, previous?, view? (png, default top)}}. Returns {base, new, counts, changes: [{id, status: added|removed|changed, entity, handle?, layer, what?, via?, distance?, text?, objects, new_objects, object_ids}], listed, total_changes, notes, match, compare (the legend, counts and ghosts of the overlay, with render), image (with render: {format, svg | png_base64 | pdf_base64, width?, height?, clouds?})}. The matching is that of import.update: by handle first, else by content, geometry and nearest position. Nothing changes in the document. See describe.review.dxf_compare.",
);

/// Most changes that an answer lists.
const MAX_LISTED: usize = 1000;
const DEFAULT_LISTED: usize = 200;
/// The color of revision clouds (a purple that none of the three tints comes close to), for a
/// light and a dark paper.
const CLOUD_RGB: [u8; 3] = [150, 40, 190];
const CLOUD_RGB_DARK: [u8; 3] = [215, 150, 255];
/// The resolution of the page of a PDF, in dots per inch.
const PDF_DPI: f64 = 150.0;

/// What `describe` says about the compare.
pub(crate) fn describe() -> Value {
    json!({
        "what": "dxf.compare reads two DXF or DWG files and tells what changed from the first to the second, without importing either one into the document. Use it to see what an architect changed between two revisions, or before import.update.",
        "diff": "The same classes and the same matching as import.update (by DXF handle, else by content, geometry and nearest position; renumbered handles are found). `changes` lists the added, removed and changed entities with stable IDs; `object_ids` are the objects of the temporary documents, they are not objects of the open document.",
        "render": "With render the answer has an overlay picture of the new file with the changes: added objects green, changed orange, removed objects as red ghosts at their old place (palette color_blind: blue, vermillion, dark yellow), unchanged objects dimmed (unchanged: show, dim or hide). Color is never the only sign: added is solid with +, changed dotted with ~, removed dashed with -. Formats: svg (the plan, with a legend), png (top view of the 3D model, tints and ghosts), pdf (one page that holds the picture of the png, 150 dpi; the text of the legend is not in it, the legend is in `compare.legend`).",
        "revision_clouds": "revision_clouds: true draws a revision cloud (ANN-10) around each cluster of changes: changes less than 4 percent of the size of the drawing apart share a cloud. At most 64 clouds are drawn, the biggest clusters first (`image.clouds` says how many and how many were left out). The clouds exist only in the picture, not in the document.",
        "issues": "REV-04 (review issues) does not exist yet. When it does, an agent can file one issue for each change or cluster from this answer: the change ID, the entity and the layer go into the title and the labels, and the box of `object_ids` anchors the issue. Until then, give the list of changes to the person.",
        "limits": "Both files are read with the limits of import.file (max_bytes, import_text_bytes of the session, max_objects) and at most 1,000,000 entities each. The match has the bounds of kadakas_io::dxf::revision. A DWG file needs the converter and the policy of import.file. The size of a picture follows the limits of render.png.",
        "examples": [
            {"method": "dxf.compare", "params": {"base": "/data/site_rev1.dxf", "new": "/data/site_rev2.dxf"}},
            {"method": "dxf.compare", "params": {"base": "/data/site_rev1.dwg", "new": "/data/site_rev2.dwg", "options": {"layers": ["TREES"]}, "render": {"format": "png", "revision_clouds": true, "width": 1200, "height": 800}}},
            {"method": "dxf.compare", "params": {"base": "a.dxf", "new": "b.dxf", "render": {"format": "svg", "palette": "color_blind"}}}
        ]
    })
}

#[derive(Deserialize)]
struct CompareParams {
    base: PathBuf,
    new: PathBuf,
    #[serde(default)]
    options: Value,
    render: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageFormat {
    Png,
    Svg,
    Pdf,
}

impl ImageFormat {
    pub(crate) fn parse(name: &str) -> Option<ImageFormat> {
        match name.trim().to_ascii_lowercase().as_str() {
            "png" => Some(ImageFormat::Png),
            "svg" => Some(ImageFormat::Svg),
            "pdf" => Some(ImageFormat::Pdf),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Svg => "svg",
            ImageFormat::Pdf => "pdf",
        }
    }
}

/// The `render` option, checked.
#[derive(Clone, Debug)]
struct RenderSpec {
    format: ImageFormat,
    clouds: bool,
    width: Option<u32>,
    height: Option<u32>,
    view: Option<String>,
    style: CompareStyle,
}

const RENDER_KEYS: &[&str] = &[
    "format",
    "revision_clouds",
    "width",
    "height",
    "view",
    "palette",
    "unchanged",
    "hide_unchanged",
    "dark",
    "ghosts",
    "previous",
];

impl RenderSpec {
    fn parse(v: &Value) -> Result<RenderSpec, RpcError> {
        let m = v
            .as_object()
            .ok_or_else(|| bad("render must be an object: {format: png|svg|pdf, ...}"))?;
        if let Some(k) = m.keys().find(|k| !RENDER_KEYS.contains(&k.as_str())) {
            return Err(bad(format!(
                "unknown render option '{k}'. Use {}",
                RENDER_KEYS.join(", ")
            )));
        }
        let format = m
            .get("format")
            .and_then(Value::as_str)
            .and_then(ImageFormat::parse)
            .ok_or_else(|| bad("render.format is png, svg or pdf"))?;
        let clouds = match m.get("revision_clouds") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(bad("render.revision_clouds must be true or false")),
        };
        let size = |key: &str| -> Result<Option<u32>, RpcError> {
            match m.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .map(Some)
                    .ok_or_else(|| bad(format!("render.{key} must be a whole number of pixels"))),
            }
        };
        let view = match m.get("view") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) => Some(t.clone()),
            Some(_) => return Err(bad("render.view is the name of a view")),
        };
        // The style options are those of the `compare` option of render.png: ask that parser.
        let mut style_opts: Map<String, Value> = m
            .iter()
            .filter(|(k, _)| {
                matches!(
                    k.as_str(),
                    "palette" | "unchanged" | "hide_unchanged" | "dark" | "ghosts" | "previous"
                )
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        style_opts.insert("from".into(), json!("current"));
        style_opts.insert("to".into(), json!("current"));
        let spec = CompareSpec::parse(Some(&Value::Object(style_opts)))?
            .expect("a compare option that is an object");
        Ok(RenderSpec {
            format,
            clouds,
            width: size("width")?,
            height: size("height")?,
            view,
            style: spec.style,
        })
    }
}

/// A cloud around a cluster, as a revision cloud annotation (for the SVG).
fn annot_cloud(points: &[DVec2], bump: f64, name: &str) -> Shape {
    Shape::Annotation(Box::new(Annot::RevisionCloud(RevisionCloud {
        points: points.to_vec(),
        bump_mm: bump,
        revision: "1".into(),
        date: String::new(),
        description: name.chars().take(80).collect(),
        author: "Kadakas".into(),
        mark: true,
    })))
}

/// The same cloud as a closed polyline (for the 3D renderer, which draws no annotations).
fn line_cloud(points: &[DVec2], bump: f64) -> Shape {
    Shape::Polyline {
        points: cloud_outline(points, bump)
            .into_iter()
            .map(|p| Vertex {
                p,
                ..Vertex::default()
            })
            .collect(),
        closed: true,
    }
}

/// The new document with revision clouds on a layer of their own. The IDs of the clouds follow
/// those of the old file's objects (the document is made from the all document), so they never meet
/// an ID of the overlay.
fn with_clouds(
    c: &Compared,
    found: &Clusters,
    pad: f64,
    bump: f64,
    annotation: bool,
) -> Result<(Document, HashSet<ObjectId>), RpcError> {
    // The layer has a scale, so that the bump of a cloud (1 to 100 mm on paper) is the size we want.
    let scale = (bump / 20.0).max(1.0);
    let bump_paper = bump / scale;
    let mut base = c.all_doc.clone();
    base.objects
        .retain(|id, _| c.new_doc.objects.contains_key(id));
    let mut ed = Editor::new(base);
    let layer = ed
        .execute(&Command::AddLayer {
            name: "Revision clouds".into(),
            elevation: 0.0,
            scale: Some(scale),
            kind: None,
        })
        .map_err(RpcError::from)?
        .created
        .first()
        .copied()
        .ok_or_else(|| RpcError::internal("the layer of the clouds has no ID"))?;
    let mut ids = HashSet::new();
    for cl in &found.clusters {
        let corners = clouds::corners(cl, pad);
        let shape = if annotation {
            annot_cloud(&corners, bump_paper, &format!("{} changes", cl.count))
        } else {
            line_cloud(&corners, bump)
        };
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: Some(kadakas_core::model::LayerId(layer)),
                class: None,
                attrs: Attributes {
                    pen: Some(Color([CLOUD_RGB[0], CLOUD_RGB[1], CLOUD_RGB[2], 255])),
                    line_weight: Some(0.5),
                    ..Attributes::default()
                },
            })
            .map_err(RpcError::from)?;
        ids.extend(out.created.iter().map(|u| ObjectId(*u)));
    }
    Ok((ed.doc().clone(), ids))
}

/// An overlay that wraps the compare overlay and draws the clouds in their own color.
struct Clouded<'a> {
    inner: SvgCompare<'a>,
    clouds: &'a HashSet<ObjectId>,
    tint: SvgTint,
}

impl SvgOverlay for Clouded<'_> {
    fn look(&self, id: ObjectId) -> SvgLook {
        if self.clouds.contains(&id) {
            SvgLook::Tint(self.tint.clone())
        } else {
            self.inner.look(id)
        }
    }

    fn ghosts(&self) -> &[SvgGhost] {
        self.inner.ghosts()
    }

    fn view(&self) -> Option<(DVec2, DVec2)> {
        self.inner.view()
    }

    fn dark(&self) -> bool {
        self.inner.dark()
    }

    fn legend(&self) -> &[SvgLegendRow] {
        self.inner.legend()
    }
}

/// The overlay of a comparison.
fn overlay_of(c: &Compared) -> CompareOverlay {
    let object = |id: &ObjectId| -> Option<Object> { c.all_doc.objects.get(id).cloned() };
    CompareOverlay::from_changes(c.changes.iter().map(|ch| match ch.status {
        Kind::Added => Change {
            id: ch.id,
            status: Status::Added,
            old: None,
            new: None,
        },
        Kind::Changed => Change {
            id: ch.id,
            status: Status::Changed,
            old: ch.old.as_ref().and_then(object),
            new: None,
        },
        Kind::Removed => Change {
            id: ch.id,
            status: Status::Removed,
            old: object(&ch.id),
            new: None,
        },
    }))
}

/// The boxes of the changes, for the clouds: the new objects of added and changed ones, the old
/// object of a removed one.
fn change_boxes(c: &Compared) -> Vec<(DVec2, DVec2)> {
    c.changes
        .iter()
        .filter_map(|ch| {
            let doc = if ch.status == Kind::Removed {
                &c.all_doc
            } else {
                &c.new_doc
            };
            let o = doc.objects.get(&ch.id)?;
            let (lo, hi) = doc.object_bounds(o);
            (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
        })
        .collect()
}

impl Session {
    pub(crate) fn dxf_compare(&mut self, p: Value) -> Result<Value, RpcError> {
        self.compare_impl(p, true)
    }

    /// `dxf.compare` for the user's own file dialog: the path policy for agents does not apply.
    pub fn dxf_compare_trusted(&mut self, p: Value) -> Result<Value, RpcError> {
        self.compare_impl(p, false)
    }

    fn compare_impl(&mut self, p: Value, checked: bool) -> Result<Value, RpcError> {
        let CompareParams {
            base,
            new,
            options,
            render,
        } = params(p)?;
        // The own options of the compare, and the rest are import options.
        let mut rest = options.as_object().cloned().unwrap_or_default();
        if !options.is_null() && !options.is_object() {
            return Err(bad("options must be an object"));
        }
        let tolerance = match rest.remove("tolerance") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_f64()
                    .ok_or_else(|| bad("options.tolerance must be a number"))?,
            ),
        };
        let layers: Vec<String> = match rest.remove("layers") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => serde_json::from_value(v)
                .map_err(|_| bad("options.layers is a list of DXF layer names"))?,
        };
        let limit = match rest.remove("limit") {
            None | Some(Value::Null) => DEFAULT_LISTED,
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=MAX_LISTED as u64).contains(n))
                .map(|n| n as usize)
                .ok_or_else(|| bad(format!("options.limit is a number from 1 to {MAX_LISTED}")))?,
        };
        let spec = render.as_ref().map(RenderSpec::parse).transpose()?;
        let (opts, run) = self.drawing_options(&Value::Object(rest))?;
        let old = self.read_drawing(&base, checked, &opts, &run, false)?;
        let newer = self.read_drawing(&new, checked, &opts, &run, false)?;
        let c = compare::compare(&compare::Request {
            base: (&old.data, &old.name),
            new: (&newer.data, &newer.name),
            options: &opts,
            tolerance,
            layers: &layers,
        })
        .map_err(crate::update_api::update_error)?;
        let mut out = json!({
            "base": {"name": old.name},
            "new": {"name": newer.name},
            "counts": c.counts,
            "changes": c.items.iter().take(limit).collect::<Vec<_>>(),
            "listed": c.items.len().min(limit),
            "total_changes": c.items.len(),
            "notes": c.notes,
            "match": {
                "truncated": c.truncated,
                "renumbered": c.renumbered,
                "tolerance": c.tolerance,
            },
        });
        if let Some(spec) = spec {
            let (image, info) = self.compare_image(&c, &spec)?;
            out["image"] = image;
            out["compare"] = info;
        }
        Ok(out)
    }

    /// The picture of a comparison and the facts about its overlay.
    fn compare_image(
        &mut self,
        c: &Compared,
        spec: &RenderSpec,
    ) -> Result<(Value, Value), RpcError> {
        let overlay = Arc::new(overlay_of(c));
        let style = spec.style;
        let boxes = if spec.clouds {
            change_boxes(c)
        } else {
            Vec::new()
        };
        let gap = clouds::default_gap(&boxes);
        let found = clouds::clusters(&boxes, gap);
        let (pad, bump) = (gap / 3.0, (gap / 2.0).max(1.0));
        let want_clouds = spec.clouds && !found.clusters.is_empty();
        let old = Some(Arc::new(c.all_doc.clone()));
        let dark = style.dark;
        let tint_rgb = if dark { CLOUD_RGB_DARK } else { CLOUD_RGB };
        let mut image = json!({"format": spec.format.name()});
        let report: GhostReport;
        match spec.format {
            ImageFormat::Svg => {
                let (doc, ids) = if want_clouds {
                    with_clouds(c, &found, pad, bump, true)?
                } else {
                    (c.new_doc.clone(), HashSet::new())
                };
                let shown = ShownDoc::Built(Arc::new(doc));
                // The width of the picture is bounded like the size of a PNG.
                let width = self.limits.image_size(spec.width.unwrap_or(1200), 16).0;
                let tint = SvgTint {
                    rgb: tint_rgb,
                    pattern: SvgPattern::Solid,
                    weight_factor: 1.6,
                    fill_alpha: 0.0,
                    opacity: 1.0,
                    symbol: None,
                    status: "revision_cloud",
                };
                let (svg, r) = self.with_shown(&shown, |doc, revision, site, annot| {
                    let inner = SvgCompare::new(doc, old.as_ref(), &overlay, style);
                    let r = inner.report();
                    let ov = Clouded {
                        inner,
                        clouds: &ids,
                        tint,
                    };
                    (
                        crate::svg_of(doc, width, revision, site, annot, Some(&ov), None),
                        r,
                    )
                });
                image["svg"] = json!(svg);
                report = r;
            }
            ImageFormat::Png | ImageFormat::Pdf => {
                let (doc, ids) = if want_clouds {
                    with_clouds(c, &found, pad, bump, false)?
                } else {
                    (c.new_doc.clone(), HashSet::new())
                };
                let shown = ShownDoc::Built(Arc::new(doc));
                let settings =
                    self.resolve_view(Some(spec.view.as_deref().unwrap_or("top")), None, None)?;
                let (width, height) = self
                    .limits
                    .image_size(spec.width.unwrap_or(1200), spec.height.unwrap_or(900));
                let req = PngRequest {
                    settings,
                    width,
                    height,
                    samples: 1,
                };
                let mut tinted = self.tint_shown(&shown, old, &overlay, &style);
                let [r, g, b] = tint_rgb;
                tinted
                    .scene
                    .recolor(&|id, _| ids.contains(&id).then_some([r, g, b, 255]));
                let (img, r) = self.overlay_image(&req, tinted, &style);
                report = r;
                image["width"] = json!(img.width);
                image["height"] = json!(img.height);
                if spec.format == ImageFormat::Png {
                    use base64::Engine;
                    image["png_base64"] =
                        json!(base64::engine::general_purpose::STANDARD.encode(img.to_png()));
                } else {
                    let rgb: Vec<u8> = img
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| [p[0], p[1], p[2]])
                        .collect();
                    let title = format!("{} compared with {}", c.name, c.base_name);
                    let pdf = kadakas_print::image_pdf(rgb, img.width, img.height, PDF_DPI, &title)
                        .ok_or_else(|| RpcError::internal("the picture does not fit a page"))?;
                    use base64::Engine;
                    image["pdf_base64"] =
                        json!(base64::engine::general_purpose::STANDARD.encode(pdf));
                }
            }
        }
        if spec.clouds {
            image["clouds"] = json!({
                "count": found.clusters.len(),
                "dropped": found.dropped,
                "skipped": found.skipped,
            });
        }
        let counts = overlay.counts();
        let info = json!({
            "style": {
                "palette": style.palette.name(),
                "unchanged": style.unchanged.name(),
                "dark": style.dark,
            },
            "counts": {"added": counts.added, "removed": counts.removed, "changed": counts.changed},
            "summary": serde_json::to_value(overlay.summary()).unwrap_or(Value::Null),
            "notes": serde_json::to_value(overlay.notes()).unwrap_or(Value::Null),
            "legend": crate::compare_api::legend(&style),
            "ghosts": serde_json::to_value(report).unwrap_or(Value::Null),
        });
        Ok((image, info))
    }
}
