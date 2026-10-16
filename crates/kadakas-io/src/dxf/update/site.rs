//! Sources inside site models (IOP-23).
//!
//! `site.create` moves 3D loci and 3D polylines into a site model as its source data. The model
//! keeps their import tags (`provenance::SiteSources`, in `data.import_sources`), one for each
//! point and line. An update from a new revision treats such a source like an object: it has a
//! tag, so it can be matched with an entity, replaced by the new one, removed or edited by the
//! person (the tag holds the state of the source at the import). A new locus or polyline on the
//! DXF layer of the sources of a model joins that model. The update then changes the model, and
//! the terrain, the contours and the cut and fill follow.
//!
//! The planner gives each tagged source an ID of its own (a *virtual object*, made from the ID of
//! the site model and the place of the source) so that the matching code, which works with object
//! IDs, needs no other change. The IDs exist only for the plan and the build of one document.

use std::collections::{BTreeMap, BTreeSet};

use kadakas_core::DVec3;
use kadakas_core::command::Command;
use kadakas_core::model::{Document, ObjectId, Shape, SiteLine};
use kadakas_core::provenance::{self, ImportTag, SiteSources, SourceRef};
use kadakas_core::uid::Uid;

/// A source point or a source line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Point,
    Line,
}

/// A tagged source of a site model, as the planner sees it.
#[derive(Clone, Debug)]
pub(super) struct Source {
    pub site: ObjectId,
    pub kind: Kind,
    pub index: usize,
    /// The DXF layer of the entity (the layer or class name of the object that it came from).
    pub dxf_layer: String,
    pub tag: ImportTag,
    /// The person edited the source since the import.
    pub edited: bool,
    /// The middle of the box of the source, for the match by position.
    pub center: [f64; 2],
}

/// The tagged sources by virtual ID.
pub(super) type Sources = BTreeMap<ObjectId, Source>;

fn virtual_id(
    doc: &Document,
    taken: &Sources,
    site: ObjectId,
    kind: Kind,
    index: usize,
) -> ObjectId {
    let mut n =
        site.0.as_u128() ^ (((index as u128) + 1) << 24) ^ (u128::from(kind == Kind::Line) << 23);
    loop {
        let id = ObjectId(Uid::from_u128(n));
        if !doc.objects.contains_key(&id) && !taken.contains_key(&id) {
            return id;
        }
        n = n.wrapping_add(1);
    }
}

fn center_of_points(points: &[DVec3]) -> [f64; 2] {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in points {
        lo = [lo[0].min(p.x), lo[1].min(p.y)];
        hi = [hi[0].max(p.x), hi[1].max(p.y)];
    }
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]
}

/// The tagged sources of the site models of the document whose tag is of `drawing`.
pub(super) fn collect(doc: &Document, drawing: &str) -> Sources {
    let mut out = Sources::new();
    for o in doc.objects.values() {
        let Some(src) = SiteSources::read(o) else {
            continue;
        };
        let Shape::SiteModel { points, lines, .. } = &o.shape else {
            continue;
        };
        for (i, r) in src.points.iter().enumerate() {
            if let Some(r) = r.as_ref().filter(|r| r.tag.drawing == drawing) {
                let id = virtual_id(doc, &out, o.id, Kind::Point, i);
                out.insert(
                    id,
                    Source {
                        site: o.id,
                        kind: Kind::Point,
                        index: i,
                        dxf_layer: r.dxf_layer.clone(),
                        tag: r.tag.clone(),
                        edited: r.tag.obj != provenance::point_state(points[i]),
                        center: [points[i].x, points[i].y],
                    },
                );
            }
        }
        for (i, r) in src.lines.iter().enumerate() {
            if let Some(r) = r.as_ref().filter(|r| r.tag.drawing == drawing) {
                let id = virtual_id(doc, &out, o.id, Kind::Line, i);
                out.insert(
                    id,
                    Source {
                        site: o.id,
                        kind: Kind::Line,
                        index: i,
                        dxf_layer: r.dxf_layer.clone(),
                        tag: r.tag.clone(),
                        edited: r.tag.obj != provenance::line_state(&lines[i]),
                        center: center_of_points(&lines[i].points),
                    },
                );
            }
        }
    }
    out
}

/// How many tagged sources the document has for each drawing.
pub(super) fn count(doc: &Document, drawing: &str) -> usize {
    doc.objects
        .values()
        .filter_map(SiteSources::read)
        .flat_map(|s| s.points.into_iter().chain(s.lines))
        .flatten()
        .filter(|r| r.tag.drawing == drawing)
        .count()
}

/// The site model that a new locus or polyline joins, by the lower case DXF layer: the first site
/// model (in drawing order) that has a source from this drawing on that layer.
pub(super) fn homes(doc: &Document, sources: &Sources) -> BTreeMap<String, ObjectId> {
    let mut layers: BTreeMap<ObjectId, BTreeSet<String>> = BTreeMap::new();
    for s in sources.values() {
        layers
            .entry(s.site)
            .or_default()
            .insert(s.dxf_layer.to_lowercase());
    }
    let mut out = BTreeMap::new();
    for o in doc.objects.values() {
        if let Some(set) = layers.get(&o.id) {
            for l in set {
                out.entry(l.clone()).or_insert(o.id);
            }
        }
    }
    out
}

/// A new source for a site model.
#[derive(Clone, Debug)]
pub(super) struct NewSource {
    pub shape: Shape,
    pub tag: ImportTag,
    pub dxf_layer: String,
}

impl NewSource {
    pub fn kind(&self) -> Option<Kind> {
        match self.shape {
            Shape::Locus3d { .. } => Some(Kind::Point),
            Shape::Polyline3d { .. } => Some(Kind::Line),
            _ => None,
        }
    }

    fn entry(&self, revision: &str) -> SourceRef {
        let mut tag = self.tag.clone();
        tag.revision = revision.to_string();
        tag.obj = match &self.shape {
            Shape::Locus3d { p } => provenance::point_state(*p),
            Shape::Polyline3d { points, closed } => provenance::line_state(&SiteLine {
                points: points.clone(),
                closed: *closed,
            }),
            _ => provenance::EDITED_OBJ.to_string(),
        };
        SourceRef {
            tag,
            dxf_layer: self.dxf_layer.clone(),
        }
    }
}

/// What an update does to the sources of one site model.
#[derive(Debug, Default)]
pub(super) struct Work {
    pub replace: Vec<(Kind, usize, NewSource)>,
    pub delete: BTreeSet<(Kind, usize)>,
    pub add: Vec<NewSource>,
    /// Sources that stay, with the identity (handle, hash, block, INSERTs) of their entity in the
    /// new file.
    pub refresh: Vec<(Kind, usize, ImportTag)>,
}

impl Work {
    pub fn is_empty(&self) -> bool {
        self.replace.is_empty()
            && self.delete.is_empty()
            && self.add.is_empty()
            && self.refresh.is_empty()
    }
}

/// The commands that change a site model: its source points and lines, and their tags. The
/// revision goes into the tags of the sources that change.
pub(super) fn commands(
    doc: &Document,
    site: ObjectId,
    work: &Work,
    revision: &str,
) -> Vec<Command> {
    let Some(obj) = doc.object(site) else {
        return Vec::new();
    };
    let Shape::SiteModel {
        points,
        lines,
        settings,
    } = &obj.shape
    else {
        return Vec::new();
    };
    let (mut points, mut lines) = (points.clone(), lines.clone());
    let mut sources =
        SiteSources::read(obj).unwrap_or_else(|| SiteSources::blank(points.len(), lines.len()));
    for (kind, i, new) in &work.replace {
        match (kind, &new.shape) {
            (Kind::Point, Shape::Locus3d { p }) => {
                points[*i] = *p;
                sources.points[*i] = Some(new.entry(revision));
            }
            (
                Kind::Line,
                Shape::Polyline3d {
                    points: pts,
                    closed,
                },
            ) => {
                lines[*i] = SiteLine {
                    points: pts.clone(),
                    closed: *closed,
                };
                sources.lines[*i] = Some(new.entry(revision));
            }
            _ => {}
        }
    }
    for (kind, i, id) in &work.refresh {
        let entry = match kind {
            Kind::Point => sources.points.get_mut(*i),
            Kind::Line => sources.lines.get_mut(*i),
        };
        if let Some(Some(r)) = entry {
            r.tag.kind = id.kind.clone();
            r.tag.handle = id.handle.clone();
            r.tag.hash = id.hash.clone();
            r.tag.block = id.block.clone();
            r.tag.via = id.via.clone();
            r.tag.part = id.part;
        }
    }
    let mut i = 0usize;
    points.retain(|_| {
        let keep = !work.delete.contains(&(Kind::Point, i));
        i += 1;
        keep
    });
    let mut i = 0usize;
    sources.points.retain(|_| {
        let keep = !work.delete.contains(&(Kind::Point, i));
        i += 1;
        keep
    });
    let mut i = 0usize;
    lines.retain(|_| {
        let keep = !work.delete.contains(&(Kind::Line, i));
        i += 1;
        keep
    });
    let mut i = 0usize;
    sources.lines.retain(|_| {
        let keep = !work.delete.contains(&(Kind::Line, i));
        i += 1;
        keep
    });
    for new in &work.add {
        match &new.shape {
            Shape::Locus3d { p } => {
                points.push(*p);
                sources.points.push(Some(new.entry(revision)));
            }
            Shape::Polyline3d {
                points: pts,
                closed,
            } => {
                lines.push(SiteLine {
                    points: pts.clone(),
                    closed: *closed,
                });
                sources.lines.push(Some(new.entry(revision)));
            }
            _ => {}
        }
    }
    let data = if sources.is_empty() {
        Command::SetData {
            id: site,
            set: Default::default(),
            remove: vec![provenance::SITE_SOURCES_KEY.to_string()],
        }
    } else {
        Command::SetData {
            id: site,
            set: [(provenance::SITE_SOURCES_KEY.to_string(), sources.to_value())].into(),
            remove: Vec::new(),
        }
    };
    vec![
        Command::ModifyObject {
            id: site,
            shape: Some(Shape::SiteModel {
                points,
                lines,
                settings: settings.clone(),
            }),
            layer: None,
            class: None,
            attrs: None,
        },
        data,
    ]
}

#[cfg(test)]
mod tests;
