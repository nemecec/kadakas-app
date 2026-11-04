//! Picking of annotation objects. The code is in `kadakas_annot::annot_pick`.

pub use kadakas_annot::annot_pick::*;

#[cfg(test)]
#[allow(unused_imports)]
use {
    glam::DVec2,
    kadakas_annot::AnnotCache,
    kadakas_core::annot::draw::{Prim, prims_bounds},
    kadakas_core::geom::dist_to_segment,
    kadakas_core::model::{Document, Object},
    std::sync::Arc,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit;
    use kadakas_core::annot::style::{AnnotResource, TagStyle};
    use kadakas_core::annot::{Annot, NorthArrow};
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::{Attributes, ObjectId, Shape};
    use kadakas_core::resource::ResourceBody;

    fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }

    #[test]
    fn a_north_arrow_is_picked_near_its_lines_and_not_far_away() {
        let mut ed = Editor::default();
        let id = add(
            &mut ed,
            Shape::Annotation(Box::new(Annot::NorthArrow(NorthArrow {
                at: DVec2::new(1000.0, 1000.0),
                size_mm: 15.0,
                rotation_deg: 0.0,
                use_georef: false,
                style: Default::default(),
            }))),
        );
        let doc = ed.doc();
        let o = doc.object(id).unwrap();
        let pk = Prims::default();
        let (lo, hi) = pk.bounds(doc, o);
        assert!(lo.x < hi.x && lo.y < hi.y);
        assert!(pk.hit(doc, o, (lo + hi) / 2.0, 1.0) || pk.hit(doc, o, lo, 1.0));
        assert!(!pk.hit(doc, o, hi + DVec2::splat(1.0e6), 1.0));
        // A path that is not an annotation has no primitives, and so no box of its own.
        let line = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        let o = ed.doc().object(line).unwrap();
        assert!(pk.of(ed.doc(), o).is_empty());
        assert_eq!(pk.bounds(ed.doc(), o), o.shape.bounds());
    }

    /// A data tag is drawn from its object and style. Picking must follow the tag when it moves,
    /// with a cache that the test owns and no global set-up.
    #[test]
    fn a_data_tag_is_picked_at_its_new_place_after_it_moves() {
        let mut ed = Editor::default();
        let plant = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 500.0,
            },
        );
        let style = ed
            .execute(&Command::AddResource {
                name: "Tag".into(),
                body: ResourceBody::annot(AnnotResource::TagStyle(TagStyle {
                    lines: vec!["Hello".into()],
                    ..Default::default()
                })),
            })
            .unwrap()
            .created[0];
        let tag = add(
            &mut ed,
            Shape::Annotation(Box::new(Annot::DataTag(kadakas_core::annot::DataTag {
                style: kadakas_core::resource::ResourceId(style),
                object: plant,
                offset: DVec2::new(8.0, 8.0),
                at: DVec2::new(3000.0, 3000.0),
                anchor: DVec2::ZERO,
            }))),
        );
        let cache = AnnotCache::default();
        let center = |ed: &Editor| {
            let pk = Prims::cached(&cache, ed.revision());
            // The middle of the text of the tag; the leader line from the object is not needed.
            let prims = pk.of(ed.doc(), ed.doc().object(tag).unwrap());
            let (lo, hi) = prims
                .iter()
                .find(|p| matches!(p, Prim::Text { .. }))
                .expect("the tag has text")
                .bounds();
            (lo + hi) / 2.0
        };
        let before = center(&ed);
        let pick = |ed: &Editor, p: DVec2| {
            let pk = Prims::cached(&cache, ed.revision());
            hit::hit_object(pk, ed.doc(), p, 5.0)
        };
        assert_eq!(pick(&ed, before), Some(tag));
        // Move the tag far to the right.
        let delta = DVec2::new(20_000.0, 0.0);
        ed.execute(&Command::MoveObjects {
            ids: vec![tag],
            delta,
        })
        .unwrap();
        assert_eq!(
            pick(&ed, before + delta),
            Some(tag),
            "picked at the new place"
        );
        assert_ne!(pick(&ed, before), Some(tag), "not at the old place");
    }
}
