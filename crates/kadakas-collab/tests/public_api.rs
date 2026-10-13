//! The property model moved to `kadakas-props`, and `kadakas-collab` re-exports it. The server
//! (the `kadakas-server` repository) uses these paths, so they must stay.

use kadakas_collab::{Kind, Materialized, Op, Quarantined, Replica, Target, Txn, Writer};
use kadakas_core::model::Document;

#[test]
fn the_old_paths_still_resolve() {
    // At the crate root: the function `materialize` and the module of the same name.
    let build: fn(&Replica, &Document) -> Materialized = kadakas_collab::materialize;
    let _with = kadakas_collab::materialize_with;
    let _module_fn = kadakas_collab::materialize::materialize_with;
    let built = build(
        &kadakas_collab::docsync::replica_of(&Document::default()),
        &Document::default(),
    );
    assert!(built.quarantined.is_empty());
    let _: Vec<Quarantined> = built.quarantined;

    // The modules and some of their items.
    let _ = kadakas_collab::blob::sha256_hex(b"");
    let _ = kadakas_collab::order::valid_key("a0");
    let _ = kadakas_collab::validate::validate_ops;
    let _ = kadakas_collab::op::validate_ops;
    let _ = kadakas_collab::op::MAX_OPS;
    let _ = kadakas_collab::replica::same_json;
    let _ = kadakas_collab::docsync::ops_from_change;
    let _ = kadakas_collab::docsync::ops_from_diff;
    let _ = kadakas_collab::docsync::diff_replicas;
    let _ = kadakas_collab::docsync::split;
    let target = Target::DOCUMENT;
    let _ = (Kind::ALL, Writer::BASELINE, target);
    let _: Option<(Op, Txn)> = None;
    // The rest of the crate.
    let _ = kadakas_collab::PROTOCOL_VERSION;
}
