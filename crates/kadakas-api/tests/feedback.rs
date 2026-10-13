//! Feedback over the wire (API-47, API-48): error hints, the call flow from report to delivery,
//! and the rule that attachments need a human. The tests use a temporary folder and a fake
//! sender; nothing touches the network or a user folder.

use std::sync::Arc;

use kadakas_api::Session;
use kadakas_api::feedback_api::review::Review;
use kadakas_feedback::dispatch::Dispatcher;
use kadakas_feedback::store::DirStorage;
use kadakas_feedback::{
    AttachmentKind, FakeSender, NoSleep, Reply, RetryPolicy, SendError, Status,
};
use serde_json::{Value, json};

fn line(s: &mut Session, method: &str, params: Value) -> Value {
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    serde_json::from_str(&s.handle_line(&req.to_string()).unwrap()).unwrap()
}

#[test]
fn unknown_method_has_a_feedback_hint_with_a_draft() {
    let mut s = Session::with_sequential_ids();
    let r = line(&mut s, "plants.teleport", json!({}));
    assert_eq!(r["error"]["code"], -32601);
    assert_eq!(r["error"]["data"]["kind"], "method_not_found");
    let f = &r["error"]["data"]["feedback"];
    assert!(f["hint"].as_str().unwrap().contains("feedback"));
    assert!(f["hint"].as_str().unwrap().len() < 200);
    let d = &f["draft"];
    assert_eq!(d["kind"], "feature_request");
    assert_eq!(d["method"], "plants.teleport");
    assert!(d["error"].as_str().unwrap().contains("unknown method"));
    assert!(d["title"].as_str().unwrap().contains("plants.teleport"));
    // The draft goes to feedback.submit as it is.
    let r = line(&mut s, "feedback.submit", d.clone());
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(r["result"]["id"], "fb-1");
}

#[test]
fn an_unsupported_option_has_a_hint_and_other_errors_do_not() {
    let mut s = Session::with_sequential_ids();
    let r = line(
        &mut s,
        "massing.add",
        json!({"footprint": [[0,0],[1000,0],[1000,1000]], "bogus": 1}),
    );
    assert_eq!(r["error"]["code"], -32602, "{r}");
    assert_eq!(
        r["error"]["data"]["feedback"]["draft"]["kind"],
        "feature_request"
    );
    assert_eq!(
        r["error"]["data"]["feedback"]["draft"]["method"],
        "massing.add"
    );
    // A plain mistake carries no hint: the token budget stays small.
    let r = line(&mut s, "objects.get", json!({}));
    assert_eq!(r["error"]["code"], -32602);
    assert!(r["error"]["data"].get("feedback").is_none());
    let r = line(&mut s, "feedback.get", json!({"id": "nope"}));
    assert_eq!(r["error"]["code"], 2);
    assert!(r["error"]["data"].get("feedback").is_none());
}

#[test]
fn the_feedback_methods_are_listed_and_described() {
    let mut s = Session::with_sequential_ids();
    let d = line(&mut s, "describe", json!({}))["result"].clone();
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for m in [
        "feedback.submit",
        "feedback.list",
        "feedback.get",
        "feedback.withdraw",
    ] {
        assert!(names.contains(&m), "{m}");
    }
    assert!(d["feedback"]["examples"].as_array().unwrap().len() >= 4);
    let kinds: Vec<&str> = d["errors"]["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"internal"));
}

#[test]
fn from_report_to_delivery_needs_the_human_for_attachments() {
    let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-api").unwrap();
    let dir = tmp.path().join("outbox");
    let mut s = Session::with_sequential_ids();
    s.feedback
        .hub
        .set_storage(Box::new(DirStorage::new(dir.clone())));
    s.feedback
        .hub
        .set_url_override(Some("https://fb.test".into()));
    s.handle(
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [385000000, 667200000.0], "max": [385001000, 667201000.0]}}}),
    )
    .unwrap();

    // A report with attachments, and one with text only.
    let a = s
        .handle(
            "feedback.submit",
            json!({"kind": "bug", "title": "With pictures"}),
        )
        .unwrap();
    let b = s
        .handle(
            "feedback.submit",
            json!({"kind": "question", "title": "Only text"}),
        )
        .unwrap();
    assert_eq!(a["status"], "needs_review");
    assert_eq!(b["status"], "pending");
    // The files are in the folder, with owner-only names we made.
    assert!(dir.join("outbox.json").is_file());
    assert!(dir.join("fb-1.render.png").is_file());

    let fake = Arc::new(FakeSender::new(vec![]));
    let policy = RetryPolicy {
        attempts: 2,
        base_ms: 1,
        max_ms: 1,
    };
    let mut d = Dispatcher::new(fake.clone(), Arc::new(NoSleep::default()), policy);

    // Sending is off: nothing goes out, whatever the host does.
    d.pump(&mut s.feedback.hub, true);
    d.finish(&mut s.feedback.hub);
    assert!(fake.requests().is_empty());

    // The person agrees to send text reports: only the text report goes.
    s.feedback.hub.set_send(Some(true)).unwrap();
    d.pump(&mut s.feedback.hub, true);
    d.finish(&mut s.feedback.hub);
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].0, "https://fb.test/v1/feedback");
    assert!(reqs[0].1.contains("Only text") && !reqs[0].1.contains("With pictures"));
    assert_eq!(s.feedback.hub.get("fb-2").unwrap().status, Status::Sent);
    assert_eq!(
        s.feedback.hub.get("fb-1").unwrap().status,
        Status::NeedsReview
    );

    // The human reviews it: the excerpt has the place removed by default.
    let mut r = Review::open(&mut s.feedback.hub, "fb-1").unwrap();
    assert!(r.included(AttachmentKind::Excerpt));
    r.set_included(AttachmentKind::Calls, false);
    let payload = r.payload().unwrap();
    assert!(!payload.contains("385000000") && !payload.contains("667200000"));
    r.approve(&mut s.feedback.hub).unwrap();
    d.pump(&mut s.feedback.hub, true);
    d.finish(&mut s.feedback.hub);
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[1].1, payload);
    let p: Value = serde_json::from_str(&reqs[1].1).unwrap();
    assert!(p["attachments"].get("calls").is_none());
    assert_eq!(p["attachments"]["render"]["mime"], "image/png");
    let sent = s.feedback.hub.get("fb-1").unwrap();
    assert_eq!(sent.status, Status::Sent);
    assert_eq!(sent.server_id.as_deref(), Some("srv-2"));
    // The attachments are gone from the folder after delivery.
    assert!(!dir.join("fb-1.render.png").exists());

    // A new session on the same folder sees both reports (the outbox is shared).
    let mut s2 = Session::with_sequential_ids();
    s2.feedback.hub.set_storage(Box::new(DirStorage::new(dir)));
    let l = s2.handle("feedback.list", json!({})).unwrap();
    assert_eq!(l["reports"].as_array().unwrap().len(), 2);
    assert_eq!(l["sending"]["enabled"], true);
}

#[test]
fn a_failing_server_keeps_the_report_and_counts_the_tries() {
    let mut s = Session::with_sequential_ids();
    s.feedback.hub.set_send_override(Some(true));
    s.handle(
        "feedback.submit",
        json!({"kind": "question", "title": "Retry me"}),
    )
    .unwrap();
    let fake = Arc::new(FakeSender::new(vec![
        Err(SendError::Transient(
            "down".into()
        ));
        2
    ]));
    let policy = RetryPolicy {
        attempts: 2,
        base_ms: 1,
        max_ms: 1,
    };
    let sleeper = Arc::new(NoSleep::default());
    let mut d = Dispatcher::new(fake.clone(), sleeper.clone(), policy);
    d.pump(&mut s.feedback.hub, true);
    d.finish(&mut s.feedback.hub);
    let r = s.feedback.hub.get("fb-1").unwrap();
    assert_eq!((r.status, r.attempts), (Status::Pending, 2));
    assert_eq!(r.last_error.as_deref(), Some("down"));
    assert_eq!(*sleeper.0.lock().unwrap(), vec![1]);
    // The next pump succeeds.
    d.pump(&mut s.feedback.hub, true);
    d.finish(&mut s.feedback.hub);
    let r = s.feedback.hub.get("fb-1").unwrap();
    assert_eq!(r.status, Status::Sent);
    let _ = Reply {
        id: String::new(),
        status: String::new(),
    };
}
