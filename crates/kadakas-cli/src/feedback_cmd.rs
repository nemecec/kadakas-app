//! `kadakas feedback review` and `kadakas feedback list` (API-48).
//!
//! The review is the terminal side of the approval path. A person sees every part of a report
//! that waits as `needs_review`: the text, each picture (its size), the objects of the model
//! excerpt with a small plan, and the raw JSON. They untick what they do not want to share, set
//! the privacy options, and send. Nothing here runs without a terminal: the command refuses
//! when stdin or stdout is a pipe, so a script or an agent cannot approve its own report.

use std::io::{BufRead, Write};

use kadakas_api::feedback_api::excerpt::ObjectBrief;
use kadakas_api::feedback_api::review::Review;
use kadakas_feedback::{
    AttachmentKind, Delivery, Hub, RetryPolicy, Sender, Sleeper, Status, deliver,
};

/// Width and height of the plan in characters.
const PLAN_W: usize = 48;
const PLAN_H: usize = 12;

/// Fails unless stdin and stdout are a terminal. `tty` is the answer of `IsTerminal`.
pub fn require_terminal(stdin_tty: bool, stdout_tty: bool) -> Result<(), String> {
    if stdin_tty && stdout_tty {
        Ok(())
    } else {
        Err("kadakas feedback review needs a terminal: a person must read the report and answer. It cannot run from a script or a pipe.".into())
    }
}

/// A small text plan of the bounding boxes of the objects.
pub fn plan(objects: &[ObjectBrief]) -> Vec<String> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for o in objects {
        for i in 0..2 {
            lo[i] = lo[i].min(o.min[i]);
            hi[i] = hi[i].max(o.max[i]);
        }
    }
    if objects.is_empty() || !(lo[0].is_finite() && hi[0].is_finite()) {
        return Vec::new();
    }
    let span = |i: usize| (hi[i] - lo[i]).max(1.0);
    let mut grid = vec![vec![' '; PLAN_W]; PLAN_H];
    let cell =
        |v: f64, i: usize, n: usize| (((v - lo[i]) / span(i)) * (n - 1) as f64).round() as usize;
    for o in objects {
        let (x0, x1) = (cell(o.min[0], 0, PLAN_W), cell(o.max[0], 0, PLAN_W));
        // The y axis points up.
        let (y0, y1) = (
            PLAN_H - 1 - cell(o.max[1], 1, PLAN_H),
            PLAN_H - 1 - cell(o.min[1], 1, PLAN_H),
        );
        for row in grid.iter_mut().take(y1 + 1).skip(y0) {
            for c in row.iter_mut().take(x1 + 1).skip(x0) {
                *c = '#';
            }
        }
    }
    let edge = format!("+{}+", "-".repeat(PLAN_W));
    let mut out = vec![edge.clone()];
    out.extend(
        grid.into_iter()
            .map(|r| format!("|{}|", r.into_iter().collect::<String>())),
    );
    out.push(edge);
    out
}

fn ask(
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    question: &str,
    default: bool,
) -> std::io::Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };
    write!(out, "{question} [{hint}] ")?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        // The input ended: the safe answer.
        return Ok(false);
    }
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    })
}

fn kb(n: usize) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

/// Shows the raw JSON. Long base64 texts are shortened on screen only; the payload is not.
fn show_json(payload: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(mut v) => {
            for k in ["screenshot", "render"] {
                if let Some(b) = v["attachments"][k]["base64"].as_str() {
                    let n = b.len();
                    v["attachments"][k]["base64"] = serde_json::json!(format!(
                        "<{n} characters of base64, shown in full in the sent message>"
                    ));
                }
            }
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        Err(_) => payload.to_string(),
    }
}

/// Reviews one report. Returns true if it was sent.
fn review_one(
    hub: &mut Hub,
    id: &str,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    sender: &dyn Sender,
    sleeper: &dyn Sleeper,
) -> Result<bool, String> {
    let mut r = Review::open(hub, id).map_err(|e| e.to_string())?;
    let io = |e: std::io::Error| e.to_string();
    let rep = r.report.clone();
    writeln!(
        out,
        "\n=== {} ({}, {:?}) ===",
        rep.id,
        rep.kind.id(),
        rep.origin
    )
    .map_err(io)?;
    writeln!(out, "Title: {}", rep.title).map_err(io)?;
    for (label, text) in [
        ("Text", &rep.body),
        ("Wanted", &rep.wanted),
        ("Tried", &rep.tried),
        ("Method", &rep.method),
        ("Error", &rep.error),
    ] {
        if !text.is_empty() {
            writeln!(out, "{label}: {text}").map_err(io)?;
        }
    }
    writeln!(
        out,
        "Kadakas {} on {}; client {} {}",
        rep.context.version, rep.context.os, rep.context.client_name, rep.context.client_version
    )
    .map_err(io)?;

    // Privacy options first: they change what the excerpt shows.
    if r.attachments()
        .iter()
        .any(|a| a.0 == AttachmentKind::Excerpt)
    {
        r.privacy.move_to_origin = ask(
            input,
            out,
            "Move the model to the origin (hides where it is)?",
            r.privacy.move_to_origin,
        )
        .map_err(io)?;
        r.privacy.remove_georef = ask(
            input,
            out,
            "Remove the georeference?",
            r.privacy.remove_georef,
        )
        .map_err(io)?;
        r.privacy.remove_names = ask(
            input,
            out,
            "Remove names, notes and record data?",
            r.privacy.remove_names,
        )
        .map_err(io)?;
    }
    for kind in AttachmentKind::ALL {
        if !r.attachments().iter().any(|a| a.0 == kind) {
            continue;
        }
        match kind {
            AttachmentKind::Excerpt => match r.excerpt_text() {
                Some(Ok(t)) => {
                    let objs = r.excerpt_objects();
                    writeln!(
                        out,
                        "\nModel excerpt: {} objects, {}",
                        objs.len(),
                        kb(t.len())
                    )
                    .map_err(io)?;
                    for o in objs.iter().take(30) {
                        writeln!(
                            out,
                            "  {} {} on {} ({:.0}, {:.0}) to ({:.0}, {:.0})",
                            o.kind, o.id, o.layer, o.min[0], o.min[1], o.max[0], o.max[1]
                        )
                        .map_err(io)?;
                    }
                    if objs.len() > 30 {
                        writeln!(out, "  ... and {} more", objs.len() - 30).map_err(io)?;
                    }
                    for line in plan(&objs) {
                        writeln!(out, "  {line}").map_err(io)?;
                    }
                }
                Some(Err(e)) => writeln!(
                    out,
                    "\nModel excerpt: cannot be shown with these options ({e})"
                )
                .map_err(io)?,
                None => {}
            },
            AttachmentKind::Screenshot | AttachmentKind::Render => {
                let n = r.image(kind).map_or(0, <[u8]>::len);
                writeln!(out, "\nPicture ({}): {}", kind.id(), kb(n)).map_err(io)?;
            }
            AttachmentKind::Calls => {
                writeln!(
                    out,
                    "\nRecent calls (names and scrubbed parameters):\n{}",
                    r.calls_text().unwrap_or("")
                )
                .map_err(io)?;
            }
            AttachmentKind::Log => {
                writeln!(out, "\nRecent log lines:\n{}", r.log_text().unwrap_or("")).map_err(io)?;
            }
        }
        let on = ask(input, out, &format!("Send the {}?", kind.id()), true).map_err(io)?;
        r.set_included(kind, on);
    }
    loop {
        if ask(input, out, "\nShow the raw JSON that will be sent?", false).map_err(io)? {
            let p = r.payload().map_err(|e| e.to_string())?;
            writeln!(out, "{}", show_json(&p)).map_err(io)?;
        }
        write!(
            out,
            "Send it (s), keep it for later (k) or withdraw it (w)? [k] "
        )
        .map_err(io)?;
        out.flush().map_err(io)?;
        let mut line = String::new();
        if input.read_line(&mut line).map_err(io)? == 0 {
            return Ok(false);
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "s" | "send" => {
                r.approve(hub).map_err(|e| e.to_string())?;
                let out_item = hub.outgoing(false, 20).into_iter().find(|o| o.id == id);
                let Some(item) = out_item else {
                    return Err("the approved report is not ready to send".into());
                };
                let url = hub.url();
                writeln!(out, "Sending to {url} ...").map_err(io)?;
                let d = deliver(sender, sleeper, &url, &item.payload, RetryPolicy::default());
                hub.record(id, &d).map_err(|e| e.to_string())?;
                match d {
                    Delivery::Sent(reply) => {
                        writeln!(out, "Sent. Server id {} ({}).", reply.id, reply.status)
                            .map_err(io)?;
                        return Ok(true);
                    }
                    Delivery::Failed { message, .. } => {
                        writeln!(out, "Not sent yet: {message}. The report is approved and stays in the outbox.").map_err(io)?;
                        return Ok(false);
                    }
                }
            }
            "w" | "withdraw" => {
                hub.withdraw(id).map_err(|e| e.to_string())?;
                writeln!(out, "Withdrawn.").map_err(io)?;
                return Ok(false);
            }
            "" | "k" | "keep" => {
                writeln!(out, "Kept. It stays in the outbox.").map_err(io)?;
                return Ok(false);
            }
            _ => writeln!(out, "Please answer s, k or w.").map_err(io)?,
        }
    }
}

/// Reviews one report, or all that wait. Returns how many were sent.
pub fn review(
    hub: &mut Hub,
    only: Option<&str>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    sender: &dyn Sender,
    sleeper: &dyn Sleeper,
) -> Result<usize, String> {
    let ids: Vec<String> = match only {
        Some(id) => vec![id.to_string()],
        None => hub
            .list()
            .into_iter()
            .filter(|r| r.status == Status::NeedsReview)
            .map(|r| r.id)
            .collect(),
    };
    if ids.is_empty() {
        writeln!(out, "No report waits for review.").map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let mut sent = 0;
    for id in ids {
        if review_one(hub, &id, input, out, sender, sleeper)? {
            sent += 1;
        }
    }
    Ok(sent)
}

/// `kadakas feedback list`: one line per report.
pub fn list(hub: &mut Hub, out: &mut dyn Write) -> std::io::Result<()> {
    let reports = hub.list();
    if reports.is_empty() {
        return writeln!(out, "The outbox is empty.");
    }
    for r in reports {
        let status = serde_json::to_value(r.status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        writeln!(
            out,
            "{}\t{}\t{}\tx{}\t{}",
            r.id,
            status,
            r.kind.id(),
            r.count,
            r.title
        )?;
    }
    writeln!(
        out,
        "Sending of text reports is {}.",
        if hub.send_enabled() { "on" } else { "off" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_api::Session;
    use kadakas_feedback::{FakeSender, NoSleep, Reply, SendError};
    use serde_json::json;
    use std::io::Cursor;

    fn session() -> Session {
        let mut s = Session::with_sequential_ids();
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [385000000, 667200000], "max": [385002000, 667203000]}}}),
        )
        .unwrap();
        s.handle(
            "feedback.submit",
            json!({"kind": "bug", "title": "Needs a look"}),
        )
        .unwrap();
        s
    }

    fn run(
        s: &mut Session,
        only: Option<&str>,
        answers: &str,
        sender: &FakeSender,
    ) -> (Result<usize, String>, String) {
        let mut out = Vec::new();
        let r = review(
            &mut s.feedback.hub,
            only,
            &mut Cursor::new(answers.to_string()),
            &mut out,
            sender,
            &NoSleep::default(),
        );
        (r, String::from_utf8(out).unwrap())
    }

    #[test]
    fn the_terminal_is_required() {
        assert!(require_terminal(true, true).is_ok());
        for (a, b) in [(false, true), (true, false), (false, false)] {
            assert!(require_terminal(a, b).unwrap_err().contains("terminal"));
        }
    }

    #[test]
    fn a_person_reviews_and_sends() {
        let mut s = session();
        let fake = FakeSender::new(vec![]);
        // Privacy: keep the defaults (Enter x3); excerpt Y, render Y, calls n; raw JSON y; send.
        let (r, text) = run(&mut s, None, "\n\n\n\n\nn\ny\ns\n", &fake);
        assert_eq!(r, Ok(1), "{text}");
        assert!(text.contains("Model excerpt: 1 objects"), "{text}");
        assert!(text.contains("+----") && text.contains('#'));
        assert!(text.contains("characters of base64"));
        assert!(text.contains("Sent. Server id srv-1"));
        let reqs = fake.requests();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].1.contains("\"render\"") && !reqs[0].1.contains("\"calls\""));
        assert!(
            !reqs[0].1.contains("385000000"),
            "the place is removed by default"
        );
        assert_eq!(s.feedback.hub.get("fb-1").unwrap().status, Status::Sent);
    }

    #[test]
    fn keep_withdraw_and_end_of_input_send_nothing() {
        for (answers, status) in [
            ("\n\n\n\n\n\n\nk\n", Status::NeedsReview),
            ("\n\n\n\n\n\n\nw\n", Status::Withdrawn),
            ("\n\n\n\n\n\n\nx\n\n", Status::NeedsReview),
            ("", Status::NeedsReview),
        ] {
            let mut s = session();
            let fake = FakeSender::new(vec![]);
            let (r, text) = run(&mut s, Some("fb-1"), answers, &fake);
            assert_eq!(r, Ok(0), "{text}");
            assert!(fake.requests().is_empty());
            assert_eq!(
                s.feedback.hub.get("fb-1").unwrap().status,
                status,
                "{answers:?}"
            );
        }
    }

    #[test]
    fn a_failed_send_keeps_the_approved_report() {
        let mut s = session();
        let fake = FakeSender::new(vec![Err(SendError::Transient("down".into())); 4]);
        let (r, text) = run(&mut s, None, "\n\n\n\n\n\n\ns\n", &fake);
        assert_eq!(r, Ok(0));
        assert!(text.contains("Not sent yet: down"));
        assert_eq!(s.feedback.hub.get("fb-1").unwrap().status, Status::Approved);
    }

    #[test]
    fn nothing_waits_and_unknown_ids() {
        let mut s = Session::with_sequential_ids();
        let fake = FakeSender::new(vec![]);
        let (r, text) = run(&mut s, None, "", &fake);
        assert_eq!(r, Ok(0));
        assert!(text.contains("No report waits"));
        let (r, _) = run(&mut s, Some("fb-9"), "", &fake);
        assert!(r.is_err());
    }

    #[test]
    fn the_plan_draws_boxes() {
        assert!(plan(&[]).is_empty());
        let o = |a: [f64; 2], b: [f64; 2]| ObjectBrief {
            id: kadakas_core::model::ObjectId::from_num(1),
            kind: "rect".into(),
            layer: "L".into(),
            min: a,
            max: b,
        };
        let p = plan(&[o([0.0, 0.0], [10.0, 10.0]), o([90.0, 90.0], [100.0, 100.0])]);
        assert_eq!(p.len(), PLAN_H + 2);
        assert!(p[1].contains('#') && p[PLAN_H].contains('#'));
        // A single point still draws.
        assert!(
            plan(&[o([5.0, 5.0], [5.0, 5.0])])
                .iter()
                .any(|l| l.contains('#'))
        );
    }

    #[test]
    fn list_shows_the_outbox() {
        let mut s = Session::with_sequential_ids();
        let mut out = Vec::new();
        list(&mut s.feedback.hub, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("empty"));
        s.handle("feedback.submit", json!({"kind": "question", "title": "Q"}))
            .unwrap();
        s.feedback
            .hub
            .record(
                "fb-1",
                &Delivery::Sent(Reply {
                    id: "s".into(),
                    status: "received".into(),
                }),
            )
            .unwrap();
        let mut out = Vec::new();
        list(&mut s.feedback.hub, &mut out).unwrap();
        let t = String::from_utf8(out).unwrap();
        assert!(
            t.contains("fb-1\tsent\tquestion\tx1\tQ") && t.contains("off"),
            "{t}"
        );
    }
}
