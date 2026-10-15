//! Measurements for the bounds of issues (REV-04, REV-07): the time of `issue.list` and
//! `issue.get`, the time to build the shared state of a document with issues and to bring a
//! second user up to date, and the size of snapshot pictures. A measurement, not a test of
//! behavior: it is ignored by default and prints its numbers.
//!
//! ```text
//! KADAKAS_BENCH_SCALES=1000:10000,10000:99000 \
//!   cargo test -p kadakas-api --release --test issue_bench -- --ignored --nocapture
//! ```
//!
//! `KADAKAS_BENCH_COLLAB=0` leaves out the collaboration part. The limits themselves are chosen
//! with the person, not by this file.

use std::time::Instant;

use kadakas_api::Session;
use kadakas_collab::docsync;
use kadakas_collab::testkit::{Hub, Sequencer};
use kadakas_core::change::Origin;
use kadakas_core::issue::*;
use kadakas_core::model::{Document, LayerId, ObjectId};
use serde_json::{Value, json};

const T0: i64 = 1_700_000_000_000;

fn issue(n: u64) -> Issue {
    Issue {
        id: IssueId::from_num(n + 10),
        title: format!("Issue {n}: a dead tree near the gate"),
        description: "The tree was planted last spring and is dead now. Please replace it with a field maple and check the soil around the root ball.".into(),
        assignee: Some("mari@example.org".into()),
        labels: vec!["planting".into(), "tree".into()],
        author: "ann@example.org".into(),
        status: if n % 3 == 0 { IssueStatus::Resolved } else { IssueStatus::Open },
        created: T0 + n as i64 * 1000,
        modified: T0 + (n * 7919 % 100_003) as i64 * 1000,
        objects: (0..3).map(|k| ObjectId::from_num(n * 3 + k + 5)).collect(),
        viewpoints: vec![Viewpoint {
            id: ViewpointId::from_num(n + 10),
            name: "Overview".into(),
            time: T0 + n as i64 * 1000,
            view: Some(ViewSpec::Plan {
                center: [1000.0, 2000.0],
                zoom: 0.5,
            }),
            selection: (0..3).map(|k| ObjectId::from_num(n * 3 + k + 5)).collect(),
            layers: (0..6)
                .map(|k| (LayerId::from_num(k + 1), k % 2 == 0))
                .collect(),
            ..Viewpoint::default()
        }],
        ..Issue::default()
    }
}

fn comment(n: u64, issues: u64) -> Comment {
    Comment {
        id: CommentId::from_num(n + 10),
        issue: IssueId::from_num(n % issues + 10),
        author: "bob@example.org".into(),
        origin: Origin::User,
        time: T0 + n as i64 * 10,
        text: "Replaced it on Monday. The new tree is a field maple, 14/16 cm girth, with a stake."
            .into(),
        viewpoint: None,
        edited: None,
    }
}

fn document(issues: u64, comments: u64) -> Document {
    let mut doc = Document::with_sequential_ids();
    doc.issues = IssueData::from_vecs(
        (0..issues).map(issue).collect(),
        (0..comments).map(|n| comment(n, issues)).collect(),
    )
    .into();
    doc
}

fn scales() -> Vec<(u64, u64)> {
    let text = std::env::var("KADAKAS_BENCH_SCALES").unwrap_or_else(|_| "1000:10000".into());
    text.split(',')
        .map(|p| {
            let (a, b) = p.split_once(':').expect("issues:comments");
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect()
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// The average time of three calls.
fn call_ms(s: &mut Session, method: &str, p: Value) -> f64 {
    let t = Instant::now();
    for _ in 0..3 {
        s.handle(method, p.clone())
            .unwrap_or_else(|e| panic!("{method}: {}", e.message));
    }
    ms(t) / 3.0
}

#[test]
#[ignore = "a measurement, run on request"]
fn measure_issue_calls_and_collaboration() {
    let collab = std::env::var("KADAKAS_BENCH_COLLAB").map_or(true, |v| v != "0");
    println!(
        "issues,comments,list_ms,list_filtered_ms,list_by_modified_ms,list_page_ms,get_ms,replica_ms,replica_things,connect_second_user_ms,second_user_result"
    );
    for (ni, nc) in scales() {
        let doc = document(ni, nc);
        let first = doc.issues.iter().nth(ni as usize / 2).unwrap().id;
        let mut s = Session::with_document(doc.clone());
        let list = call_ms(&mut s, "issue.list", json!({}));
        let filtered = call_ms(
            &mut s,
            "issue.list",
            json!({"status": ["open"], "text": "maple", "assignee": "mari@example.org"}),
        );
        let by_modified = call_ms(&mut s, "issue.list", json!({"sort": "modified"}));
        let page = call_ms(&mut s, "issue.list", json!({"offset": 5000, "limit": 200}));
        let get = call_ms(&mut s, "issue.get", json!({"id": first}));
        let (replica_ms, things, connect_ms, result) = if collab {
            let t = Instant::now();
            let replica = docsync::replica_of(&doc);
            let replica_ms = ms(t);
            let things = replica.things().count();
            let t = Instant::now();
            let built = kadakas_collab::materialize::materialize(&replica, &Document::default());
            println!(
                "  materialize of the replica: {:.0} ms ({} issues)",
                ms(t),
                built.doc.issues.len()
            );
            let hub = Hub::new(Sequencer::new(replica));
            let mut a = Session::with_document(doc.clone());
            a.set_collab_endpoint(Box::new(hub.endpoint()));
            let mut b = Session::default();
            b.set_collab_endpoint(Box::new(hub.endpoint()));
            let _ = a.handle("collab.connect", json!({"name": "Ann", "now_ms": 1_000}));
            let t = Instant::now();
            let r = b.handle("collab.connect", json!({"name": "Bo", "now_ms": 1_000}));
            let mut result = match &r {
                Ok(v) => v["state"].as_str().unwrap_or("?").to_string(),
                Err(e) => format!("error: {}", e.message),
            };
            for k in 0..4 {
                let _ = b.handle("collab.sync", json!({"now_ms": 2_000 + k}));
            }
            let connect_ms = ms(t);
            if b.editor.doc().issues.len() as u64 == ni {
                result.push_str(" (all issues arrived)");
            } else {
                result.push_str(&format!(
                    " ({} of {ni} issues arrived)",
                    b.editor.doc().issues.len()
                ));
            }
            (replica_ms, things, connect_ms, result)
        } else {
            (0.0, 0, 0.0, "skipped".into())
        };
        println!(
            "{ni},{nc},{list:.1},{filtered:.1},{by_modified:.1},{page:.1},{get:.1},{replica_ms:.0},{things},{connect_ms:.0},{result}"
        );
    }
}

/// The size of snapshot pictures: a render of a plan with some thousand objects, at 1920 x 1080
/// and at 3840 x 2160 (4K). The PNG is what `issue.create {capture: true}` stores.
#[test]
#[ignore = "a measurement, run on request"]
fn measure_snapshot_sizes() {
    let mut s = Session::default();
    // A site of 100 m x 60 m: paving, beds, trees and lines, on some layers.
    let mut commands = Vec::new();
    for i in 0..60i64 {
        for j in 0..40i64 {
            let (x, y) = (i * 1500, j * 1500);
            commands.push(json!({"op": "add_object", "shape":
                {"kind": "rect", "min": [x, y], "max": [x + 1200, y + 1200]}}));
            if (i + j) % 3 == 0 {
                commands.push(json!({"op": "add_object", "shape":
                    {"kind": "circle", "center": [x + 600, y + 600], "radius": 400}}));
            }
        }
    }
    let n = commands.len();
    s.handle(
        "execute",
        json!({"command": {"op": "batch", "commands": commands}}),
    )
    .expect("the objects");
    println!("objects,width,height,render_ms,png_bytes,pixels");
    for (w, h) in [(640u32, 480u32), (1920, 1080), (3840, 2160)] {
        let t = Instant::now();
        let r = s
            .handle("render.png", json!({"width": w, "height": h}))
            .expect("render.png");
        let ms = ms(t);
        let b64 = r["png_base64"].as_str().unwrap();
        let bytes = b64.len() / 4 * 3;
        println!(
            "{n},{w},{h},{ms:.0},{bytes},{}",
            u64::from(w) * u64::from(h)
        );
    }
    // The same through the issue (the picture is stored once in the document).
    for (w, h) in [(1920, 1080), (3840, 2160)] {
        let r = s.handle(
            "issue.create",
            json!({"title": format!("Snapshot {w}x{h}"), "capture": true,
                   "snapshot": {"width": w, "height": h}}),
        );
        match r {
            Ok(_) => println!("issue.create capture {w}x{h}: ok"),
            Err(e) => println!("issue.create capture {w}x{h}: {} ({})", e.message, e.kind),
        }
    }
}

/// For comparison: the same collaboration steps for a document of drawing objects only, with as
/// many things as the issues and comments of a scale have. `KADAKAS_BENCH_OBJECTS=109000`.
#[test]
#[ignore = "a measurement, run on request"]
fn measure_collab_with_objects_only() {
    let n: i64 = std::env::var("KADAKAS_BENCH_OBJECTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(11_000);
    let mut s = Session::default();
    let commands: Vec<Value> = (0..n)
        .map(|i| {
            json!({"op": "add_object", "shape":
                {"kind": "rect", "min": [i * 10, 0], "max": [i * 10 + 5, 5]}})
        })
        .collect();
    s.handle(
        "execute",
        json!({"command": {"op": "batch", "commands": commands}}),
    )
    .expect("objects");
    let doc = s.editor.doc().clone();
    let t = Instant::now();
    let replica = docsync::replica_of(&doc);
    let replica_ms = ms(t);
    let hub = Hub::new(Sequencer::new(replica));
    let mut a = Session::with_document(doc);
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    let mut b = Session::default();
    b.set_collab_endpoint(Box::new(hub.endpoint()));
    let _ = a.handle("collab.connect", json!({"name": "Ann", "now_ms": 1_000}));
    let t = Instant::now();
    let _ = b.handle("collab.connect", json!({"name": "Bo", "now_ms": 1_000}));
    for k in 0..4 {
        let _ = b.handle("collab.sync", json!({"now_ms": 2_000 + k}));
    }
    println!(
        "objects only: {n} objects, replica {replica_ms:.0} ms, second user {:.0} ms, {} objects arrived",
        ms(t),
        b.editor.doc().objects.len()
    );
}
