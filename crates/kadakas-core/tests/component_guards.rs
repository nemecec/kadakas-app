//! Guards of the walks over component definitions (MOD-07): the cost of a walk is bounded by the
//! size of the definition graph and not by the number of paths in it, a visited-node budget
//! refuses what is too big, the total of models over all instances, the size of a definition
//! and the cost of an explode are bounded. Every limit is tested at the exact value (passes) and
//! at the value plus one (refused).

#[path = "limits_common.rs"]
mod limits_common;

use std::time::Duration;

use glam::{DAffine3, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::component::{self, ComponentError, Part};
use kadakas_core::graphics::{GraphicResource, SymbolDef};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::surface::{Edge, EdgeId, SurfaceModel, VertexId};
use limits_common::{def_parts, instance_shape, push_def, push_instances};

// ------------------------------------------------------------------ helpers

/// A chain of `levels` definitions in which definition i holds `fan` instances of definition
/// i-1. The bottom definition is one empty group, so the chain has no model at all, but the
/// number of paths through it is `fan` to the power `levels`.
fn empty_chain(levels: usize, fan: usize) -> (Document, ResourceId, Vec<ObjectId>) {
    let mut doc = Document::with_sequential_ids();
    let mut def = push_def(&mut doc, "D0", def_parts("empty", 1));
    for lvl in 1..=levels {
        let parts: Vec<Part> = (0..fan)
            .map(|k| Part::new(instance_shape(def, k)))
            .collect();
        def = push_def(&mut doc, &format!("D{lvl}"), parts);
    }
    let ids = push_instances(&mut doc, def, 3);
    (doc, def, ids)
}

/// Runs `f` and fails if it takes more than five times `limit` of CPU time. The old walk took
/// 20 s and more, so the generous bound still finds a hang, and a busy machine does not fail it.
fn under(what: &str, limit: Duration, f: impl FnOnce()) {
    let start = limits_common::cpu();
    f();
    let took = limits_common::cpu() - start;
    let bound = 5.0 * limit.as_secs_f64();
    assert!(
        took < bound,
        "{what} took {took:.2} s of CPU time, the bound is {bound} s"
    );
}

fn symbol_body(parts: Vec<Part>) -> ResourceBody {
    ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
        parts,
        ..SymbolDef::default()
    })))
}

fn add_resource(ed: &mut Editor, name: &str, parts: Vec<Part>) -> Result<ResourceId, String> {
    ed.execute(&Command::AddResource {
        name: name.into(),
        body: symbol_body(parts),
    })
    .map(|o| ResourceId(*o.created.last().expect("added")))
    .map_err(|e| e.to_string())
}

fn add_instance(ed: &mut Editor, symbol: ResourceId, k: usize) -> Result<ObjectId, String> {
    ed.execute(&Command::AddObject {
        shape: instance_shape(symbol, k),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .map(|o| ObjectId(o.created[0]))
    .map_err(|e| e.to_string())
}

/// `n` empty surface models (they count as models, and draw nothing).
fn empty_models(n: usize) -> Vec<Part> {
    (0..n)
        .map(|_| Part::new(Shape::SurfaceModel(Box::new(SurfaceModel::new()))))
        .collect()
}

/// A model of `k` loose edges (1 m long, each apart from the others): `k` edges and `2k` vertices.
fn loose_edges(k: usize) -> SurfaceModel {
    let mut vertices = std::collections::BTreeMap::new();
    let mut edges = std::collections::BTreeMap::new();
    let mut next = 1u32;
    for i in 0..k {
        let (x, y) = ((i % 100) as f64 * 2000.0, (i / 100) as f64 * 2000.0);
        let (a, b) = (VertexId(next), VertexId(next + 1));
        vertices.insert(a, DVec3::new(x, y, 0.0));
        vertices.insert(b, DVec3::new(x + 1000.0, y, 0.0));
        edges.insert(EdgeId(next + 2), Edge { v: [a, b] });
        next += 3;
    }
    SurfaceModel::from_parts(vertices, edges, Default::default(), next)
}

fn model_part(m: SurfaceModel) -> Part {
    Part::new(Shape::SurfaceModel(Box::new(m)))
}

/// Parts with exactly `edges` edges: models of loose edges, 25,000 at most in a model.
fn parts_with_edges(edges: usize) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut left = edges;
    while left > 0 {
        let k = left.min(25_000);
        parts.push(model_part(loose_edges(k)));
        left -= k;
    }
    parts
}

// ---------------------------------------------------------- 1. the walks

#[test]
fn a_chain_of_empty_definitions_is_not_walked_path_by_path() {
    // 8 instances per level, 10 levels: 8^10 = 1 billion paths, 22 s with the plain walk.
    let (doc, top, ids) = empty_chain(10, 8);
    under("every walk of the chain", Duration::from_secs(1), || {
        assert!(matches!(
            component::leaves(&doc.resources, top),
            Err(ComponentError::TooBig { .. })
        ));
        let _ = component::definition_depth(&doc.resources, top);
        assert!(component::version(&doc.resources, top).is_none());
        assert!(component::definition_bounds(&doc.resources, top).is_none());
        assert!(!component::definition_findings(&doc).is_empty());
        assert_eq!(component::findings(&doc).len(), 3);
        let shape = &doc.object(ids[0]).unwrap().shape;
        assert!(matches!(
            component::plan_mark(&doc, shape),
            Some(component::PlanMark::Broken(..))
        ));
        let scene = mesh::build(&doc);
        assert_eq!(scene.lines.segment_count(), 9, "a cross for each of 3");
    });
}

#[test]
fn the_levels_6_8_10_12_all_finish_at_once() {
    for levels in [6, 8, 10, 12] {
        let (doc, top, _) = empty_chain(levels, 8);
        under(&format!("{levels} levels"), Duration::from_secs(1), || {
            let r = component::leaves(&doc.resources, top);
            let _ = component::definition_depth(&doc.resources, top);
            let _ = component::findings(&doc);
            // 8^6 = 262,144 visited parts are within the budget of a million, 8^7 are not.
            if levels <= 6 {
                assert!(r.is_ok(), "{levels} levels: {:?}", r.err());
            } else {
                assert!(
                    matches!(r, Err(ComponentError::TooBig { .. })),
                    "{levels} levels"
                );
            }
        });
    }
}

#[test]
fn the_budget_of_visited_parts_is_exact() {
    // D0 has 1000 parts. D1 has `n` instances of D0 and `g` groups: it visits
    // (n + g) + n * 1000 parts.
    let build = |n: usize, g: usize| {
        let mut doc = Document::with_sequential_ids();
        let d0 = push_def(&mut doc, "Base", def_parts("empty", 1000));
        let mut parts: Vec<Part> = (0..n).map(|k| Part::new(instance_shape(d0, k))).collect();
        parts.extend(def_parts("empty", g));
        let top = push_def(&mut doc, "Top", parts);
        (doc, top)
    };
    // 1000 + 999 * 1000 = 1,000,000: passes.
    let (doc, top) = build(999, 1);
    assert_eq!(component::MAX_VISITED, 1_000_000);
    let e = component::ExpansionCache::new(&doc.resources)
        .get(top)
        .unwrap();
    assert_eq!(e.nodes, 1_000_000);
    assert!(component::leaves(&doc.resources, top).unwrap().is_empty());
    assert!(component::findings(&doc).is_empty());
    // One more part: 1,000,001, refused, with the name of the definition and the numbers.
    let (doc, top) = build(999, 2);
    let err = component::leaves(&doc.resources, top).unwrap_err();
    assert!(
        matches!(&err, ComponentError::TooBig { name, max: 1_000_000 } if name == "Top"),
        "{err:?}"
    );
    let text = err.to_string();
    assert!(
        text.contains("'Top'") && text.contains("1,000,000"),
        "{text}"
    );
}

#[test]
fn a_command_that_makes_a_chain_over_the_budget_is_refused_and_changes_nothing() {
    let mut ed = Editor::with_sequential_ids();
    let mut def = add_resource(&mut ed, "D0", def_parts("empty", 1)).unwrap();
    let t = limits_common::cpu();
    let mut refused = None;
    for lvl in 1..=10 {
        let parts: Vec<Part> = (0..8).map(|k| Part::new(instance_shape(def, k))).collect();
        let before = ed.doc().resources.iter().count();
        match add_resource(&mut ed, &format!("D{lvl}"), parts) {
            Ok(id) => def = id,
            Err(e) => {
                assert_eq!(ed.doc().resources.iter().count(), before, "unchanged");
                refused = Some((lvl, e));
                break;
            }
        }
    }
    assert!(
        limits_common::cpu() - t < 5.0,
        "{} s of CPU time",
        limits_common::cpu() - t
    );
    let (lvl, e) = refused.expect("the chain is refused");
    assert_eq!(lvl, 7, "8^7 visited parts are over the budget");
    assert!(e.contains("'D7'") && e.contains("1,000,000"), "{e}");
}

#[test]
fn the_other_commands_do_not_walk_the_chain_either() {
    // A definition over the budget came with a file. Explode, make unique and edit run at once.
    let (doc, top, ids) = empty_chain(10, 8);
    let mut ed = Editor::new(doc);
    under("the commands", Duration::from_secs(1), || {
        // The instance cannot be edited or made unique, and says why. Explode moves its parts up.
        let _ = ed.execute(&Command::MakeUnique { ids: vec![ids[0]] });
        let _ = ed.execute(&Command::EditComponent { id: ids[1] });
        let _ = ed.execute(&Command::Explode { ids: vec![ids[2]] });
        // A new definition on top of it is refused.
        let parts = vec![Part::new(instance_shape(top, 0))];
        let e = add_resource(&mut ed, "Above", parts).unwrap_err();
        assert!(e.contains("over a limit"), "{e}");
    });
}

#[test]
fn a_file_with_the_chain_loads_at_once_with_a_finding_and_draws_placeholders() {
    let (doc, _top, ids) = empty_chain(10, 8);
    let text = limits_common::save_text(&doc);
    under("load and draw", Duration::from_secs(2), || {
        let loaded = Document::from_json(&text).expect("loads");
        assert!(
            loaded.load_notes().iter().any(|n| n.contains("'D10'")),
            "{:?}",
            loaded.load_notes()
        );
        let broken = component::findings(&loaded);
        assert_eq!(broken.len(), 3);
        assert!(ids.iter().all(|i| broken.iter().any(|f| f.object == *i)));
        let scene = mesh::build(&loaded);
        assert_eq!(scene.lines.segment_count(), 9);
    });
}

// ------------------------------------------------- 2. models over all instances

/// A document with a definition of 10,000 empty models, and `n` instances of it.
fn world_of_10k(n: usize) -> (Editor, ResourceId) {
    let mut doc = Document::with_sequential_ids();
    let sym = push_def(&mut doc, "Big", empty_models(10_000));
    push_instances(&mut doc, sym, n);
    (Editor::new(doc), sym)
}

#[test]
fn the_total_of_models_over_all_instances_is_at_most_250000() {
    assert_eq!(component::MAX_TOTAL_MODELS, 250_000);
    // 25 instances of 10,000 models: exactly 250,000.
    let (mut ed, sym) = world_of_10k(25);
    assert_eq!(component::total_models(ed.doc()), 250_000);
    assert!(component::findings(ed.doc()).is_empty());
    // One more instance: 260,000, refused with a clear text, the document unchanged.
    let (rev, count) = (ed.revision(), ed.doc().objects.len());
    let e = add_instance(&mut ed, sym, 99).unwrap_err();
    assert!(
        e.contains("total models would be 260,000, the limit is 250,000"),
        "{e}"
    );
    assert!(e.contains("'Big'") && e.contains("26 instances"), "{e}");
    assert_eq!((ed.revision(), ed.doc().objects.len()), (rev, count));
    // A definition of one model: 250,001 is refused, and the one that is not an instance is fine.
    let one = add_resource(&mut ed, "One", empty_models(1)).unwrap();
    let e = add_instance(&mut ed, one, 0).unwrap_err();
    assert!(e.contains("total models would be 250,001"), "{e}");
    // After a removal there is room again: 249,999 + 1.
    let victim = *ed.doc().objects.keys().next().unwrap();
    ed.execute(&Command::DeleteObjects { ids: vec![victim] })
        .unwrap();
    assert_eq!(component::total_models(ed.doc()), 240_000);
    add_instance(&mut ed, one, 0).unwrap();
    assert_eq!(component::total_models(ed.doc()), 240_001);
}

#[test]
fn copy_array_and_paste_are_held_to_the_total() {
    let (mut ed, sym) = world_of_10k(24);
    let first = *ed.doc().objects.keys().next().unwrap();
    // One copy: 250,000, fine. A second one: refused.
    ed.execute(&Command::CopyObjects {
        ids: vec![first],
        delta: glam::DVec2::new(1.0, 1.0),
    })
    .unwrap();
    assert_eq!(component::total_models(ed.doc()), 250_000);
    let rev = ed.revision();
    let e = ed
        .execute(&Command::CopyObjects {
            ids: vec![first],
            delta: glam::DVec2::new(2.0, 2.0),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("total models would be 260,000"), "{e}");
    assert_eq!(ed.revision(), rev, "unchanged");
    // An array of 3 more copies (the original stays): refused as a whole.
    let e = ed
        .execute(&Command::ArrayObjects {
            ids: vec![first],
            array: kadakas_core::edit2d::spec::ArrayParams {
                layout: kadakas_core::edit2d::spec::ArrayLayout::Linear {
                    count: 3,
                    offset: Some(glam::DVec2::new(5.0, 0.0)),
                    distance: None,
                    angle: None,
                },
                resize: None,
                rotate: None,
                retain: true,
            },
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("total models would be"), "{e}");
    assert_eq!(ed.revision(), rev);
    // A paste is a batch of AddObject.
    let e = ed
        .execute(&Command::Batch {
            commands: vec![Command::AddObject {
                shape: instance_shape(sym, 5),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            }],
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("total models would be 260,000"), "{e}");
    assert_eq!(component::total_models(ed.doc()), 250_000);
}

#[test]
fn a_change_of_a_definition_is_held_to_the_total() {
    let (mut ed, sym) = world_of_10k(25);
    // Add a model to the definition: 25 * 10,001 = 250,025.
    let mut parts = ed.doc().resources.symbol(sym).unwrap().parts.clone();
    parts.truncate(9_999);
    // One model fewer is fine: 25 * 9,999.
    ed.execute(&Command::ModifyResource {
        id: sym,
        name: None,
        body: Some(symbol_body(parts.clone())),
    })
    .unwrap();
    assert_eq!(component::total_models(ed.doc()), 249_975);
    // Back to 10,000 models (250,000: exactly the limit) passes.
    parts.push(model_part(SurfaceModel::new()));
    ed.execute(&Command::ModifyResource {
        id: sym,
        name: None,
        body: Some(symbol_body(parts.clone())),
    })
    .unwrap();
    assert_eq!(component::total_models(ed.doc()), 250_000);
    let one = add_resource(&mut ed, "One", empty_models(1)).unwrap();
    parts.pop();
    parts.push(Part::new(instance_shape(one, 0)));
    // 9,999 models and a nested instance of 1 model: still 10,000 per instance, passes.
    ed.execute(&Command::ModifyResource {
        id: sym,
        name: None,
        body: Some(symbol_body(parts.clone())),
    })
    .unwrap();
    assert_eq!(component::total_models(ed.doc()), 250_000);
    // Replace a group part with a nested instance of a definition of 2 models: 25 more, refused.
    let two = add_resource(&mut ed, "Two", empty_models(2)).unwrap();
    let mut more = parts.clone();
    more[0] = Part::new(instance_shape(two, 0));
    let rev = ed.revision();
    let e = ed
        .execute(&Command::ModifyResource {
            id: sym,
            name: None,
            body: Some(symbol_body(more)),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("total models would be 250,025"), "{e}");
    assert_eq!(ed.revision(), rev, "unchanged");
    assert_eq!(component::total_models(ed.doc()), 250_000);
}

#[test]
fn closing_an_edit_is_held_to_the_total() {
    let (mut ed, _sym) = world_of_10k(25);
    let inst = *ed.doc().objects.keys().next().unwrap();
    let out = ed.execute(&Command::EditComponent { id: inst }).unwrap();
    let group = ObjectId(out.created[0]);
    // Take one model out and put an instance of a component of 2 models in: 10,001 models in the
    // definition, 250,025 in all.
    let two = add_resource(&mut ed, "Two", empty_models(2)).unwrap();
    let victim = ed.doc().children(group)[0];
    ed.execute(&Command::DeleteObjects { ids: vec![victim] })
        .unwrap();
    ed.execute(&Command::AddObjectIn {
        parent: group,
        shape: instance_shape(two, 0),
        class: None,
        attrs: Attributes::default(),
        id: None,
    })
    .unwrap();
    let rev = ed.revision();
    let e = ed
        .execute(&Command::CloseComponent {
            group,
            resolve: Some(component::Resolve::Overwrite),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("total models would be 250,025"), "{e}");
    assert_eq!(ed.revision(), rev, "unchanged: the edit is still open");
    assert!(ed.doc().object(group).is_some());
    // With an instance of one model instead the close is exactly at the limit and passes.
    let one = add_resource(&mut ed, "One", empty_models(1)).unwrap();
    let extra = *ed.doc().children(group).last().unwrap();
    ed.execute(&Command::DeleteObjects { ids: vec![extra] })
        .unwrap();
    ed.execute(&Command::AddObjectIn {
        parent: group,
        shape: instance_shape(one, 0),
        class: None,
        attrs: Attributes::default(),
        id: None,
    })
    .unwrap();
    ed.execute(&Command::CloseComponent {
        group,
        resolve: Some(component::Resolve::Overwrite),
    })
    .unwrap();
    assert_eq!(component::total_models(ed.doc()), 250_000);
}

#[test]
fn make_component_does_not_add_models_when_it_replaces() {
    let (mut ed, sym) = world_of_10k(24);
    // Two instances become one definition with two nested instances: the total stays.
    let ids: Vec<ObjectId> = ed.doc().objects.keys().take(2).copied().collect();
    let before = component::total_models(ed.doc());
    ed.execute(&Command::MakeComponent {
        name: "Pair".into(),
        model: None,
        faces: vec![],
        edges: vec![],
        objects: ids,
        replace: true,
    })
    .unwrap();
    assert_eq!(component::total_models(ed.doc()), before);
    // At the limit, one more instance is refused.
    add_instance(&mut ed, sym, 1).unwrap();
    assert_eq!(component::total_models(ed.doc()), 250_000);
    assert!(add_instance(&mut ed, sym, 2).is_err());
}

#[test]
fn undo_and_redo_keep_the_total_true() {
    let (mut ed, sym) = world_of_10k(24);
    add_instance(&mut ed, sym, 1).unwrap();
    assert!(add_instance(&mut ed, sym, 2).is_err());
    assert!(ed.undo());
    assert_eq!(component::total_models(ed.doc()), 240_000);
    add_instance(&mut ed, sym, 3).unwrap();
    assert!(ed.undo());
    assert!(ed.redo());
    assert_eq!(component::total_models(ed.doc()), 250_000);
    assert!(add_instance(&mut ed, sym, 4).is_err());
}

#[test]
fn a_document_over_the_total_loads_with_a_finding_and_draws_placeholders() {
    // 26 instances of 10,000 models, as a file (or another user) can bring.
    let (ed, _) = world_of_10k(26);
    let text = limits_common::save_text(ed.doc());
    let loaded = Document::from_json(&text).expect("loads, nothing is cut off");
    assert_eq!(loaded.objects.len(), 26);
    let notes = loaded.load_notes().join("\n");
    assert!(
        notes.contains("260,000 models") && notes.contains("limit is 250,000"),
        "{notes}"
    );
    assert!(
        notes.contains("1 instances are drawn as placeholders"),
        "{notes}"
    );
    // The finding names the last instance.
    let found = component::findings(&loaded);
    assert_eq!(found.len(), 1);
    assert_eq!(Some(&found[0].object), loaded.objects.keys().last());
    // The scene draws a cross for that instance (3 lines) and nothing for the empty models.
    let scene = mesh::build(&loaded);
    assert_eq!(scene.lines.segment_count(), 3);
    assert!(
        scene
            .lines
            .colors
            .iter()
            .all(|c| *c == component::WARNING_COLOR)
    );
    // Editing a document that is over the limit is not blocked, unless it makes the total larger.
    let mut ed = Editor::new(loaded);
    let first = *ed.doc().objects.keys().next().unwrap();
    ed.execute(&Command::MoveObjects {
        ids: vec![first],
        delta: glam::DVec2::new(10.0, 0.0),
    })
    .expect("a move adds no model");
    let sym = match &ed.doc().object(first).unwrap().shape {
        Shape::Component { symbol, .. } => *symbol,
        _ => unreachable!(),
    };
    assert!(add_instance(&mut ed, sym, 1).is_err(), "it adds models");
    // Removing one makes room.
    let last = *ed.doc().objects.keys().last().unwrap();
    ed.execute(&Command::DeleteObjects { ids: vec![last] })
        .unwrap();
    assert_eq!(component::total_models(ed.doc()), 250_000);
}

#[test]
fn restoring_a_version_over_the_total_is_a_finding_and_not_a_refusal() {
    let (mut ed, _sym) = world_of_10k(25);
    let (big, _) = world_of_10k(27);
    let target = big.doc().clone();
    assert!(ed.restore_content(&target, None).unwrap());
    assert_eq!(ed.doc().objects.len(), 27);
    assert!(
        ed.doc()
            .load_notes()
            .iter()
            .any(|n| n.contains("270,000 models")),
        "{:?}",
        ed.doc().load_notes()
    );
    assert_eq!(component::findings(ed.doc()).len(), 2);
}

#[test]
fn remote_content_over_the_total_is_a_finding_and_not_a_refusal() {
    let (mut ed, _sym) = world_of_10k(25);
    let (big, _) = world_of_10k(27);
    ed.apply_remote(big.doc().clone()).expect("a change");
    assert_eq!(ed.doc().objects.len(), 27);
    assert!(
        ed.doc()
            .load_notes()
            .iter()
            .any(|n| n.contains("270,000 models")),
        "{:?}",
        ed.doc().load_notes()
    );
}

// ------------------------------------------------------- 3. size of a definition

#[test]
fn a_definition_has_at_most_100000_edges_in_all_its_parts() {
    assert_eq!(component::MAX_DEFINITION_EDGES, 100_000);
    let mut ed = Editor::with_sequential_ids();
    // Exactly 100,000: passes. The parts are four models of 25,000 loose edges.
    let ok = add_resource(&mut ed, "Full", parts_with_edges(100_000)).unwrap();
    assert_eq!(
        component::definition_edges(&ed.doc().resources.symbol(ok).unwrap().parts),
        100_000
    );
    // 100,001: refused, and nothing is added.
    let count = ed.doc().resources.iter().count();
    let e = add_resource(&mut ed, "Over", parts_with_edges(100_001)).unwrap_err();
    assert!(
        e.contains("at most 100,000 edges") && e.contains("100,001"),
        "{e}"
    );
    assert_eq!(ed.doc().resources.iter().count(), count);
    // A change of the definition is held to the same size.
    let rev = ed.revision();
    let e = ed
        .execute(&Command::ModifyResource {
            id: ok,
            name: None,
            body: Some(symbol_body(parts_with_edges(100_001))),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("100,001"), "{e}");
    assert_eq!(ed.revision(), rev);
    ed.execute(&Command::ModifyResource {
        id: ok,
        name: None,
        body: Some(symbol_body(parts_with_edges(99_999))),
    })
    .unwrap();
}

#[test]
fn closing_an_edit_is_held_to_the_size_of_a_definition() {
    let mut ed = Editor::with_sequential_ids();
    let sym = add_resource(&mut ed, "Full", parts_with_edges(99_990)).unwrap();
    let inst = add_instance(&mut ed, sym, 0).unwrap();
    let out = ed.execute(&Command::EditComponent { id: inst }).unwrap();
    let group = ObjectId(out.created[0]);
    let add = |ed: &mut Editor, k: usize| {
        ed.execute(&Command::AddObjectIn {
            parent: group,
            shape: Shape::SurfaceModel(Box::new(loose_edges(k))),
            class: None,
            attrs: Attributes::default(),
            id: None,
        })
        .unwrap();
    };
    // 99,990 + 11 = 100,001: refused.
    add(&mut ed, 11);
    let rev = ed.revision();
    let e = ed
        .execute(&Command::CloseComponent {
            group,
            resolve: Some(component::Resolve::Overwrite),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("at most 100,000 edges"), "{e}");
    assert_eq!(ed.revision(), rev, "unchanged: the edit is still open");
    // Keep as new is held to the same size.
    let e = ed
        .execute(&Command::CloseComponent {
            group,
            resolve: Some(component::Resolve::KeepAsNew),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("at most 100,000 edges"), "{e}");
    // Take the 11 away and put 10 in: exactly 100,000, the close passes.
    let extra = *ed.doc().children(group).last().unwrap();
    ed.execute(&Command::DeleteObjects { ids: vec![extra] })
        .unwrap();
    add(&mut ed, 10);
    ed.execute(&Command::CloseComponent {
        group,
        resolve: Some(component::Resolve::Overwrite),
    })
    .unwrap();
}

/// Parts whose saved text is exactly `bytes` long, with no edge: one model whose name is padded.
fn parts_with_text(bytes: usize) -> Vec<Part> {
    let mut parts = vec![model_part(SurfaceModel::new())];
    let Shape::SurfaceModel(m) = &mut parts[0].shape else {
        unreachable!()
    };
    // A model with a name of one character; each more character is one more byte.
    m.name = "x".into();
    let base = component::definition_bytes(&parts);
    assert!(bytes >= base);
    let Shape::SurfaceModel(m) = &mut parts[0].shape else {
        unreachable!()
    };
    m.name = "x".repeat(bytes - base + 1);
    assert_eq!(component::definition_bytes(&parts), bytes);
    parts
}

#[test]
fn a_definition_has_at_most_16_mib_of_saved_text() {
    assert_eq!(component::MAX_DEFINITION_BYTES, 16 * 1024 * 1024);
    let mut ed = Editor::with_sequential_ids();
    let exact = component::MAX_DEFINITION_BYTES;
    let ok = add_resource(&mut ed, "Text", parts_with_text(exact)).unwrap();
    let count = ed.doc().resources.iter().count();
    let e = add_resource(&mut ed, "Over", parts_with_text(exact + 1)).unwrap_err();
    assert!(e.contains("at most 16 MiB"), "{e}");
    assert_eq!(ed.doc().resources.iter().count(), count);
    let rev = ed.revision();
    assert!(
        ed.execute(&Command::ModifyResource {
            id: ok,
            name: None,
            body: Some(symbol_body(parts_with_text(exact + 1))),
        })
        .is_err()
    );
    assert_eq!(ed.revision(), rev);
}

#[test]
fn a_definition_over_the_size_in_a_file_loads_with_a_finding() {
    let mut doc = Document::with_sequential_ids();
    let sym = push_def(&mut doc, "Fat", parts_with_edges(100_001));
    push_instances(&mut doc, sym, 1);
    let thin = push_def(
        &mut doc,
        "Text",
        parts_with_text(component::MAX_DEFINITION_BYTES + 1),
    );
    push_instances(&mut doc, thin, 1);
    let text = limits_common::save_text(&doc);
    let loaded = Document::from_json(&text).expect("loads, nothing is cut off");
    let notes = loaded.load_notes().join("\n");
    assert!(
        notes.contains("'Fat'") && notes.contains("100,001"),
        "{notes}"
    );
    assert!(
        notes.contains("'Text'") && notes.contains("16 MiB"),
        "{notes}"
    );
    assert_eq!(loaded.resources.iter().count(), 2, "the definitions stay");
}

// A small anchor so that the imports above stay used in every build.
#[allow(dead_code)]
fn _keep(_: DAffine3) {}

// ------------------------------------------------------------------ 4. explode

fn grid_part(n: usize, ox: f64) -> Part {
    model_part(limits_common::construct(&limits_common::grid_mesh(
        n, ox, 0.0, 0.0,
    )))
}

/// A document with one instance of a definition with the given parts.
fn explode_world(parts: Vec<Part>) -> (Editor, ObjectId) {
    let mut doc = Document::with_sequential_ids();
    let sym = push_def(&mut doc, "Parts", parts);
    let ids = push_instances(&mut doc, sym, 1);
    (Editor::new(doc), ids[0])
}

#[test]
fn an_explode_of_more_than_20000_faces_is_refused_and_changes_nothing() {
    assert_eq!(component::MAX_EXPLODE_FACES, 20_000);
    // 10,000 + 10,000 + 1 faces.
    let (mut ed, inst) = explode_world(vec![
        grid_part(100, 0.0),
        grid_part(100, 200_000.0),
        grid_part(1, 400_000.0),
    ]);
    let (rev, n) = (ed.revision(), ed.doc().objects.len());
    let t = limits_common::cpu();
    let e = ed
        .execute(&Command::Explode { ids: vec![inst] })
        .unwrap_err()
        .to_string();
    assert!(limits_common::cpu() - t < 5.0);
    assert!(
        e.contains("would weld 20,001 faces, the limit is 20,000") && e.contains("smaller groups"),
        "{e}"
    );
    assert_eq!((ed.revision(), ed.doc().objects.len()), (rev, n));
}

#[test]
#[ignore = "slow in a debug build (the real weld of 20,000 faces); run it with --ignored"]
fn an_explode_of_exactly_20000_faces_runs() {
    let (mut ed, inst) = explode_world(vec![grid_part(100, 0.0), grid_part(100, 200_000.0)]);
    ed.execute(&Command::Explode { ids: vec![inst] })
        .expect("20,000 faces are within the limit");
    let models: Vec<usize> = ed
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::SurfaceModel(m) => Some(m.face_count()),
            _ => None,
        })
        .collect();
    assert_eq!(models, vec![20_000]);
}

/// `n` one-face parts in a row.
fn small_parts(n: usize) -> Vec<Part> {
    (0..n).map(|i| grid_part(1, i as f64 * 2_000.0)).collect()
}

#[test]
fn an_explode_of_more_than_2000_parts_is_refused_and_changes_nothing() {
    assert_eq!(component::MAX_EXPLODE_PARTS, 2_000);
    // Few faces, many parts: the cost follows the parts, so the part limit refuses it.
    let (mut ed, inst) = explode_world(small_parts(2_001));
    let (rev, n) = (ed.revision(), ed.doc().objects.len());
    let t = limits_common::cpu();
    let e = ed
        .execute(&Command::Explode { ids: vec![inst] })
        .unwrap_err()
        .to_string();
    assert!(
        limits_common::cpu() - t < 5.0,
        "the refusal comes before any work"
    );
    assert!(
        e.contains("would weld 2,001 parts, the limit is 2,000") && e.contains("smaller groups"),
        "{e}"
    );
    assert_eq!((ed.revision(), ed.doc().objects.len()), (rev, n));
}

#[test]
#[ignore = "slow in a debug build (the real weld of 2,000 parts); run it with --ignored"]
fn an_explode_of_exactly_2000_parts_runs() {
    let (mut ed, inst) = explode_world(small_parts(2_000));
    ed.execute(&Command::Explode { ids: vec![inst] })
        .expect("2,000 parts are within the limit");
    let models: Vec<usize> = ed
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::SurfaceModel(m) => Some(m.face_count()),
            _ => None,
        })
        .collect();
    assert_eq!(models, vec![2_000]);
}

#[test]
fn a_group_of_more_than_20000_faces_is_refused_too() {
    let (mut ed, _) = explode_world(vec![]);
    let mut ids = Vec::new();
    for (i, n) in [100usize, 100, 1].iter().enumerate() {
        let m = limits_common::construct(&limits_common::grid_mesh(
            *n,
            i as f64 * 200_000.0,
            0.0,
            0.0,
        ));
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::new(m)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ids.push(ObjectId(out.created[0]));
    }
    let g = ed
        .execute(&Command::Group { ids, name: None })
        .unwrap()
        .created[0];
    let e = ed
        .execute(&Command::Explode {
            ids: vec![ObjectId(g)],
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("20,001 faces"), "{e}");
}

// ------------------------------------------- the running total of models

#[test]
fn commands_that_change_nothing_do_not_count_the_models_again_after_an_undo() {
    let (mut ed, sym) = world_of_10k(3);
    let first = *ed.doc().objects.keys().next().unwrap();
    add_instance(&mut ed, sym, 50).unwrap();
    assert!(ed.undo());
    let counted = ed.total_recounts();
    // A move by nothing changes nothing: no revision, no step.
    for _ in 0..5 {
        let rev = ed.revision();
        ed.execute(&Command::MoveObjects {
            ids: vec![first],
            delta: glam::DVec2::ZERO,
        })
        .unwrap();
        assert_eq!(ed.revision(), rev, "the command changed nothing");
    }
    assert_eq!(ed.total_recounts(), counted, "no walk over all objects");
    assert_eq!(ed.running_total(), Some(component::total_models(ed.doc())));
    // The undo and the redo themselves carry the total on.
    assert!(ed.redo());
    assert!(ed.undo());
    assert_eq!(ed.total_recounts(), counted);
    assert_eq!(ed.running_total(), Some(30_000));
}

#[test]
fn a_redo_that_brings_the_total_over_the_limit_keeps_the_total_and_says_so() {
    // A restore of a version is allowed to go over the limit (it has a finding). Its undo goes
    // back to a document within the limit, and its redo goes over again, by design.
    let (mut ed, _sym) = world_of_10k(25);
    let (big, _) = world_of_10k(27);
    assert!(ed.restore_content(big.doc(), None).unwrap());
    assert_eq!(component::total_models(ed.doc()), 270_000);
    let counted = ed.total_recounts();
    assert!(ed.undo());
    assert_eq!(component::total_models(ed.doc()), 250_000);
    assert!(ed.redo());
    assert_eq!(component::total_models(ed.doc()), 270_000);
    assert_eq!(ed.running_total(), Some(270_000), "the total is kept");
    assert!(
        ed.total_recounts() <= counted + 2,
        "an undo and a redo carry the total on"
    );
    let notes = ed.doc().load_notes().join("\n");
    assert!(notes.contains("270,000 models"), "{notes}");
    // Commands that do not add models still work, and one that adds is refused.
    let first = *ed.doc().objects.keys().next().unwrap();
    ed.execute(&Command::DeleteObjects { ids: vec![first] })
        .unwrap();
}

mod running_total_props {
    use super::*;
    use proptest::prelude::*;

    /// A small world: a big definition (10,000 empty models), a small one (3 models) and a
    /// single-model one, with 20 big instances (200,000 models) and a few small ones.
    fn world() -> (Editor, [ResourceId; 3]) {
        let mut doc = Document::with_sequential_ids();
        let big = push_def(&mut doc, "Big", empty_models(10_000));
        let small = push_def(&mut doc, "Small", empty_models(3));
        let one = push_def(&mut doc, "One", empty_models(1));
        push_instances(&mut doc, big, 20);
        push_instances(&mut doc, small, 6);
        (Editor::new(doc), [big, small, one])
    }

    fn pick(ed: &Editor, n: usize) -> Option<ObjectId> {
        let keys: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
        (!keys.is_empty()).then(|| keys[n % keys.len()])
    }

    fn step(ed: &mut Editor, defs: &[ResourceId; 3], op: u8, n: usize) {
        let target = pick(ed, n);
        let _ = match op % 13 {
            0 => add_instance(ed, defs[0], n).map(|_| ()),
            1 => add_instance(ed, defs[1], n).map(|_| ()),
            2 => add_instance(ed, defs[2], n).map(|_| ()),
            3 => target.map_or(Ok(()), |t| {
                ed.execute(&Command::CopyObjects {
                    ids: vec![t],
                    delta: glam::DVec2::new(5.0, 5.0),
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
            }),
            4 => target.map_or(Ok(()), |t| {
                ed.execute(&Command::ArrayObjects {
                    ids: vec![t],
                    array: kadakas_core::edit2d::spec::ArrayParams {
                        layout: kadakas_core::edit2d::spec::ArrayLayout::Linear {
                            count: 1 + n % 4,
                            offset: Some(glam::DVec2::new(7.0, 0.0)),
                            distance: None,
                            angle: None,
                        },
                        resize: None,
                        rotate: None,
                        retain: true,
                    },
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
            }),
            5 => target.map_or(Ok(()), |t| {
                ed.execute(&Command::DeleteObjects { ids: vec![t] })
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }),
            6 => {
                ed.undo();
                Ok(())
            }
            7 => {
                ed.redo();
                Ok(())
            }
            8 => target.map_or(Ok(()), |t| {
                ed.execute(&Command::MakeComponent {
                    name: format!("C{n}"),
                    model: None,
                    faces: vec![],
                    edges: vec![],
                    objects: vec![t],
                    replace: true,
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
            }),
            9 => target.map_or(Ok(()), |t| {
                ed.execute(&Command::Explode { ids: vec![t] })
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }),
            10 => ed
                .execute(&Command::ModifyResource {
                    id: defs[1],
                    name: None,
                    body: Some(symbol_body(empty_models(1 + n % 6))),
                })
                .map(|_| ())
                .map_err(|e| e.to_string()),
            11 => ed
                .execute(&Command::ModifyResource {
                    id: defs[0],
                    name: None,
                    body: Some(symbol_body(empty_models(9_990 + n % 11))),
                })
                .map(|_| ())
                .map_err(|e| e.to_string()),
            _ => {
                // A command that fails halfway: a batch whose second command is bad.
                ed.execute(&Command::Batch {
                    commands: vec![
                        Command::AddObject {
                            shape: instance_shape(defs[0], n),
                            layer: None,
                            class: None,
                            attrs: Attributes::default(),
                        },
                        Command::DeleteObjects {
                            ids: vec![ObjectId::from_num(999_999)],
                        },
                    ],
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
            }
        };
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn the_running_total_equals_the_count_from_scratch(
            ops in proptest::collection::vec((any::<u8>(), 0usize..1000), 1..30)
        ) {
            let (mut ed, defs) = world();
            for (op, n) in ops {
                step(&mut ed, &defs, op, n);
                let truth = component::total_models(ed.doc());
                if let Some(t) = ed.running_total() {
                    prop_assert_eq!(t, truth, "op {} {}", op % 13, n);
                }
                // The documents that the commands make are within the limit, or were not made
                // larger by the step (a rollback leaves the document as it was).
                prop_assert!(truth <= component::MAX_TOTAL_MODELS + 3 * 10_011,
                    "total {}", truth);
            }
        }
    }
}

// ------------------------------------------------------------ explode, more

#[test]
fn the_faces_of_all_ids_are_added_before_any_work_starts() {
    // Two instances of 10,000 faces and 10,001 faces: each alone is within the limit.
    let mut doc = Document::with_sequential_ids();
    let a = push_def(&mut doc, "A", vec![grid_part(100, 0.0)]);
    let b = push_def(
        &mut doc,
        "B",
        vec![grid_part(100, 0.0), grid_part(1, 400_000.0)],
    );
    let ia = push_instances(&mut doc, a, 1)[0];
    let ib = push_instances(&mut doc, b, 1)[0];
    let mut ed = Editor::new(doc);
    let (rev, n) = (ed.revision(), ed.doc().objects.len());
    let t = limits_common::cpu();
    let e = ed
        .execute(&Command::Explode { ids: vec![ia, ib] })
        .unwrap_err()
        .to_string();
    assert!(limits_common::cpu() - t < 5.0, "refused before any weld");
    assert!(e.contains("would weld 20,001 faces"), "{e}");
    assert_eq!((ed.revision(), ed.doc().objects.len()), (rev, n));
}

#[test]
fn the_faces_that_the_target_already_holds_count_too() {
    // A terrain of 90,738 triangles on the layer, a group of 10,001 faces: 100,739 in one model.
    let mut ed = Editor::with_sequential_ids();
    let add = |ed: &mut Editor, m: SurfaceModel| {
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
    };
    add(
        &mut ed,
        limits_common::construct(&limits_common::terrain_mesh(213, 0.0, 0.0, 0.0)),
    );
    let g1 = add(
        &mut ed,
        limits_common::construct(&limits_common::grid_mesh(100, 900_000.0, 0.0, 0.0)),
    );
    let g2 = add(
        &mut ed,
        limits_common::construct(&limits_common::grid_mesh(1, 2_000_000.0, 0.0, 0.0)),
    );
    let group = ed
        .execute(&Command::Group {
            ids: vec![g1, g2],
            name: None,
        })
        .unwrap()
        .created[0];
    let (rev, n) = (ed.revision(), ed.doc().objects.len());
    let t = limits_common::cpu();
    let e = ed
        .execute(&Command::Explode {
            ids: vec![ObjectId(group)],
        })
        .unwrap_err()
        .to_string();
    assert!(limits_common::cpu() - t < 5.0, "refused before any weld");
    assert!(
        e.contains("a surface model of 100,739 faces, the limit is 100,000"),
        "{e}"
    );
    assert_eq!((ed.revision(), ed.doc().objects.len()), (rev, n));
}

#[test]
fn models_inside_a_nested_group_are_not_welded_and_not_counted() {
    // The top of the definition: a group, and inside it a model of 24,964 faces. An explode
    // moves the group up and welds nothing.
    let mut group = Part::new(Shape::Group { name: "g".into() });
    group.parent = None;
    let mut inner = grid_part(158, 0.0);
    inner.parent = Some(0);
    let (mut ed, inst) = explode_world(vec![group, inner]);
    ed.execute(&Command::Explode { ids: vec![inst] })
        .expect("nothing is welded");
}

#[test]
fn many_small_parts_in_several_instances_weld_into_one_model() {
    let mut doc = Document::with_sequential_ids();
    let a = push_def(&mut doc, "A", def_parts("sq", 600));
    let b = push_def(&mut doc, "B", def_parts("sq", 400));
    let ia = push_instances(&mut doc, a, 1)[0];
    let mut ids = vec![ia];
    // The second instance is far from the first, so that the squares do not overlap.
    let ib = ObjectId(doc.alloc_id());
    doc.objects.insert(
        ib,
        Object::new(
            ib,
            FIRST_LAYER,
            NONE_CLASS,
            Shape::Component {
                symbol: b,
                transform: component::raw(&DAffine3::from_translation(DVec3::new(0.0, 5.0e6, 0.0))),
            },
        ),
    );
    ids.push(ib);
    let mut ed = Editor::new(doc);
    ed.execute(&Command::Explode { ids }).unwrap();
    let faces: Vec<usize> = ed
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::SurfaceModel(m) => Some(m.face_count()),
            _ => None,
        })
        .collect();
    assert_eq!(faces.iter().sum::<usize>(), 1_000);
}

#[test]
#[ignore = "slow in a debug build (7 s in a release build); run it with --ignored"]
fn thousands_of_small_parts_near_the_limit_weld_into_one_model() {
    // 3,333 cubes: 19,998 faces, two faces under the limit.
    let (mut ed, inst) = explode_world(def_parts("cube", 3_333));
    ed.execute(&Command::Explode { ids: vec![inst] }).unwrap();
    let faces: Vec<usize> = ed
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::SurfaceModel(m) => Some(m.face_count()),
            _ => None,
        })
        .collect();
    assert_eq!(faces, vec![19_998]);
}

#[test]
fn twenty_thousand_definitions_are_counted_in_linear_time() {
    // Every definition holds a group and an instance of the one before it (chains of 10).
    let mut doc = Document::with_sequential_ids();
    let mut prev: Option<ResourceId> = None;
    for i in 0..20_000usize {
        let mut parts = def_parts("empty", 1);
        if let (Some(p), true) = (prev, i % 10 != 0) {
            parts.push(Part::new(instance_shape(p, 0)));
        }
        prev = Some(push_def(&mut doc, &format!("D{i}"), parts));
    }
    under(
        "counting 20,000 definitions",
        Duration::from_secs(1),
        || {
            assert!(component::definition_findings(&doc).is_empty());
        },
    );
    // The index follows the changes of the list.
    let mut res = doc.resources.clone();
    let last = res.iter().last().unwrap().id;
    assert!(res.get(last).is_some());
    res.remove(last);
    assert!(res.get(last).is_none());
    assert!(res.get_mut(last).is_none());
    let first = res.iter().next().unwrap().id;
    res.get_mut(first).unwrap().name = "renamed".into();
    assert_eq!(res.get(first).unwrap().name, "renamed");
}
