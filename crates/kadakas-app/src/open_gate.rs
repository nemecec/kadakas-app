//! The question before a big file opens (DOC-19): "This file is N MB and needs about M GB of memory
//! to open. Open it?", with Open and Cancel, and the refusal of a file that the computer cannot hold.
//!
//! The decision is `kadakas_ui_core::open_limit::decide`. This module holds the state of the
//! question and the window. One gate serves one way into a file. The way passes what it wants to open
//! (`T`: a path, an index, a request) to [`OpenGate::check`], and opens it when `check` gives it
//! back at once or when the person answers Open in [`OpenGate::show`].

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
    question: Option<Question<T>>,
}

impl<T> Default for OpenGate<T> {
    fn default() -> Self {
        OpenGate { question: None }
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
        self.question = Some(Question {
            what,
            estimate,
            refused,
        });
        None
    }

    /// True while a question or a refusal is on the screen.
    pub fn is_pending(&self) -> bool {
        self.question.is_some()
    }

    /// The text of the question or the refusal.
    pub fn text(&self) -> Option<String> {
        self.question.as_ref().map(|q| {
            if q.refused {
                q.estimate.refuse_text()
            } else {
                q.estimate.ask_text()
            }
        })
    }

    /// The answer of the person. `open` true gives back what was asked about, but never for a
    /// refusal. Either way the question is gone.
    pub fn answer(&mut self, open: bool) -> Option<T> {
        let q = self.question.take()?;
        (open && !q.refused).then_some(q.what)
    }

    /// Shows the window. Returns what the person agreed to open.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<T> {
        let q = self.question.as_ref()?;
        let (title, refused) = if q.refused {
            (tr!("open-gate-refused-title"), true)
        } else {
            (tr!("open-gate-ask-title"), false)
        };
        let text = self.text().unwrap_or_default();
        let mut answer = None;
        egui::Window::new(title)
            .id(egui::Id::new("win-open-big-file"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(text);
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

    const GB: u64 = 1024 * 1024 * 1024;

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
        // Open: the file comes back once.
        g.check("big.kad", decide(GB, None));
        assert_eq!(g.answer(true), Some("big.kad"));
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
        let _ = ctx.run_ui(Default::default(), |u| {
            let ctx = u.ctx();
            assert_eq!(g.show(ctx), None, "no click: still waiting");
        });
        assert!(g.is_pending());
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
