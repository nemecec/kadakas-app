//! The run of a plug-in tool in the UI: the state between events, without egui (PLAT-10).
//!
//! A plug-in tool (kind `tool`) is not a state machine of this crate. The module is the state
//! machine, and it keeps its progress in a `state` value that the host sends back with every
//! event. This module holds that value for one run, thins out pointer moves, remembers the preview
//! and the hint, and decides what a result or an error means for the run. The UI code calls the
//! plug-in host (through the API) and gives the answer to [`ToolRun::apply`].
//!
//! The type `D` is what a finished run delivers (the checked shapes or object of the API). This
//! crate does not look inside it.

use glam::DVec2;
use kadakas_core::model::Shape;
use serde_json::Value;

/// A plug-in that offers a tool, as the "Plug-ins" tool set lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolEntry {
    /// The id of the plug-in.
    pub plugin: String,
    /// The name to show on the button.
    pub name: String,
    /// True if the user trusted the module. An untrusted tool opens the trust dialog.
    pub trusted: bool,
}

/// What the module answered to one event, already checked by the host.
#[derive(Clone, Debug, PartialEq)]
pub struct StepOutput<D> {
    pub state: Value,
    pub preview: Vec<Shape>,
    pub hint: String,
    pub done: Option<D>,
}

/// What a step means for the run.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<D> {
    /// The run goes on.
    Continue,
    /// The run ended with a result. The run is over.
    Done(D),
    /// The module failed (a limit, a crash, a bad answer, or its own message). The run is over,
    /// so a module that hangs or crashes is not asked again for every mouse move.
    Failed(String),
}

/// A pointer must move at least this far (in mm of the drawing) for the next `move` event.
/// The UI passes a distance of a few pixels.
pub const MIN_MOVE_PIXELS: f64 = 2.0;

/// One run of a tool.
#[derive(Clone, Debug)]
pub struct ToolRun {
    plugin: String,
    state: Value,
    hint: String,
    preview: Vec<Shape>,
    last_move: Option<DVec2>,
    over: bool,
}

impl ToolRun {
    /// A run of the tool of this plug-in. The first event to send is `start`.
    pub fn new(plugin: &str) -> ToolRun {
        ToolRun {
            plugin: plugin.to_string(),
            state: Value::Null,
            hint: String::new(),
            preview: Vec::new(),
            last_move: None,
            over: false,
        }
    }

    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// What the module wants back with the next event.
    pub fn state(&self) -> &Value {
        &self.state
    }

    /// The text for the status bar.
    pub fn hint(&self) -> &str {
        &self.hint
    }

    /// The shapes to draw on the canvas. They are not in the document.
    pub fn preview(&self) -> &[Shape] {
        &self.preview
    }

    /// True after a result or an error. A run that is over takes no more events.
    pub fn is_over(&self) -> bool {
        self.over
    }

    /// The pointer moved. Returns the point to send as a `move` event, or `None` if the pointer
    /// has not moved enough since the last event (`pixel` is the size of a pixel in mm). This keeps
    /// the number of calls to the module small.
    pub fn move_to(&mut self, p: DVec2, pixel: f64) -> Option<DVec2> {
        if self.over || !p.is_finite() {
            return None;
        }
        if self
            .last_move
            .is_some_and(|q| q.distance(p) < MIN_MOVE_PIXELS * pixel)
        {
            return None;
        }
        self.last_move = Some(p);
        Some(p)
    }

    /// A click was sent: the next move is new, wherever the pointer is.
    pub fn clicked(&mut self) {
        self.last_move = None;
    }

    /// Takes the answer to an event.
    pub fn apply<D>(&mut self, answer: Result<StepOutput<D>, String>) -> Outcome<D> {
        match answer {
            Err(message) => {
                self.finish();
                Outcome::Failed(message)
            }
            Ok(out) => {
                self.state = out.state;
                self.preview = out.preview;
                self.hint = out.hint;
                match out.done {
                    Some(d) => {
                        self.finish();
                        Outcome::Done(d)
                    }
                    None => Outcome::Continue,
                }
            }
        }
    }

    /// A step that failed but does not end the run: the module refused an input (for example a
    /// click that is too close). The state and the preview stay as they were.
    pub fn refused(&mut self, message: &str) {
        self.hint = message.to_string();
    }

    /// Escape: drops the state and the preview. The module is not asked.
    pub fn cancel(&mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        self.over = true;
        self.state = Value::Null;
        self.preview.clear();
        self.last_move = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn out(done: Option<&'static str>) -> StepOutput<&'static str> {
        StepOutput {
            state: json!({"start": [0, 0]}),
            preview: vec![Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            }],
            hint: "Click the end".into(),
            done,
        }
    }

    #[test]
    fn a_run_keeps_the_state_the_preview_and_the_hint() {
        let mut run = ToolRun::new("org.example.row");
        assert_eq!(run.plugin(), "org.example.row");
        assert!(run.state().is_null() && run.preview().is_empty() && !run.is_over());
        assert_eq!(run.apply(Ok(out(None))), Outcome::Continue);
        assert_eq!(run.state(), &json!({"start": [0, 0]}));
        assert_eq!(run.preview().len(), 1);
        assert_eq!(run.hint(), "Click the end");
        assert!(!run.is_over());
    }

    #[test]
    fn a_result_ends_the_run_and_clears_it() {
        let mut run = ToolRun::new("p");
        assert_eq!(run.apply(Ok(out(Some("shapes")))), Outcome::Done("shapes"));
        assert!(run.is_over() && run.state().is_null() && run.preview().is_empty());
        assert!(run.move_to(DVec2::ONE, 1.0).is_none());
    }

    #[test]
    fn an_error_ends_the_run_with_the_message() {
        let mut run = ToolRun::new("p");
        run.apply(Ok(out(None)));
        let o: Outcome<&str> = run.apply(Err("the plug-in crashed".into()));
        assert_eq!(o, Outcome::Failed("the plug-in crashed".into()));
        assert!(run.is_over() && run.preview().is_empty());
    }

    #[test]
    fn a_refused_input_keeps_the_run_going() {
        let mut run = ToolRun::new("p");
        run.apply(Ok(out(None)));
        run.refused("too close");
        assert_eq!(run.hint(), "too close");
        assert!(!run.is_over() && run.preview().len() == 1);
    }

    #[test]
    fn escape_drops_everything() {
        let mut run = ToolRun::new("p");
        run.apply(Ok(out(None)));
        run.cancel();
        assert!(run.is_over() && run.state().is_null() && run.preview().is_empty());
    }

    #[test]
    fn small_moves_are_thinned_out() {
        let mut run = ToolRun::new("p");
        // The first move is always sent.
        assert!(run.move_to(DVec2::new(0.0, 0.0), 1.0).is_some());
        assert!(run.move_to(DVec2::new(1.0, 0.0), 1.0).is_none());
        assert!(run.move_to(DVec2::new(5.0, 0.0), 1.0).is_some());
        // A pixel of 10 mm: 15 mm is not enough.
        assert!(run.move_to(DVec2::new(20.0, 0.0), 10.0).is_none());
        assert!(run.move_to(DVec2::new(30.0, 0.0), 10.0).is_some());
        // After a click the next move is sent.
        run.clicked();
        assert!(run.move_to(DVec2::new(30.0, 0.0), 10.0).is_some());
        // A point that is not a number is not sent.
        assert!(run.move_to(DVec2::new(f64::NAN, 0.0), 1.0).is_none());
    }

    #[test]
    fn the_entry_is_plain_data() {
        let e = ToolEntry {
            plugin: "a".into(),
            name: "A".into(),
            trusted: false,
        };
        assert_eq!(e.clone(), e);
    }
}
