//! File > BCF Issues...: write the issues as a BCF file and read the issues of one (REV-06).
//!
//! The dialog calls `issue.export_bcf` and `issue.import_bcf`, so the app and an agent use one
//! code path. A file that holds an issue with the same GUID as an issue in the drawing, but other
//! content, is a conflict: the import changed nothing and the dialog asks. The choice goes back
//! as `on_conflict`. Nothing is overwritten without that choice. Items of the file that do not
//! fit the limits (a comment, a picture, a camera) are listed and need the choice too: the import
//! changed nothing until the person says to import without them (`skip_items`).

use std::path::PathBuf;

use egui::Color32;
use kadakas_api::{REASON_BCF_CONFLICT, REASON_BCF_UNFIT, RpcError, Session};
use kadakas_l10n::{msg, tr, translate};
use serde_json::{Value, json};

/// An import that waits for the choice of the person.
struct Conflict {
    path: PathBuf,
    count: usize,
    /// For each issue: the title here, the status here, the title in the file, the status there.
    rows: Vec<[String; 4]>,
}

/// An import that found items that do not fit, and waits for the person.
struct Unfit {
    path: PathBuf,
    /// One line for each item: what it is, where, and why.
    rows: Vec<String>,
}

/// State of the dialog.
#[derive(Default)]
pub struct BcfDialog {
    version_21: bool,
    only_open: bool,
    /// What happened last: true for good news.
    message: Option<(bool, String)>,
    notes: Vec<String>,
    conflict: Option<Conflict>,
    unfit: Option<Unfit>,
    /// The person agreed to import without the items that do not fit.
    skip_items: bool,
}

fn status_name(v: &Value) -> String {
    v.as_str()
        .and_then(kadakas_core::issue::IssueStatus::parse)
        .map(crate::issues_ui::status_text)
        .unwrap_or_default()
}

impl BcfDialog {
    #[cfg(test)]
    pub fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }

    #[cfg(test)]
    pub fn has_unfit(&self) -> bool {
        self.unfit.is_some()
    }

    #[cfg(test)]
    pub fn unfit_rows(&self) -> Vec<String> {
        self.unfit
            .as_ref()
            .map(|u| u.rows.clone())
            .unwrap_or_default()
    }

    /// Takes the answer to the question about unfit items, as the button does.
    #[cfg(test)]
    pub fn skip_items_for_test(&mut self, session: &mut Session, status: &mut String) {
        if let Some(u) = self.unfit.take() {
            self.skip_items = true;
            self.run_import(session, status, u.path, None);
        }
    }

    #[cfg(test)]
    pub fn message(&self) -> Option<&(bool, String)> {
        self.message.as_ref()
    }

    #[cfg(test)]
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Takes the answer to a conflict, as the buttons do.
    #[cfg(test)]
    pub fn choose_for_test(&mut self, session: &mut Session, status: &mut String, choice: &str) {
        if let Some(c) = self.conflict.take() {
            self.run_import(session, status, c.path, Some(choice));
        }
    }

    /// Draws the dialog. Returns false when the person closed it.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        status: &mut String,
    ) -> bool {
        let mut open = true;
        egui::Window::new(tr!("issues-ui-bcf-title"))
            .id(egui::Id::new("win-bcf"))
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.small(tr!("issues-ui-bcf-intro"));
                ui.separator();
                ui.strong(tr!("issues-ui-bcf-export-heading"));
                ui.horizontal(|ui| {
                    ui.label(tr!("issues-ui-bcf-version"));
                    ui.selectable_value(&mut self.version_21, false, "3.0");
                    ui.selectable_value(&mut self.version_21, true, "2.1");
                });
                ui.checkbox(&mut self.only_open, tr!("issues-ui-bcf-only-open"));
                if ui.button(tr!("issues-ui-bcf-export")).clicked() {
                    self.export(session, status);
                }
                ui.separator();
                ui.strong(tr!("issues-ui-bcf-import-heading"));
                if ui.button(tr!("issues-ui-bcf-import")).clicked() {
                    self.import(session, status);
                }
                self.unfit_ui(ui, session, status);
                self.conflict_ui(ui, session, status);
                if let Some((good, text)) = &self.message {
                    let c = if *good {
                        ui.visuals().text_color()
                    } else {
                        Color32::from_rgb(200, 60, 40)
                    };
                    ui.colored_label(c, text);
                }
                if !self.notes.is_empty() {
                    egui::CollapsingHeader::new(tr!(
                        "issues-ui-bcf-notes",
                        count = self.notes.len()
                    ))
                    .id_salt("bcf-notes")
                    .show(ui, |ui| {
                        for n in &self.notes {
                            ui.small(n);
                        }
                    });
                }
            });
        open
    }

    fn fail(&mut self, e: &RpcError) {
        self.message = Some((false, tr!("issues-ui-bcf-error", e = e.message.clone())));
        self.notes.clear();
    }

    fn export(&mut self, session: &mut Session, status: &mut String) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr!("issues-ui-bcf-filter"), &["bcfzip"])
            .set_file_name("issues.bcfzip")
            .save_file()
        else {
            return;
        };
        let mut p = json!({
            "path": path.display().to_string(),
            "version": if self.version_21 { "2.1" } else { "3.0" },
            "overwrite": true,
        });
        if self.only_open {
            p["open_only"] = json!(true);
        }
        match session.handle("issue.export_bcf", p) {
            Ok(r) => {
                let text = tr!(
                    "issues-ui-bcf-exported",
                    topics = r["topics"].as_u64().unwrap_or(0),
                    path = path.display().to_string()
                );
                *status = text.clone();
                self.message = Some((true, text));
                self.notes = r["notes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|n| n.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                self.conflict = None;
            }
            Err(e) => self.fail(&e),
        }
    }

    fn import(&mut self, session: &mut Session, status: &mut String) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr!("issues-ui-bcf-filter"), &["bcfzip", "zip"])
            .pick_file()
        else {
            return;
        };
        self.skip_items = false;
        self.run_import(session, status, path, None);
    }

    /// Reads the file. `choice` is the answer to a conflict.
    pub fn run_import(
        &mut self,
        session: &mut Session,
        status: &mut String,
        path: PathBuf,
        choice: Option<&str>,
    ) {
        let mut p = json!({
            "path": path.display().to_string(),
            "now_ms": crate::issues_ui::now_ms(),
            "author": session_author(),
        });
        if let Some(c) = choice {
            p["on_conflict"] = json!(c);
        }
        if self.skip_items {
            p["skip_items"] = json!(true);
        }
        self.conflict = None;
        self.unfit = None;
        match session.handle("issue.import_bcf", p) {
            Ok(r) => {
                let n = |k: &str| r[k].as_u64().unwrap_or(0);
                let text = tr!(
                    "issues-ui-bcf-imported",
                    imported = n("imported") + n("kept_both"),
                    replaced = n("replaced"),
                    unchanged = n("unchanged"),
                    skipped = n("skipped_conflicts")
                        + r["topics_skipped"].as_array().map_or(0, |a| a.len() as u64)
                );
                *status = text.clone();
                self.message = Some((true, text));
                let mut notes: Vec<String> = Vec::new();
                for s in r["topics_skipped"].as_array().into_iter().flatten() {
                    notes.push(format!(
                        "{}: {}",
                        s["topic"].as_str().unwrap_or(""),
                        s["reason"].as_str().unwrap_or("")
                    ));
                }
                notes.extend(
                    r["warnings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|w| w.as_str().map(str::to_string)),
                );
                for s in r["items_skipped"].as_array().into_iter().flatten() {
                    notes.push(unfit_row(s));
                }
                self.notes = notes;
                self.skip_items = false;
            }
            Err(e) if e.reason == Some(REASON_BCF_UNFIT) => {
                let d = e.details.clone().unwrap_or(Value::Null);
                self.unfit = Some(Unfit {
                    path,
                    rows: d["unfit"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(unfit_row)
                        .collect(),
                });
                self.message = None;
                self.notes.clear();
            }
            Err(e) if e.reason == Some(REASON_BCF_CONFLICT) => {
                let d = e.details.clone().unwrap_or(Value::Null);
                let rows = d["conflicts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        [
                            c["ours"]["title"].as_str().unwrap_or("").to_string(),
                            status_name(&c["ours"]["status"]),
                            c["file"]["title"].as_str().unwrap_or("").to_string(),
                            status_name(&c["file"]["status"]),
                        ]
                    })
                    .collect::<Vec<_>>();
                self.conflict = Some(Conflict {
                    path,
                    count: rows.len(),
                    rows,
                });
                self.message = None;
                self.notes.clear();
            }
            Err(e) => self.fail(&e),
        }
    }

    fn unfit_ui(&mut self, ui: &mut egui::Ui, session: &mut Session, status: &mut String) {
        let Some(u) = &self.unfit else { return };
        ui.separator();
        ui.strong(tr!("issues-ui-bcf-unfit-title"));
        ui.label(tr!("issues-ui-bcf-unfit-text", count = u.rows.len()));
        egui::ScrollArea::vertical()
            .id_salt("bcf-unfit")
            .max_height(140.0)
            .show(ui, |ui| {
                for r in &u.rows {
                    ui.small(r);
                }
            });
        let path = u.path.clone();
        let mut go = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("issues-ui-bcf-unfit-skip")).clicked() {
                go = true;
            }
            if ui.button(tr!("issues-ui-cancel")).clicked() {
                self.unfit = None;
            }
        });
        if go {
            self.skip_items = true;
            self.run_import(session, status, path, None);
        }
    }

    fn conflict_ui(&mut self, ui: &mut egui::Ui, session: &mut Session, status: &mut String) {
        let Some(c) = &self.conflict else { return };
        ui.separator();
        ui.strong(tr!("issues-ui-bcf-conflict-title"));
        ui.label(tr!("issues-ui-bcf-conflict-text", count = c.count));
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .show(ui, |ui| {
                for [ours, ours_status, file, file_status] in &c.rows {
                    ui.small(tr!(
                        "issues-ui-bcf-conflict-row",
                        ours = format!("{ours} ({ours_status})"),
                        file = format!("{file} ({file_status})")
                    ));
                }
            });
        let path = c.path.clone();
        let mut choice: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("issues-ui-bcf-keep-mine")).clicked() {
                choice = Some("skip");
            }
            if ui.button(tr!("issues-ui-bcf-take-file")).clicked() {
                choice = Some("replace");
            }
            if ui.button(tr!("issues-ui-bcf-keep-both")).clicked() {
                choice = Some("keep_both");
            }
            if ui.button(tr!("issues-ui-cancel")).clicked() {
                self.conflict = None;
            }
        });
        if let Some(choice) = choice {
            self.run_import(session, status, path, Some(choice));
        }
    }
}

/// One line for an item that does not fit: the topic, what it is and why.
fn unfit_row(u: &Value) -> String {
    let item = translate(
        match u["item"].as_str() {
            Some("comment") => msg!("issues-ui-bcf-unfit-comment"),
            Some("snapshot") => msg!("issues-ui-bcf-unfit-snapshot"),
            _ => msg!("issues-ui-bcf-unfit-camera"),
        },
        &[],
    );
    tr!(
        "issues-ui-bcf-unfit-row",
        topic = u["topic"].as_str().unwrap_or("").to_string(),
        item = item,
        reason = u["reason"].as_str().unwrap_or("").to_string()
    )
}

fn session_author() -> String {
    for key in ["USER", "USERNAME"] {
        if let Ok(v) = std::env::var(key)
            && !v.trim().is_empty()
        {
            return v.trim().to_string();
        }
    }
    tr!("issues-ui-author-default")
}
