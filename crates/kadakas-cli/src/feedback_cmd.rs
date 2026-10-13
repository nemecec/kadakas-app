//! `kadakas feedback review` and `kadakas feedback list` (API-48).
//!
//! The review is the terminal side of the approval path. A person sees every part of a report
//! that waits as `needs_review`: the text, each picture (its size), the objects of the model
//! excerpt with a small plan, and the raw JSON. They untick what they do not want to share, set
//! the privacy options, and send. Nothing here runs without a terminal: the command refuses
//! when stdin or stdout is a pipe, so a script or an agent cannot approve its own report.
//!
//! **What this protects, and what it does not.** The guarantee holds for an agent that is limited
//! to the MCP tools and to JSON-RPC: nothing in them approves or sends an attachment, and a
//! restricted session cannot write into the folder of the outbox. An agent that has a shell, or
//! that is allowed to run scripts (`--allow-agent-scripts`) or converters, runs as the same user
//! as the person. It can read and write the same files, and it can type into a terminal that it
//! started itself. A terminal check cannot stop such an agent, and this program does not claim to.
//! Do not give a shell to an agent that you do not trust with your drawings.
//!
//! The pictures of a report are written to private temporary files (owner only) before the
//! question about them, and the path is printed, so the person can open them in a viewer. The
//! default answer for a picture is no.

use std::io::{BufRead, Write};
use std::path::Path;

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

/// Reviews one report. Returns true if it was sent.
/// Seconds since 1970.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn review_one(
    hub: &mut Hub,
    id: &str,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    sender: &dyn Sender,
    sleeper: &dyn Sleeper,
    images: &Path,
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
        writeln!(
            out,
            "These options also hide coordinates and CRS values in the recent calls and log lines."
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
                let png = r.image(kind).unwrap_or_default();
                let file = images.join(format!("{id}-{}.png", kind.id()));
                kadakas_sys::fsio::write_private(&file, png).map_err(|e| e.to_string())?;
                writeln!(
                    out,
                    "\nPicture ({}): {}. Open it to look at it: {}",
                    kind.id(),
                    kb(png.len()),
                    file.display()
                )
                .map_err(io)?;
            }
            AttachmentKind::Calls => {
                writeln!(
                    out,
                    "\nRecent calls (names and scrubbed parameters):\n{}",
                    r.calls_text().unwrap_or_default()
                )
                .map_err(io)?;
            }
            AttachmentKind::Log => {
                writeln!(
                    out,
                    "\nRecent log lines:\n{}",
                    r.log_text().unwrap_or_default()
                )
                .map_err(io)?;
            }
        }
        // A picture can show more than the person expects: the default answer is no.
        let default_yes = !matches!(kind, AttachmentKind::Screenshot | AttachmentKind::Render);
        let on = ask(input, out, &format!("Send the {}?", kind.id()), default_yes).map_err(io)?;
        r.set_included(kind, on);
    }
    loop {
        if ask(input, out, "\nShow the raw JSON that will be sent?", false).map_err(io)? {
            let p = r.payload().map_err(|e| e.to_string())?;
            writeln!(
                out,
                "{}",
                kadakas_api::feedback_api::review::display_json(&p)
            )
            .map_err(io)?;
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
                if let Err(e) = r.approve(hub) {
                    // For example: the person turned sending off. Nothing changed.
                    writeln!(out, "Not approved: {e}.").map_err(io)?;
                    return Ok(false);
                }
                let Some(item) = hub.claim(id, now()) else {
                    writeln!(
                        out,
                        "The report is approved. Another program is sending it, or sending is off."
                    )
                    .map_err(io)?;
                    return Ok(false);
                };
                let url = hub.url();
                writeln!(out, "Sending to {url} ...").map_err(io)?;
                let d = deliver(sender, sleeper, &url, &item.payload, RetryPolicy::default());
                hub.record(id, &d, now()).map_err(|e| e.to_string())?;
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

/// Reviews one report, or all that wait. Returns how many were sent. The pictures go to a private
/// temporary folder that is removed at the end.
pub fn review(
    hub: &mut Hub,
    only: Option<&str>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    sender: &dyn Sender,
    sleeper: &dyn Sleeper,
) -> Result<usize, String> {
    let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-feedback-review")
        .map_err(|e| format!("cannot make a private temporary folder: {e}"))?;
    review_in(hub, only, input, out, sender, sleeper, dir.path())
}

/// Like [`review`], with the folder for the pictures given.
pub fn review_in(
    hub: &mut Hub,
    only: Option<&str>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    sender: &dyn Sender,
    sleeper: &dyn Sleeper,
    images: &Path,
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
        if review_one(hub, &id, input, out, sender, sleeper, images)? {
            sent += 1;
        }
    }
    Ok(sent)
}

/// `kadakas feedback retry [id]`: gives a report (or every unsent one) a fresh start after
/// failures: no tries counted, no waiting.
pub fn retry(hub: &mut Hub, id: Option<&str>, out: &mut dyn Write) -> Result<(), String> {
    hub.reset_tries(id).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "{} will be tried again at the next start of sending.",
        id.unwrap_or("Every unsent report")
    )
    .map_err(|e| e.to_string())
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
        if let Some(e) = &r.last_error {
            writeln!(
                out,
                "\tlast error: {e} (tries {}, next try at {})",
                r.attempts, r.next_try
            )?;
        }
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
        // Privacy: keep the defaults (Enter x3); excerpt Enter, render y (the default is no),
        // calls n; raw JSON y; send.
        let (r, text) = run(&mut s, None, "\n\n\n\ny\nn\ny\ns\n", &fake);
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
    fn a_picture_is_written_to_a_private_file_before_the_question_and_the_default_is_no() {
        let mut s = session();
        let fake = FakeSender::new(vec![]);
        let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-images").unwrap();
        let mut out = Vec::new();
        // Privacy Enter x3, excerpt Enter, picture Enter (= no), calls Enter, raw JSON Enter, send.
        let r = review_in(
            &mut s.feedback.hub,
            None,
            &mut Cursor::new("\n\n\n\n\n\n\ns\n".to_string()),
            &mut out,
            &fake,
            &NoSleep::default(),
            dir.path(),
        );
        let text = String::from_utf8(out).unwrap();
        assert_eq!(r, Ok(1), "{text}");
        let file = dir.path().join("fb-1-render.png");
        assert!(text.contains(&file.display().to_string()), "{text}");
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // The path comes before the question about the picture.
        let at_path = text.find(&file.display().to_string()).unwrap();
        let at_question = text.find("Send the render?").unwrap();
        assert!(at_path < at_question);
        // The answer was no: the message has no picture.
        assert!(!fake.requests()[0].1.contains("\"render\""));
    }

    #[test]
    fn a_refused_approval_changes_nothing() {
        let mut s = session();
        s.feedback.hub.set_send(Some(false)).unwrap();
        let fake = FakeSender::new(vec![]);
        let (r, text) = run(&mut s, None, "\n\n\n\n\n\n\ns\n", &fake);
        assert_eq!(r, Ok(0), "{text}");
        assert!(text.contains("Not approved: sending is off"), "{text}");
        assert!(fake.requests().is_empty());
        assert_eq!(
            s.feedback.hub.get("fb-1").unwrap().status,
            Status::NeedsReview
        );
    }

    #[test]
    fn retry_gives_a_fresh_start() {
        let mut s = session();
        s.feedback.hub.set_send(Some(true)).unwrap();
        s.feedback
            .hub
            .submit(
                kadakas_feedback::Draft::new(
                    kadakas_feedback::Kind::Question,
                    "Q",
                    "",
                    kadakas_feedback::Origin::User,
                ),
                vec![],
                0,
            )
            .unwrap();
        let failed = Delivery::Failed {
            message: "down".into(),
            permanent: false,
            unreachable: false,
            retry_after: None,
            attempts: 1,
        };
        s.feedback.hub.record("fb-2", &failed, 1_000).unwrap();
        let mut out = Vec::new();
        list(&mut s.feedback.hub, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("last error: down"));
        let mut out = Vec::new();
        retry(&mut s.feedback.hub, Some("fb-2"), &mut out).unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("fb-2 will be tried again")
        );
        let r = s.feedback.hub.get("fb-2").unwrap();
        assert_eq!((r.attempts, r.next_try, r.last_error), (0, 0, None));
        let mut out = Vec::new();
        retry(&mut s.feedback.hub, None, &mut out).unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("Every unsent report")
        );
        assert!(retry(&mut s.feedback.hub, Some("fb-77"), &mut Vec::new()).is_err());
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
                1_000,
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
