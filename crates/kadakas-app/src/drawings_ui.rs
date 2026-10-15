//! File > Compare Drawings... (IOP-24).
//!
//! The person picks two DXF or DWG files. The window shows the overlay of the changes in a picture
//! (added objects green, changed orange, removed objects as ghosts, each with a line pattern and a
//! mark), with an optional revision cloud around each cluster of changes, and the list of the
//! changes. The picture can be exported as PNG, SVG or PDF. Neither file is imported: the open
//! document is not touched.
//!
//! The window only draws and asks. The comparison and the pictures are `dxf.compare` of
//! `kadakas-api` (`Session::dxf_compare_trusted`), so the app and an agent get the same picture.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use egui::{Color32, Ui};
use kadakas_api::Session;
use kadakas_l10n::tr;
use kadakas_review::overlay::{CompareStyle, Palette, Status, UnchangedMode};
use serde_json::{Value, json};

use crate::compare_ui::swatch;

/// The width of the picture that the window shows, in pixels.
const PICTURE_WIDTH: u32 = 1100;
/// The most rows of the list that are drawn.
const LIST_ROWS: usize = 200;
/// The width of an exported picture, in pixels.
const EXPORT_WIDTH: u32 = 2400;
const EXPORT_HEIGHT: u32 = 1700;

/// What the comparison was made with. The picture is made again when this changes.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    base: PathBuf,
    new: PathBuf,
    clouds: bool,
    palette: Palette,
    unchanged: UnchangedMode,
    dark: bool,
}

/// The picture and the facts of one comparison.
struct Shown {
    key: Key,
    texture: Option<egui::TextureHandle>,
    counts: [u64; 3],
    unchanged: u64,
    rows: Vec<Row>,
    total: usize,
    clouds: u64,
    notes: Vec<String>,
}

struct Row {
    status: Status,
    text: String,
}

/// State of the window.
#[derive(Default)]
pub struct DrawingsUi {
    open: bool,
    base: Option<PathBuf>,
    new: Option<PathBuf>,
    clouds: bool,
    style: CompareStyle,
    dark: bool,
    shown: Option<Shown>,
    error: Option<String>,
    dirty: bool,
}

fn pick(title: String) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter(tr!("update-ui-filter"), &["dxf", "dwg"])
        .add_filter("DXF", &["dxf"])
        .add_filter(tr!("io-dialog-dwg-needs-a-converter"), &["dwg"])
        .pick_file()
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

impl DrawingsUi {
    /// File > Compare Drawings...: asks for the old file, then the new one.
    pub fn start(&mut self) {
        let Some(base) = pick(tr!("drawings-ui-pick-old")) else {
            return;
        };
        let Some(new) = pick(tr!("drawings-ui-pick-new")) else {
            return;
        };
        self.begin(base, new);
    }

    /// Compares the two files.
    fn begin(&mut self, base: PathBuf, new: PathBuf) {
        self.base = Some(base);
        self.new = Some(new);
        self.open = true;
        self.dirty = true;
    }

    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.dirty = true;
        }
    }

    /// The options of `render` for a format.
    fn render_options(&self, format: &str, dark: bool) -> Value {
        let mut r = json!({
            "format": format,
            "revision_clouds": self.clouds,
            "palette": self.style.palette.name(),
            "unchanged": self.style.unchanged.name(),
            "dark": dark,
        });
        if format == "png" || format == "pdf" {
            r["width"] = json!(EXPORT_WIDTH);
            r["height"] = json!(EXPORT_HEIGHT);
        } else {
            r["width"] = json!(PICTURE_WIDTH);
        }
        r
    }

    fn key(&self) -> Option<Key> {
        Some(Key {
            base: self.base.clone()?,
            new: self.new.clone()?,
            clouds: self.clouds,
            palette: self.style.palette,
            unchanged: self.style.unchanged,
            dark: self.dark,
        })
    }

    /// Compares and draws the picture again, if the files or the options changed.
    fn update(&mut self, ctx: &egui::Context, session: &mut Session) {
        let Some(key) = self.key() else { return };
        if !self.dirty && self.shown.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        self.dirty = false;
        self.error = None;
        let params = json!({
            "base": key.base,
            "new": key.new,
            "options": {"limit": LIST_ROWS},
            "render": self.render_options("svg", key.dark),
        });
        match session.dxf_compare_trusted(params) {
            Ok(r) => self.shown = Some(shown_of(ctx, key, &r)),
            Err(e) => {
                self.shown = None;
                self.error = Some(e.message);
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        if !self.open {
            return;
        }
        self.update(ctx, session);
        let mut open = self.open;
        egui::Window::new(tr!("drawings-ui-title"))
            .id(egui::Id::new("win-compare-drawings"))
            .open(&mut open)
            .default_width(760.0)
            .default_height(640.0)
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.body(ui, session, status));
            });
        self.open = open;
        if !open {
            self.shown = None;
        }
    }

    fn body(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.small(tr!("drawings-ui-intro"));
        let (Some(base), Some(new)) = (self.base.clone(), self.new.clone()) else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("drawings-ui-old", name = name_of(&base)));
            if ui.button(tr!("drawings-ui-change")).clicked()
                && let Some(p) = pick(tr!("drawings-ui-pick-old"))
            {
                self.base = Some(p);
            }
            ui.label(tr!("drawings-ui-new", name = name_of(&new)));
            if ui.button(tr!("drawings-ui-change")).clicked()
                && let Some(p) = pick(tr!("drawings-ui-pick-new"))
            {
                self.new = Some(p);
            }
            if ui.button(tr!("drawings-ui-swap")).clicked() {
                std::mem::swap(&mut self.base, &mut self.new);
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.clouds, tr!("drawings-ui-clouds"));
            ui.label(tr!("compare-ui-unchanged"));
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Show,
                tr!("compare-ui-unchanged-show"),
            );
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Dim,
                tr!("compare-ui-unchanged-dim"),
            );
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Hide,
                tr!("compare-ui-unchanged-hide"),
            );
        });
        ui.horizontal(|ui| {
            ui.label(tr!("compare-ui-palette"));
            ui.radio_value(
                &mut self.style.palette,
                Palette::Standard,
                tr!("compare-ui-palette-standard"),
            );
            ui.radio_value(
                &mut self.style.palette,
                Palette::ColorBlind,
                tr!("compare-ui-palette-blind"),
            );
        });
        if let Some(e) = &self.error {
            ui.colored_label(
                Color32::from_rgb(200, 60, 40),
                tr!("drawings-ui-failed", message = e.clone()),
            );
            return;
        }
        let Some(shown) = &self.shown else { return };
        ui.horizontal_wrapped(|ui| {
            for (st, n) in [
                (Status::Added, shown.counts[0]),
                (Status::Removed, shown.counts[1]),
                (Status::Changed, shown.counts[2]),
            ] {
                let text = match st {
                    Status::Added => tr!("compare-ui-legend-added", count = n),
                    Status::Removed => tr!("compare-ui-legend-removed", count = n),
                    Status::Changed => tr!("compare-ui-legend-changed", count = n),
                };
                swatch(ui, &self.style, st);
                ui.label(text);
            }
            ui.label(tr!("update-ui-unchanged", count = shown.unchanged));
            if self.clouds {
                ui.label(tr!("drawings-ui-cloud-count", count = shown.clouds));
            }
        });
        for n in &shown.notes {
            ui.small(n);
        }
        // Export.
        let mut export: Option<&'static str> = None;
        ui.horizontal(|ui| {
            for (format, label) in [
                ("png", tr!("drawings-ui-export-png")),
                ("svg", tr!("drawings-ui-export-svg")),
                ("pdf", tr!("drawings-ui-export-pdf")),
            ] {
                if ui.button(label).clicked() {
                    export = Some(format);
                }
            }
        });
        if let Some(format) = export {
            self.export(session, format, status);
        }
        let Some(shown) = &self.shown else { return };
        if let Some(tex) = &shown.texture {
            let width = ui.available_width().clamp(200.0, 1400.0);
            let size = tex.size_vec2();
            let scale = width / size.x.max(1.0);
            ui.add(egui::Image::new((tex.id(), size * scale)));
        }
        if shown.rows.is_empty() {
            ui.label(tr!("compare-ui-no-changes"));
            return;
        }
        for row in &shown.rows {
            ui.horizontal(|ui| {
                swatch(ui, &self.style, row.status);
                ui.label(&row.text);
            });
        }
        if shown.total > shown.rows.len() {
            ui.small(tr!("compare-ui-more"));
        }
    }

    /// Writes the picture in a format to a file that the person chooses.
    fn export(&mut self, session: &mut Session, format: &'static str, status: &mut String) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(format.to_uppercase(), &[format])
            .set_file_name(format!("compare.{format}"))
            .save_file()
        else {
            return;
        };
        self.export_to(session, format, &path, status);
    }

    /// Writes the picture in a format to `path`.
    fn export_to(&mut self, session: &mut Session, format: &str, path: &Path, status: &mut String) {
        let Some(key) = self.key() else { return };
        let params = json!({
            "base": key.base,
            "new": key.new,
            "options": {"limit": 1},
            "render": self.render_options(format, false),
        });
        let result = session.dxf_compare_trusted(params).map_err(|e| e.message);
        let bytes = result.and_then(|r| {
            let image = &r["image"];
            match format {
                "svg" => Ok(image["svg"]
                    .as_str()
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec()),
                f => base64::engine::general_purpose::STANDARD
                    .decode(image[format!("{f}_base64")].as_str().unwrap_or_default())
                    .map_err(|e| e.to_string()),
            }
        });
        *status = match bytes
            .and_then(|b| kadakas_sys::fsio::write_atomic(path, &b).map_err(|e| e.to_string()))
        {
            Ok(()) => tr!("drawings-ui-exported", path = path.display().to_string()),
            Err(e) => tr!("drawings-ui-failed", message = e),
        };
    }
}

/// Reads the answer of `dxf.compare` for the window: the picture as a texture, the counts, and
/// the first rows of the list.
fn shown_of(ctx: &egui::Context, key: Key, r: &Value) -> Shown {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let counts = [
        n(&r["counts"]["added"]),
        n(&r["counts"]["removed"]),
        n(&r["counts"]["changed"]),
    ];
    let texture = r["image"]["svg"]
        .as_str()
        .and_then(raster)
        .map(|img| ctx.load_texture("compare-drawings", img, egui::TextureOptions::LINEAR));
    let rows = r["changes"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|c| {
                    let status = match c["status"].as_str() {
                        Some("added") => Status::Added,
                        Some("removed") => Status::Removed,
                        _ => Status::Changed,
                    };
                    let what = c["what"]
                        .as_array()
                        .map(|w| {
                            w.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    let mut text = format!(
                        "{} {} {}",
                        status.symbol(),
                        c["entity"].as_str().unwrap_or(""),
                        c["layer"].as_str().unwrap_or("")
                    );
                    if !what.is_empty() {
                        text = format!("{text} ({what})");
                    }
                    Row { status, text }
                })
                .collect()
        })
        .unwrap_or_default();
    Shown {
        key,
        texture,
        counts,
        unchanged: n(&r["counts"]["unchanged"]),
        rows,
        total: r["total_changes"].as_u64().unwrap_or(0) as usize,
        clouds: n(&r["image"]["clouds"]["count"]),
        notes: r["notes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Draws an SVG to pixels (the program draws no text with it: the legend is in the window).
fn raster(svg: &str) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 40_000_000 {
        return None;
    }
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    pm.fill(tiny_skia::Color::WHITE);
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pm.as_mut());
    Some(egui::ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pm.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(ents: &str) -> String {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
        )
    }

    fn line(handle: u32, y: i64) -> String {
        format!("0\nLINE\n5\n{handle:X}\n8\nA\n10\n0\n20\n{y}\n11\n4000\n21\n{y}\n")
    }

    fn frame(ui: &mut DrawingsUi, session: &mut Session, status: &mut String) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), session, status);
        });
    }

    #[test]
    fn the_window_compares_draws_and_exports() {
        let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-app-drawings").unwrap();
        let old = dir.path().join("old.dxf");
        let new = dir.path().join("new.dxf");
        std::fs::write(
            &old,
            file(&[line(0x10, 0), line(0x11, 1000), line(0x12, 2000)].concat()),
        )
        .unwrap();
        std::fs::write(
            &new,
            file(&[line(0x10, 0), line(0x11, 1300), line(0x13, 4000)].concat()),
        )
        .unwrap();
        let mut session = Session::with_sequential_ids();
        let mut ui = DrawingsUi::default();
        let mut status = String::new();
        ui.begin(old.clone(), new.clone());
        frame(&mut ui, &mut session, &mut status);
        let shown = ui.shown.as_ref().expect("a comparison");
        assert_eq!(shown.counts, [1, 1, 1]);
        assert_eq!(shown.rows.len(), 3);
        assert!(shown.texture.is_some());
        assert_eq!(shown.clouds, 0);
        // The open document is not touched.
        assert!(session.editor.doc().objects.is_empty());

        // An option makes the picture again.
        ui.clouds = true;
        frame(&mut ui, &mut session, &mut status);
        assert!(ui.shown.as_ref().unwrap().clouds >= 1);
        ui.style.palette = Palette::ColorBlind;
        ui.set_dark(true);
        frame(&mut ui, &mut session, &mut status);
        assert!(ui.shown.is_some() && ui.error.is_none());

        // Export in the three formats.
        for (format, magic) in [
            ("png", &b"\x89PNG"[..]),
            ("svg", b"<svg"),
            ("pdf", b"%PDF-"),
        ] {
            let out = dir.path().join(format!("out.{format}"));
            ui.export_to(&mut session, format, &out, &mut status);
            let bytes = std::fs::read(&out).unwrap_or_default();
            assert!(
                bytes.starts_with(magic) || String::from_utf8_lossy(&bytes).contains("<svg"),
                "{format}: {status}"
            );
            assert!(status.contains("out."), "{status}");
        }
        // Swap and a bad file.
        std::mem::swap(&mut ui.base, &mut ui.new);
        ui.dirty = true;
        frame(&mut ui, &mut session, &mut status);
        assert_eq!(ui.shown.as_ref().unwrap().counts, [1, 1, 1]);
        let junk = dir.path().join("junk.dxf");
        std::fs::write(&junk, "nothing").unwrap();
        ui.new = Some(junk);
        frame(&mut ui, &mut session, &mut status);
        assert!(ui.error.is_some() && ui.shown.is_none());
        frame(&mut ui, &mut session, &mut status);
        ui.export_to(
            &mut session,
            "png",
            &dir.path().join("bad.png"),
            &mut status,
        );
        assert!(!dir.path().join("bad.png").exists());
        assert!(raster("<svg").is_none());
    }
}
