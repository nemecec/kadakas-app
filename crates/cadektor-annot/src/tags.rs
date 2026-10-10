//! Tagging many objects at once, and the list of revisions.

use cadektor_core::annot::{Annot, DataTag, RevisionCloud, ref_point};
use cadektor_core::command::Command;
use cadektor_core::model::{Attributes, Document, LayerId, LayerKind, ObjectId, Shape};
use cadektor_core::resource::ResourceId;
use glam::DVec2;

/// The commands that tag every object that matches the criteria with the tag style.
///
/// `layer` limits the objects to one layer. An object that has a tag of this style already is
/// left out, so the call is safe to repeat. `offset_mm` is the place of the label against the
/// object, in paper mm. Each tag goes on the layer of its object. Objects on locked layers
/// are left out.
pub fn tag_all(
    doc: &Document,
    style: ResourceId,
    criteria: &str,
    layer: Option<LayerId>,
    offset_mm: DVec2,
) -> Result<Vec<Command>, String> {
    if doc.resources.tag_style(style).is_none() {
        return Err(format!("resource {style} is not a tag style"));
    }
    let tagged: std::collections::HashSet<ObjectId> = doc
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Annotation(a) => match &**a {
                Annot::DataTag(t) if t.style == style => Some(t.object),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for obj in crate::select(doc, criteria)? {
        let l = doc.layer(obj.layer);
        if layer.is_some_and(|x| x != obj.layer)
            || tagged.contains(&obj.id)
            || l.is_none_or(|l| l.locked || l.kind != LayerKind::Design)
        {
            continue;
        }
        let Some(anchor) = ref_point(&obj.shape) else {
            continue;
        };
        let paper = l.map_or(1.0, |l| l.scale);
        let tag = DataTag {
            style,
            object: obj.id,
            offset: offset_mm,
            at: anchor + offset_mm * paper,
            anchor,
        };
        out.push(Command::AddObject {
            shape: Shape::Annotation(Box::new(Annot::DataTag(tag))),
            layer: Some(obj.layer),
            class: Some(obj.class),
            attrs: Attributes::default(),
        });
    }
    Ok(out)
}

/// One revision: its mark, the data of its first cloud, and the clouds.
#[derive(Clone, Debug, PartialEq)]
pub struct RevisionInfo {
    pub revision: String,
    pub date: String,
    pub description: String,
    pub author: String,
    pub clouds: Vec<ObjectId>,
}

/// The revisions of the document, sorted by mark. Clouds with the same mark belong together.
/// The date, description and author come from the first cloud that has them.
pub fn revisions(doc: &Document) -> Vec<RevisionInfo> {
    let mut out: Vec<RevisionInfo> = Vec::new();
    for o in doc.objects.values() {
        let Shape::Annotation(a) = &o.shape else {
            continue;
        };
        let Annot::RevisionCloud(RevisionCloud {
            revision,
            date,
            description,
            author,
            ..
        }) = &**a
        else {
            continue;
        };
        let at = match out
            .iter()
            .position(|r| r.revision.eq_ignore_ascii_case(revision))
        {
            Some(i) => i,
            None => {
                out.push(RevisionInfo {
                    revision: revision.clone(),
                    date: String::new(),
                    description: String::new(),
                    author: String::new(),
                    clouds: vec![],
                });
                out.len() - 1
            }
        };
        let r = &mut out[at];
        for (dst, src) in [
            (&mut r.date, date),
            (&mut r.description, description),
            (&mut r.author, author),
        ] {
            if dst.is_empty() {
                dst.clone_from(src);
            }
        }
        r.clouds.push(o.id);
    }
    out.sort_by(|a, b| a.revision.to_lowercase().cmp(&b.revision.to_lowercase()));
    out
}
