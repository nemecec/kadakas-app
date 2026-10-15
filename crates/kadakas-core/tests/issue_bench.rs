//! Measurements for the bounds of issues (REV-04). Not a test of behavior: it measures memory,
//! the time to build, save and load a document, and the bytes that one undo step keeps, for
//! different numbers of issues and comments. It is ignored by default and prints its numbers:
//!
//! ```text
//! KADAKAS_BENCH_SCALES=1000:10000,10000:100000 \
//!   cargo test -p kadakas-core --release --test issue_bench -- --ignored --nocapture
//! ```
//!
//! The numbers are for the review of the limits (`MAX_ISSUES`, `MAX_COMMENTS_TOTAL`); the limits
//! themselves are chosen with the person, not by this file.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use kadakas_core::change::Origin;
use kadakas_core::command::{Command, Editor};
use kadakas_core::issue::*;
use kadakas_core::model::{Document, ObjectId};

/// Counts the bytes that are alive.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        if new >= l.size() {
            LIVE.fetch_add(new - l.size(), Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(l.size() - new, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

const T0: i64 = 1_700_000_000_000;

fn issue(n: u64) -> Issue {
    Issue {
        id: IssueId::from_num(n + 10),
        title: format!("Issue {n}: a dead tree near the gate"),
        description: "The tree was planted last spring and is dead now. Please replace it with a field maple and check the soil around the root ball.".into(),
        assignee: Some("mari@example.org".into()),
        labels: vec!["planting".into(), "tree".into()],
        author: "ann@example.org".into(),
        created: T0 + n as i64 * 1000,
        modified: T0 + n as i64 * 1000,
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
                .map(|k| (kadakas_core::model::LayerId::from_num(k + 1), k % 2 == 0))
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

fn data(issues: u64, comments: u64) -> IssueData {
    IssueData::from_vecs(
        (0..issues).map(issue).collect(),
        (0..comments).map(|n| comment(n, issues)).collect(),
    )
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

fn mib(b: usize) -> f64 {
    b as f64 / 1_048_576.0
}

#[test]
#[ignore = "a measurement, run on request"]
fn measure_issue_data() {
    println!(
        "issues,comments,build_ms,memory_mib,save_ms,json_mib,load_ms,comment_cmd_ms,undo_bytes_per_comment_step,modify_cmd_ms,undo_bytes_per_modify_step"
    );
    for (ni, nc) in scales() {
        let before = live();
        let t = Instant::now();
        let mut doc = Document::with_sequential_ids();
        doc.issues = Issues::from(data(ni, nc));
        let build = ms(t);
        let memory = live() - before;

        let t = Instant::now();
        let mut json = Vec::new();
        doc.write_pretty(&mut json).unwrap();
        let save = ms(t);
        let json_size = json.len();
        let t = Instant::now();
        let loaded = Document::from_json(std::str::from_utf8(&json).unwrap()).unwrap();
        let load = ms(t);
        assert_eq!(loaded.issues.len(), ni as usize);
        drop(loaded);
        drop(json);

        let mut ed = Editor::new(doc);
        ed.set_origin(Origin::User);
        let steps = 20u64;
        // One comment per step on an issue.
        let before = live();
        let t = Instant::now();
        // Over the bound of comments a command refuses a new one: an edit of a comment is the
        // step then (it has the same costs for the undo step).
        let adds = (nc as usize) < MAX_COMMENTS_TOTAL;
        for k in 0..steps {
            let command = if adds {
                Command::AddIssueComment {
                    comment: Box::new(Comment {
                        id: CommentId::default(),
                        issue: IssueId::from_num(k % ni + 10),
                        time: T0 + 10_000_000_000 + k as i64,
                        ..comment(0, ni)
                    }),
                }
            } else {
                Command::EditIssueComment {
                    id: CommentId::from_num(k * 7 + 10),
                    text: format!("Edited {k}"),
                    now: T0 + 10_000_000_000 + k as i64,
                }
            };
            ed.execute(&command).unwrap_or_else(|e| panic!("{e}"));
        }
        let comment_ms = ms(t) / steps as f64;
        let per_comment_step = (live() - before) / steps as usize;
        // One edit of an issue per step.
        let before = live();
        let t = Instant::now();
        for k in 0..steps {
            ed.execute(&Command::ModifyIssue {
                id: IssueId::from_num(k % ni + 10),
                patch: Box::new(IssuePatch {
                    status: Some(IssueStatus::Resolved),
                    ..IssuePatch::default()
                }),
                now: T0 + 20_000_000_000 + k as i64,
            })
            .unwrap_or_else(|e| panic!("{e}"));
        }
        let modify_ms = ms(t) / steps as f64;
        let per_modify_step = (live() - before) / steps as usize;
        println!(
            "{ni},{nc},{build:.0},{:.1},{save:.0},{:.1},{load:.0},{comment_ms:.2},{per_comment_step},{modify_ms:.2},{per_modify_step}",
            mib(memory),
            mib(json_size)
        );
    }
}
