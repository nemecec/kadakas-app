//! The question before a big file opens (DOC-19): "This file is N MB and needs about M GB of memory
//! to open. Open it?", with Open and Cancel, and the refusal of a file that the computer cannot hold.
//!
//! The decision is `kadakas_ui_core::open_limit::decide`. This module holds the state of the
//! question and the window. One gate serves one way into a file. The way passes what it wants to open
//! (`T`: a path, an index, a request) to [`OpenGate::check`], and opens it when `check` gives it
//! back at once or when the person answers Open in [`OpenGate::show`]. A second question that comes
//! while one is on the screen waits in a queue; it is not lost.

use std::collections::VecDeque;

use kadakas_l10n::tr;
use kadakas_ui_core::open_limit::{Estimate, OpenDecision};

/// What waits for the answer.
struct Question<T> {
    what: T,
    estimate: Estimate,
    /// The file is too big for this computer: there is nothing to confirm.
    refused: bool,
}

pub struct OpenGate<T> {
    /// The first question is the one on the screen.
    questions: VecDeque<Question<T>>,
}

impl<T> Default for OpenGate<T> {
    fn default() -> Self {
        OpenGate {
            questions: VecDeque::new(),
        }
    }
}

impl<T> OpenGate<T> {
    /// Applies the decision for `what`. `Open` gives `what` back: open it now. A question or a refusal
    /// is kept for [`show`](Self::show), and the result is None.
    pub fn check(&mut self, what: T, decision: OpenDecision) -> Option<T> {
        let (estimate, refused) = match decision {
            OpenDecision::Open => return Some(what),
            OpenDecision::Ask(e) => (e, false),
            OpenDecision::Refuse(e) => (e, true),
        };
        self.questions.push_back(Question {
            what,
            estimate,
            refused,
        });
        None
    }

    /// True while a question or a refusal is on the screen or waits.
    pub fn is_pending(&self) -> bool {
        !self.questions.is_empty()
    }

    /// How many questions wait, the one on the screen included.
    #[cfg(test)]
    pub fn queued(&self) -> usize {
        self.questions.len()
    }

    /// The text of the question or the refusal that is on the screen.
    pub fn text(&self) -> Option<String> {
        self.questions.front().map(|q| {
            if q.refused {
                q.estimate.refuse_text()
            } else {
                q.estimate.ask_text()
            }
        })
    }

    /// The answer of the person to the question on the screen. `open` true gives back what was asked
    /// about and the size of the file that the person agreed to (the caller checks that the file did
    /// not grow), but never for a refusal. Either way the question is gone, and the next one shows.
    pub fn answer(&mut self, open: bool) -> Option<(T, u64)> {
        let q = self.questions.pop_front()?;
        (open && !q.refused).then_some((q.what, q.estimate.file_bytes))
    }

    /// Shows the window. Returns what the person agreed to open, with the size they agreed to.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<(T, u64)> {
        let q = self.questions.front()?;
        let refused = q.refused;
        let title = if refused {
            tr!("open-gate-refused-title")
        } else {
            tr!("open-gate-ask-title")
        };
        let text = self.text().unwrap_or_default();
        let waiting = self.questions.len() - 1;
        let mut answer = None;
        // A modal window: the rest of the app waits for the answer.
        egui::Modal::new(egui::Id::new("win-open-big-file")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(title);
            ui.label(text);
            if waiting > 0 {
                ui.small(tr!("open-gate-more-questions", count = waiting));
            }
            ui.horizontal(|ui| {
                if refused {
                    if ui.button(tr!("common-ok")).clicked() {
                        answer = Some(false);
                    }
                } else {
                    if ui.button(tr!("open-gate-open")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(tr!("common-cancel")).clicked() {
                        answer = Some(false);
                    }
                }
            });
        });
        self.answer(answer?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_ui_core::open_limit::{ASK_ABOVE_BYTES, decide};

    const GB: u64 = 1_000_000_000;

    #[test]
    fn a_small_file_opens_at_once() {
        let mut g = OpenGate::default();
        assert_eq!(g.check("a.kad", decide(1000, Some(8 * GB))), Some("a.kad"));
        assert!(!g.is_pending() && g.text().is_none());
    }

    #[test]
    fn a_big_file_waits_for_the_answer() {
        let mut g = OpenGate::default();
        assert_eq!(g.check("big.kad", decide(GB, Some(16 * GB))), None);
        assert!(g.is_pending());
        let t = g.text().unwrap();
        assert!(t.contains("Open it?"), "{t}");
        // Cancel: gone, and nothing opens.
        assert_eq!(g.answer(false), None);
        assert!(!g.is_pending());
        // Open: the file comes back once, with the size that was agreed to.
        g.check("big.kad", decide(GB, None));
        assert_eq!(g.answer(true), Some(("big.kad", GB)));
        assert_eq!(g.answer(true), None, "answered already");
    }

    #[test]
    fn a_refused_file_never_opens_even_when_the_person_says_open() {
        let mut g = OpenGate::default();
        assert_eq!(g.check(1, decide(20 * GB, Some(8 * GB))), None);
        let t = g.text().unwrap();
        assert!(t.contains("cannot open"), "{t}");
        assert_eq!(g.answer(true), None);
        assert!(!g.is_pending());
    }

    #[test]
    fn a_second_question_waits_in_a_queue_and_is_not_lost() {
        let mut g = OpenGate::default();
        assert_eq!(g.check("one", decide(GB, None)), None);
        assert_eq!(g.check("two", decide(2 * GB, None)), None);
        assert_eq!(g.queued(), 2);
        // A small file in the meantime opens at once; it does not disturb the queue.
        assert_eq!(g.check("small", decide(10, None)), Some("small"));
        assert_eq!(g.queued(), 2);
        assert_eq!(g.answer(true), Some(("one", GB)));
        assert!(g.is_pending());
        assert!(
            g.text().unwrap().contains("2000"),
            "the second question shows now"
        );
        assert_eq!(g.answer(true), Some(("two", 2 * GB)));
        assert!(!g.is_pending());
    }

    #[test]
    fn the_threshold_is_the_one_of_the_decision() {
        let mut g = OpenGate::default();
        assert!(g.check((), decide(ASK_ABOVE_BYTES, None)).is_some());
        assert!(g.check((), decide(ASK_ABOVE_BYTES + 1, None)).is_none());
    }

    #[test]
    fn the_window_shows_and_answers_in_a_frame() {
        let ctx = egui::Context::default();
        let mut g = OpenGate::default();
        g.check(7, decide(GB, None));
        g.check(8, decide(GB, None));
        let _ = ctx.run_ui(Default::default(), |u| {
            let ctx = u.ctx();
            assert_eq!(g.show(ctx), None, "no click: still waiting");
        });
        assert_eq!(g.queued(), 2);
        // A refusal shows the OK button; the question shows two.
        let mut r = OpenGate::default();
        r.check(1, decide(20 * GB, Some(GB)));
        let _ = ctx.run_ui(Default::default(), |u| {
            let ctx = u.ctx();
            assert_eq!(r.show(ctx), None);
        });
        assert!(r.is_pending());
        // Without a question, the window does not show.
        let mut none: OpenGate<u8> = OpenGate::default();
        let _ = ctx.run_ui(Default::default(), |u| {
            let ctx = u.ctx();
            assert_eq!(none.show(ctx), None);
        });
    }
}
