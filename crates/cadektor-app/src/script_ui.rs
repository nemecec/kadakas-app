//! Scripting in the app: the Scripts palette with its console, the parameters of plug-in objects in
//! Object Info, and the error badge on the canvas.
//!
//! Scripts run only when the user presses Run. The script gets the address and token of this app
//! in the environment, so it sees the live document. Output and errors go to the console.

use std::path::{Path, PathBuf};

use cadektor_api::Session;
use cadektor_core::model::{Document, Object, ObjectId};
use cadektor_script::host::{Line, RunSpec, Status, Stream};
use cadektor_script::python;
use egui::{Color32, Ui};
use serde_json::{Map, Value, json};

use crate::view::View;

/// Most console lines the window keeps.
const KEEP_LINES: usize = 5_000;

/// State of the Scripts window and of the plug-in object editor in Object Info.
#[derive(Default)]
pub struct ScriptUi {
    open: bool,
    folder: String,
    files: Vec<PathBuf>,
    selected: Option<PathBuf>,
    run: Option<u64>,
    next_line: usize,
    lines: Vec<Line>,
    status: Option<Status>,
    /// Set by the Run button: the app starts the script.
    want_run: Option<PathBuf>,
    edit: Option<PluginEdit>,
}

/// Working copy of the parameters of the plug-in object in Object Info.
struct PluginEdit {
    id: ObjectId,
    saved: Value,
    working: Map<String, Value>,
    dirty: bool,
}

fn default_folder() -> String {
    std::env::var("CADEKTOR_SCRIPTS_DIR").unwrap_or_else(|_| {
        let here = std::env::current_dir().unwrap_or_default();
        let scripts = here.join("scripts");
        let examples = here.join("python").join("examples");
        [scripts, examples]
            .into_iter()
            .find(|p| p.is_dir())
            .unwrap_or(here)
            .display()
            .to_string()
    })
}

fn list_scripts(folder: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "py"))
        .collect();
    v.sort();
    v
}

impl ScriptUi {
    /// File > Scripts: show the window.
    pub fn open(&mut self) {
        self.open = true;
        if self.folder.is_empty() {
            self.folder = default_folder();
        }
        self.files = list_scripts(Path::new(&self.folder));
    }

    /// Starts a script as the user asked. The app has made sure that a live session address exists.
    pub fn start(&mut self, session: &mut Session, path: PathBuf, status: &mut String) {
        self.lines.clear();
        self.next_line = 0;
        let spec = RunSpec {
            script: path.clone(),
            args: vec![],
            endpoint: session.scripting.endpoint.clone(),
            interpreter: session.scripting.interpreter.clone(),
        };
        match session.scripting.host.start(spec) {
            Ok(id) => {
                self.run = Some(id);
                self.status = Some(Status::Running);
            }
            Err(e) => {
                *status = format!("Script: {e}");
                self.lines.push(Line {
                    n: 0,
                    stream: Stream::Err,
                    text: e,
                });
                self.run = None;
            }
        }
    }

    /// Reads new console lines of the running script.
    fn poll(&mut self, session: &Session) {
        let Some(run) = self.run else { return };
        if let Some((lines, status)) = session.scripting.host.poll(run, self.next_line) {
            if let Some(last) = lines.last() {
                self.next_line = last.n + 1;
            }
            self.lines.extend(lines);
            let excess = self.lines.len().saturating_sub(KEEP_LINES);
            self.lines.drain(..excess);
            self.status = Some(status);
        }
    }

    /// Draws the Scripts window. Returns the script that the user wants to run.
    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) -> Option<PathBuf> {
        self.poll(session);
        let running = self.status.as_ref().is_some_and(Status::is_running);
        if running {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !self.open {
            return self.want_run.take();
        }
        let mut open = self.open;
        egui::Window::new("Scripts")
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| self.window_body(ui, session, running));
        self.open = open;
        self.want_run.take()
    }

    fn window_body(&mut self, ui: &mut Ui, session: &mut Session, running: bool) {
        match python::discover() {
            Some(py) => ui.small(format!("Python: {}", py.describe())),
            None => ui.colored_label(
                Color32::from_rgb(200, 40, 40),
                "No Python found. Install python3 or uv, or set CADEKTOR_PYTHON.",
            ),
        };
        ui.small("A script runs only when you press Run. It can change the open document.");
        ui.horizontal(|ui| {
            ui.label("Folder");
            let r = ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(300.0));
            if r.lost_focus() {
                self.files = list_scripts(Path::new(&self.folder));
            }
            let chosen = if ui.button("Choose...").clicked() {
                rfd::FileDialog::new().pick_folder()
            } else {
                None
            };
            if let Some(dir) = chosen {
                self.folder = dir.display().to_string();
                self.files = list_scripts(&dir);
            }
            if ui.button("Refresh").clicked() {
                self.files = list_scripts(Path::new(&self.folder));
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("script_files")
            .max_height(120.0)
            .show(ui, |ui| {
                if self.files.is_empty() {
                    ui.weak("No .py files in this folder.");
                }
                for f in &self.files {
                    let name = f
                        .file_name()
                        .map_or(String::new(), |n| n.to_string_lossy().into());
                    let sel = self.selected.as_ref() == Some(f);
                    if ui.selectable_label(sel, name).clicked() {
                        self.selected = Some(f.clone());
                    }
                }
            });
        ui.horizontal(|ui| {
            let can_run = self.selected.is_some() && !running;
            if ui.add_enabled(can_run, egui::Button::new("Run")).clicked() {
                self.want_run = self.selected.clone();
            }
            let stop = ui.add_enabled(running, egui::Button::new("Stop")).clicked();
            if let (true, Some(id)) = (stop, self.run) {
                session.scripting.host.stop(id);
            }
            if ui.button("Clear console").clicked() {
                self.lines.clear();
            }
            if let Some(st) = &self.status {
                ui.label(st.name());
            }
        });
        ui.separator();
        ui.strong("Console");
        egui::ScrollArea::vertical()
            .id_salt("script_console")
            .stick_to_bottom(true)
            .max_height(260.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for l in &self.lines {
                    let text = egui::RichText::new(&l.text).monospace();
                    ui.label(match l.stream {
                        Stream::Out => text,
                        Stream::Err => text.color(Color32::from_rgb(200, 40, 40)),
                        Stream::Info => text.weak().italics(),
                    });
                }
            });
    }

    // ------------------------------------------------------------------ Object Info

    /// The parameters of the selected plug-in object, in Object Info. Does nothing for other objects.
    pub fn plugin_info(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        let [id] = session.selection[..] else {
            self.edit = None;
            return;
        };
        let Some(info) = cadektor_api::instance_info(session, id) else {
            self.edit = None;
            return;
        };
        let saved = info["params"].clone();
        if self
            .edit
            .as_ref()
            .is_none_or(|e| e.id != id || e.saved != saved)
        {
            self.edit = Some(PluginEdit {
                id,
                working: saved.as_object().cloned().unwrap_or_default(),
                saved,
                dirty: false,
            });
        }
        let edit = self.edit.as_mut().expect("set above");
        ui.separator();
        ui.strong(format!(
            "Plug-in object: {}",
            info["name"].as_str().unwrap_or("")
        ));
        if let Some(err) = info["error"].as_str() {
            ui.colored_label(Color32::from_rgb(200, 40, 40), "\u{26a0} The script failed")
                .on_hover_text(err);
            ui.small(err);
            ui.small("The shapes are from the last good run.");
        }
        let schema = info["schema"].as_array().cloned().unwrap_or_default();
        for p in &schema {
            let name = p["name"].as_str().unwrap_or("");
            let mut value = edit.working.get(name).cloned().unwrap_or(Value::Null);
            if param_widget(ui, p, &mut value, name) {
                edit.dirty = true;
            }
            edit.working.insert(name.to_string(), value);
        }
        let settled = !ui.input(|i| i.pointer.any_down()) && ui.memory(|m| m.focused().is_none());
        let reset = ui
            .button("Reset")
            .on_hover_text("Run the script again")
            .clicked();
        if (edit.dirty && settled) || reset {
            let changed: Map<String, Value> = edit
                .working
                .iter()
                .filter(|(k, v)| edit.saved.get(k.as_str()) != Some(v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            edit.dirty = false;
            match cadektor_api::user_set_params(session, id, &changed) {
                Ok(r) if r["ok"] == json!(false) => {
                    *status = format!("Plug-in object: {}", r["error"].as_str().unwrap_or("error"));
                }
                Ok(_) => {}
                Err(e) => {
                    *status = format!("Plug-in object: {}", e.message);
                    // Go back to the saved values.
                    self.edit = None;
                }
            }
            session
                .selection
                .retain(|id| session.editor.doc().object(*id).is_some());
        }
    }
}

fn parse_color(s: &str) -> Color32 {
    let h = s.trim_start_matches('#');
    if h.len() != 6 && h.len() != 8 {
        return Color32::BLACK;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    let a = if h.len() >= 8 { byte(6) } else { 255 };
    Color32::from_rgba_unmultiplied(byte(0), byte(2), byte(4), a)
}

fn color_text(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// One editor for one parameter. Returns true if the user changed the value.
fn param_widget(ui: &mut Ui, p: &Value, value: &mut Value, id_salt: &str) -> bool {
    let label = p["label"]
        .as_str()
        .filter(|l| !l.is_empty())
        .or(p["name"].as_str())
        .unwrap_or("");
    let mut changed = false;
    match p["type"].as_str().unwrap_or("") {
        ty @ ("number" | "length") => {
            let mut x = value.as_f64().unwrap_or(0.0);
            let lo = p["min"].as_f64().unwrap_or(f64::MIN);
            let hi = p["max"].as_f64().unwrap_or(f64::MAX);
            ui.horizontal(|ui| {
                ui.label(label);
                let mut d = egui::DragValue::new(&mut x)
                    .range(lo..=hi)
                    .speed(if ty == "length" { 10.0 } else { 0.1 });
                if ty == "length" {
                    d = d.suffix(" mm");
                }
                if ui.add(d).changed() {
                    changed = true;
                    *value = json!(x);
                }
            });
        }
        "text" => {
            let mut s = value.as_str().unwrap_or("").to_string();
            ui.horizontal(|ui| {
                ui.label(label);
                if ui.text_edit_singleline(&mut s).changed() {
                    changed = true;
                    *value = json!(s);
                }
            });
        }
        "bool" => {
            let mut b = value.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, label).changed() {
                changed = true;
                *value = json!(b);
            }
        }
        "choice" => {
            let current = value.as_str().unwrap_or("").to_string();
            ui.horizontal(|ui| {
                ui.label(label);
                egui::ComboBox::from_id_salt(id_salt)
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for o in p["options"].as_array().into_iter().flatten() {
                            let o = o.as_str().unwrap_or("");
                            if ui.selectable_label(o == current, o).clicked() && o != current {
                                changed = true;
                                *value = json!(o);
                            }
                        }
                    });
            });
        }
        "color" => {
            let mut c = parse_color(value.as_str().unwrap_or("#000000"));
            ui.horizontal(|ui| {
                ui.label(label);
                if ui.color_edit_button_srgba(&mut c).changed() {
                    changed = true;
                    *value = json!(color_text(c));
                }
            });
        }
        "record" => {
            let mut fields = value.as_object().cloned().unwrap_or_default();
            ui.collapsing(label, |ui| {
                for f in p["fields"].as_array().into_iter().flatten() {
                    let name = f["name"].as_str().unwrap_or("");
                    let mut v = fields.get(name).cloned().unwrap_or(Value::Null);
                    if param_widget(ui, f, &mut v, &format!("{id_salt}.{name}")) {
                        changed = true;
                    }
                    fields.insert(name.to_string(), v);
                }
            });
            *value = Value::Object(fields);
        }
        _ => {}
    }
    changed
}

/// The plug-in error of an object, if its last reset failed.
pub fn plugin_error(o: &Object) -> Option<&str> {
    o.data
        .get(cadektor_api::PLUGIN_DATA_KEY)?
        .get("error")?
        .as_str()
}

/// Draws a red mark at the top right corner of a plug-in object whose script failed.
pub fn draw_badge(painter: &egui::Painter, view: &View, doc: &Document, o: &Object) {
    if plugin_error(o).is_none() {
        return;
    }
    let (lo, hi) = doc.object_bounds(o);
    if !(lo.is_finite() && hi.is_finite()) {
        return;
    }
    let at = view.to_screen(glam::DVec2::new(hi.x, hi.y)) + egui::vec2(8.0, -8.0);
    painter.circle_filled(at, 8.0, Color32::from_rgb(200, 40, 40));
    painter.text(
        at,
        egui::Align2::CENTER_CENTER,
        "!",
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        assert_eq!(color_text(parse_color("#8b5a2b")), "#8b5a2b");
        assert_eq!(parse_color("#00000080").a(), 128);
        assert_eq!(parse_color("#ff0000"), Color32::from_rgb(255, 0, 0));
        assert_eq!(parse_color("bad"), Color32::from_rgb(0, 0, 0));
    }

    fn frame(f: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        let mut f = f;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            f(ui);
        });
    }

    #[test]
    fn object_info_shows_parameters_and_the_error_badge() {
        let mut s = Session::default();
        s.handle("plugin.install_example", json!({"name": "Bench"}))
            .unwrap();
        let g = s
            .handle("plugin.place", json!({"definition": "Bench", "at": [0, 0]}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let id = ObjectId(g);
        let mut ui_state = ScriptUi::default();
        let mut status = String::new();
        s.selection = vec![id];
        frame(|ui| ui_state.plugin_info(ui, &mut s, &mut status));
        let edit = ui_state
            .edit
            .as_ref()
            .expect("editor for the plug-in object");
        assert_eq!(edit.working["slats"], json!(5.0));
        assert!(plugin_error(s.editor.doc().object(id).unwrap()).is_none());
        // A selection that is not a plug-in object clears the editor.
        s.selection = vec![];
        frame(|ui| ui_state.plugin_info(ui, &mut s, &mut status));
        assert!(ui_state.edit.is_none());
        // The error mark shows when the data has an error.
        s.editor
            .execute(&cadektor_core::command::Command::SetData {
                id,
                set: [(
                    cadektor_api::PLUGIN_DATA_KEY.to_string(),
                    json!({"error": "boom"}),
                )]
                .into(),
                remove: vec![],
            })
            .unwrap();
        assert_eq!(
            plugin_error(s.editor.doc().object(id).unwrap()),
            Some("boom")
        );
        let view = View::default();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_badge(
                ui.painter(),
                &view,
                s.editor.doc(),
                s.editor.doc().object(id).unwrap(),
            );
        });
    }

    #[test]
    fn scripts_window_runs_a_script_and_shows_output() {
        let dir = std::env::temp_dir().join(format!("cadektor-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.py");
        std::fs::write(&file, "print('hello from a script')\n").unwrap();
        assert_eq!(list_scripts(&dir), vec![file.clone()]);
        let mut s = Session::default();
        s.scripting.endpoint = Some(("127.0.0.1:1".into(), "t".into()));
        let mut ui_state = ScriptUi::default();
        ui_state.open();
        let mut status = String::new();
        ui_state.start(&mut s, file, &mut status);
        let ctx = egui::Context::default();
        for _ in 0..200 {
            let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
            ui_state.show(&ctx, &mut s);
            if ui_state.status.as_ref().is_some_and(|s| !s.is_running()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            ui_state
                .lines
                .iter()
                .any(|l| l.text == "hello from a script")
        );
        // A missing file is reported in the status line and the console.
        ui_state.start(&mut s, dir.join("missing.py"), &mut status);
        assert!(status.contains("not a file"));
    }
}
