//! Dialogs for GIS: File > Map Sources (add a WMS, WMTS or WFS source, pick layers, load a geoimage)
//! and File > Change CRS (convert the document, with a preview).
//!
//! The dialogs send the same JSON as an agent would send, through `Session::gis_call_trusted`. The
//! network calls run on worker threads: a dialog only starts a job and looks at the answer in
//! each frame (`Session::gis_poll`), so the UI never waits for a server.

use kadakas_api::Session;
use kadakas_l10n::tr;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Wms,
    Wmts,
    Wfs,
    Image,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Wms, Kind::Wmts, Kind::Wfs, Kind::Image];

    fn label(self) -> String {
        match self {
            Kind::Wms => tr!("gis-dialogs-wms-map"),
            Kind::Wmts => tr!("gis-dialogs-wmts-map"),
            Kind::Wfs => tr!("gis-dialogs-wfs-features"),
            Kind::Image => tr!("gis-dialogs-geoimage-file"),
        }
    }
}

/// A layer or feature type in the picker.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    name: String,
    title: String,
}

pub struct GisDialogs {
    sources_open: bool,
    crs_open: bool,
    kind: Kind,
    url: String,
    entries: Vec<Entry>,
    filter: String,
    selected: Vec<String>,
    crs: String,
    format: String,
    max_px: u32,
    opacity: f64,
    image_path: String,
    caps_job: Option<u64>,
    add_job: Option<u64>,
    message: String,
    allow_text: String,
    allow_loaded: bool,
    target_crs: String,
    preview: Option<Value>,
}

impl Default for GisDialogs {
    fn default() -> Self {
        GisDialogs {
            sources_open: false,
            crs_open: false,
            kind: Kind::Wms,
            url: String::new(),
            entries: Vec::new(),
            filter: String::new(),
            selected: Vec::new(),
            crs: String::new(),
            format: String::new(),
            max_px: 2048,
            opacity: 1.0,
            image_path: String::new(),
            caps_job: None,
            add_job: None,
            message: String::new(),
            allow_text: String::new(),
            allow_loaded: false,
            target_crs: String::new(),
            preview: None,
        }
    }
}

/// The layers of a capabilities answer.
fn entries_of(kind: Kind, v: &Value) -> Vec<Entry> {
    let (list, name_key) = match kind {
        Kind::Wms => (&v["layers"], "name"),
        Kind::Wmts => (&v["layers"], "id"),
        Kind::Wfs => (&v["feature_types"], "name"),
        Kind::Image => return Vec::new(),
    };
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| {
            let name = l[name_key].as_str()?.to_string();
            let title = l["title"].as_str().unwrap_or("").to_string();
            Some(Entry { name, title })
        })
        .collect()
}

/// The parameters of the add call, or the reason it cannot be made.
fn add_params(d: &GisDialogs) -> Result<(&'static str, Value), String> {
    let url = d.url.trim();
    let pick =
        |reason: String| -> Result<String, String> { d.selected.first().cloned().ok_or(reason) };
    let mut p = serde_json::Map::new();
    let text = |p: &mut serde_json::Map<String, Value>, k: &str, v: &str| {
        if !v.trim().is_empty() {
            p.insert(k.into(), json!(v.trim()));
        }
    };
    match d.kind {
        Kind::Wms => {
            if d.selected.is_empty() {
                return Err(tr!("gis-dialogs-pick-at-least-one-layer-first"));
            }
            p.insert("url".into(), json!(url));
            p.insert("layers".into(), json!(d.selected));
            text(&mut p, "crs", &d.crs);
            text(&mut p, "format", &d.format);
            p.insert("max_px".into(), json!(d.max_px));
            p.insert("opacity".into(), json!(d.opacity));
            Ok(("gis.wms.add", Value::Object(p)))
        }
        Kind::Wmts => {
            p.insert("url".into(), json!(url));
            p.insert(
                "layer".into(),
                json!(pick(tr!("gis-dialogs-pick-a-layer-first"))?),
            );
            text(&mut p, "format", &d.format);
            p.insert("max_px".into(), json!(d.max_px));
            p.insert("opacity".into(), json!(d.opacity));
            Ok(("gis.wmts.add", Value::Object(p)))
        }
        Kind::Wfs => {
            p.insert("url".into(), json!(url));
            p.insert(
                "type_name".into(),
                json!(pick(tr!("gis-dialogs-pick-a-feature-type-first"))?),
            );
            text(&mut p, "crs", &d.crs);
            Ok(("gis.wfs.import", Value::Object(p)))
        }
        Kind::Image => {
            if d.image_path.trim().is_empty() {
                return Err(tr!("gis-dialogs-choose-a-picture-file-first"));
            }
            p.insert("path".into(), json!(d.image_path.trim()));
            text(&mut p, "crs", &d.crs);
            p.insert("max_px".into(), json!(d.max_px));
            p.insert("opacity".into(), json!(d.opacity));
            Ok(("gis.geoimage.import", Value::Object(p)))
        }
    }
}

impl GisDialogs {
    /// File > Map Sources...
    pub fn open_sources(&mut self) {
        self.sources_open = true;
    }

    /// File > Change CRS...
    pub fn open_crs(&mut self, session: &Session) {
        self.crs_open = true;
        self.preview = None;
        if self.target_crs.is_empty() {
            self.target_crs = session
                .editor
                .doc()
                .georef
                .as_ref()
                .map(|g| g.crs.clone())
                .unwrap_or_default();
        }
    }

    /// Draws the open dialogs and handles the answers of the jobs. Returns true when the document changed.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        status: &mut String,
    ) -> bool {
        let mut changed = self.poll(session, status);
        if self.sources_open {
            self.sources_window(ctx, session, status, &mut changed);
        }
        if self.crs_open {
            self.crs_window(ctx, session, status, &mut changed);
        }
        // Keep polling while a job runs, without a busy loop.
        if !session.gis_jobs().is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        changed
    }

    fn poll(&mut self, session: &mut Session, status: &mut String) -> bool {
        let mut changed = false;
        for (id, _label, result) in session.gis_poll() {
            if self.caps_job == Some(id) {
                self.caps_job = None;
                match result {
                    Ok(v) => {
                        self.entries = entries_of(self.kind, &v);
                        let title = v["title"]
                            .as_str()
                            .filter(|t| !t.is_empty())
                            .map_or_else(|| tr!("gis-dialogs-server"), str::to_string);
                        self.message = if self.kind == Kind::Wfs {
                            tr!(
                                "gis-dialogs-loaded-feature-types",
                                title = title,
                                count = self.entries.len()
                            )
                        } else {
                            tr!(
                                "gis-dialogs-loaded-layers",
                                title = title,
                                count = self.entries.len()
                            )
                        };
                        self.selected.clear();
                    }
                    Err(e) => self.message = tr!("gis-dialogs-error", e = e),
                }
            } else {
                if self.add_job == Some(id) {
                    self.add_job = None;
                }
                match result {
                    Ok(v) => {
                        changed = true;
                        self.message = done_text(&v);
                        *status = self.message.clone();
                    }
                    Err(e) => self.message = tr!("gis-dialogs-error", e = e),
                }
            }
        }
        changed
    }

    fn sources_window(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        status: &mut String,
        changed: &mut bool,
    ) {
        if !self.allow_loaded {
            self.allow_text = session.gis.urls.entries().join("\n");
            self.allow_loaded = true;
        }
        let mut open = true;
        egui::Window::new(tr!("gis-dialogs-map-sources"))
            .id(egui::Id::new("win-map-sources"))
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for k in Kind::ALL {
                        if ui.selectable_value(&mut self.kind, k, k.label()).clicked() {
                            self.entries.clear();
                            self.selected.clear();
                            self.message.clear();
                        }
                    }
                });
                ui.separator();
                if self.kind == Kind::Image {
                    ui.horizontal(|ui| {
                        ui.label(tr!("gis-dialogs-file"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.image_path).desired_width(260.0),
                        );
                        if ui.button(tr!("gis-dialogs-browse")).clicked()
                            && let Some(p) = rfd::FileDialog::new()
                                .add_filter(
                                    tr!("gis-dialogs-pictures"),
                                    &["png", "jpg", "jpeg", "tif", "tiff"],
                                )
                                .pick_file()
                        {
                            self.image_path = p.display().to_string();
                        }
                    });
                    ui.small(tr!("gis-dialogs-a-png-or-jpeg-needs-a-world"));
                } else {
                    ui.horizontal(|ui| {
                        ui.label(tr!("gis-dialogs-server-address"));
                        ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(300.0));
                        let busy = self.caps_job.is_some();
                        if ui
                            .add_enabled(
                                !busy && !self.url.trim().is_empty(),
                                egui::Button::new(tr!("gis-dialogs-load-layers")),
                            )
                            .clicked()
                        {
                            let method = match self.kind {
                                Kind::Wms => "gis.wms.capabilities",
                                Kind::Wmts => "gis.wmts.capabilities",
                                _ => "gis.wfs.capabilities",
                            };
                            match session.gis_call_trusted(method, json!({"url": self.url.trim()}))
                            {
                                Ok(v) => {
                                    self.caps_job = v["job"].as_u64();
                                    self.message = tr!("gis-dialogs-loading");
                                }
                                Err(e) => {
                                    self.message = tr!("gis-dialogs-error-2", message = e.message)
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr!("gis-dialogs-filter"));
                        ui.text_edit_singleline(&mut self.filter);
                    });
                    let f = self.filter.to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for e in self.entries.iter().filter(|e| {
                                f.is_empty()
                                    || e.name.to_lowercase().contains(&f)
                                    || e.title.to_lowercase().contains(&f)
                            }) {
                                let on = self.selected.contains(&e.name);
                                let label = if e.title.is_empty() || e.title == e.name {
                                    e.name.clone()
                                } else {
                                    format!("{}  ({})", e.title, e.name)
                                };
                                if ui.selectable_label(on, label).clicked() {
                                    if self.kind == Kind::Wms {
                                        // A WMS can draw several layers together.
                                        if on {
                                            self.selected.retain(|n| n != &e.name);
                                        } else {
                                            self.selected.push(e.name.clone());
                                        }
                                    } else {
                                        self.selected = vec![e.name.clone()];
                                    }
                                }
                            }
                        });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tr!("gis-dialogs-crs"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.crs)
                            .desired_width(110.0)
                            .hint_text(tr!("gis-dialogs-automatic")),
                    );
                    if matches!(self.kind, Kind::Wms | Kind::Wmts) {
                        ui.label(tr!("gis-dialogs-format"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.format)
                                .desired_width(90.0)
                                .hint_text("image/png"),
                        );
                    }
                });
                if self.kind != Kind::Wfs {
                    ui.horizontal(|ui| {
                        ui.label(tr!("gis-dialogs-pixels"));
                        ui.add(
                            egui::DragValue::new(&mut self.max_px)
                                .range(64..=4096)
                                .speed(16),
                        );
                        ui.label(tr!("gis-dialogs-opacity"));
                        ui.add(egui::Slider::new(&mut self.opacity, 0.0..=1.0));
                    });
                    ui.small(tr!("gis-dialogs-the-map-covers-the-drawing-plus-a"));
                } else {
                    ui.small(tr!(
                        "gis-dialogs-the-features-become-vector-objects-their-attributes"
                    ));
                }
                ui.horizontal(|ui| {
                    let busy = self.add_job.is_some();
                    if ui
                        .add_enabled(!busy, egui::Button::new(tr!("gis-dialogs-add-to-drawing")))
                        .clicked()
                    {
                        match add_params(self) {
                            Ok((method, p)) => match session.gis_call_trusted(method, p) {
                                Ok(v) if v["job"].is_u64() => {
                                    self.add_job = v["job"].as_u64();
                                    self.message = tr!("gis-dialogs-fetching");
                                }
                                Ok(v) => {
                                    *changed = true;
                                    self.message = done_text(&v);
                                    *status = self.message.clone();
                                }
                                Err(e) => {
                                    self.message = tr!("gis-dialogs-error-2", message = e.message)
                                }
                            },
                            Err(e) => self.message = e,
                        }
                    }
                    if busy || self.caps_job.is_some() {
                        ui.spinner();
                    }
                });
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
                ui.separator();
                ui.collapsing(tr!("gis-dialogs-agents-and-the-cache"), |ui| {
                    ui.label(tr!("gis-dialogs-addresses-that-agents-may-fetch-one-per"));
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut self.allow_text)
                                .desired_rows(3)
                                .desired_width(420.0),
                        )
                        .changed()
                    {
                        session.gis.urls = kadakas_api::UrlPolicy::allow_list(
                            self.allow_text.lines().map(str::to_string),
                        );
                    }
                    if let Ok(c) = session.gis_call_trusted("gis.sources", json!({})) {
                        ui.horizontal(|ui| {
                            ui.label(tr!(
                                "gis-dialogs-tile-cache-files-mb",
                                files = c["cache"]["files"].as_u64().unwrap_or(0),
                                megabytes = kadakas_units::current().number_text(
                                    c["cache"]["bytes"].as_u64().unwrap_or(0) as f64 / 1.0e6,
                                    1
                                )
                            ));
                            if ui.button(tr!("gis-dialogs-clear")).clicked() {
                                let _ = session.gis_call_trusted("gis.cache.clear", json!({}));
                            }
                        });
                    }
                });
            });
        self.sources_open = open;
    }

    fn crs_window(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        status: &mut String,
        changed: &mut bool,
    ) {
        let mut open = true;
        egui::Window::new(tr!("gis-dialogs-change-crs"))
            .id(egui::Id::new("win-change-crs"))
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                let current = session
                    .editor
                    .doc()
                    .georef
                    .as_ref()
                    .map(|g| g.crs.clone())
                    .unwrap_or_default();
                if current.is_empty() {
                    ui.label(tr!("gis-dialogs-the-document-has-no-crs-yet-give"));
                } else {
                    ui.label(tr!("gis-dialogs-current-crs", current = current));
                }
                ui.horizontal(|ui| {
                    ui.label(tr!("gis-dialogs-new-crs"));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.target_crs)
                                .desired_width(160.0)
                                .hint_text("EPSG:32635"),
                        )
                        .changed()
                    {
                        self.preview = None;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(tr!("gis-dialogs-preview")).clicked() {
                        let r = if current.is_empty() {
                            Err(tr!("gis-dialogs-set-the-crs-below-to-begin-no"))
                        } else {
                            session
                                .gis_call_trusted(
                                    "gis.reproject",
                                    json!({"crs": self.target_crs.trim(), "dry_run": true}),
                                )
                                .map_err(|e| e.message)
                        };
                        match r {
                            Ok(v) => self.preview = Some(v["report"].clone()),
                            Err(e) => {
                                self.preview = None;
                                self.crs_message(e);
                            }
                        }
                    }
                    let label = if current.is_empty() {
                        tr!("gis-dialogs-set-crs")
                    } else {
                        tr!("gis-dialogs-convert")
                    };
                    if ui
                        .add_enabled(!self.target_crs.trim().is_empty(), egui::Button::new(label))
                        .clicked()
                    {
                        let r = if current.is_empty() {
                            session.gis_call_trusted(
                                "gis.georef.set",
                                json!({"crs": self.target_crs.trim()}),
                            )
                        } else {
                            session.gis_call_trusted(
                                "gis.reproject",
                                json!({"crs": self.target_crs.trim()}),
                            )
                        };
                        match r {
                            Ok(_) => {
                                *changed = true;
                                *status =
                                    tr!("gis-dialogs-crs-is-now", crs = self.target_crs.trim());
                                self.preview = None;
                            }
                            Err(e) => self.crs_message(e.message),
                        }
                    }
                });
                if let Some(r) = &self.preview {
                    ui.separator();
                    let u = kadakas_units::current();
                    ui.label(tr!(
                        "gis-dialogs-objects-change",
                        count = r["objects_changed"].as_u64().unwrap_or(0),
                        factor = u.number_text(r["scale_factor"].as_f64().unwrap_or(1.0), 6),
                        new = u.number_text(r["new_rotation_deg"].as_f64().unwrap_or(0.0), 4),
                        old = u.number_text(r["old_rotation_deg"].as_f64().unwrap_or(0.0), 4)
                    ));
                    ui.label(tr!(
                        "gis-dialogs-new-origin",
                        east = u.number_text(r["new_origin"][0].as_f64().unwrap_or(0.0), 0),
                        north = u.number_text(r["new_origin"][1].as_f64().unwrap_or(0.0), 0),
                        shift = u.length_text_d(r["max_shift_mm"].as_f64().unwrap_or(0.0), 1)
                    ));
                    for w in r["warnings"].as_array().into_iter().flatten() {
                        ui.small(format!("- {}", w.as_str().unwrap_or("")));
                    }
                    for s in r["skipped"].as_array().into_iter().flatten().take(8) {
                        ui.small(tr!(
                            "gis-dialogs-not-converted-object",
                            id = s["id"]
                                .as_str()
                                .map_or_else(|| s["id"].to_string(), str::to_string),
                            kind = s["kind"].as_str().unwrap_or(""),
                            reason = s["reason"].as_str().unwrap_or("")
                        ));
                    }
                }
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
            });
        self.crs_open = open;
    }

    fn crs_message(&mut self, m: String) {
        self.message = tr!("gis-dialogs-error-3", m = m);
    }
}

/// The text for a finished call.
fn done_text(v: &Value) -> String {
    if let Some(s) = v.get("stats") {
        tr!(
            "gis-dialogs-map-added-tiles-from-the-cache-crs",
            tiles = s["tiles"].as_u64().unwrap_or(0),
            cached = s["from_cache"].as_u64().unwrap_or(0),
            crs = s["crs"].as_str().unwrap_or("")
        )
    } else if let Some(n) = v.get("imported") {
        tr!(
            "gis-dialogs-objects-imported",
            count = n.as_u64().unwrap_or(0)
        )
    } else if v.get("width").is_some() {
        tr!(
            "gis-dialogs-picture-added-x-px",
            width = v["width"].as_u64().unwrap_or(0),
            height = v["height"].as_u64().unwrap_or(0)
        )
    } else {
        tr!("gis-dialogs-done")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_lists_come_from_the_capabilities_answers() {
        let wms = json!({"layers": [{"name": null, "title": "Group"}, {"name": "ortho", "title": "Orthophoto"}]});
        assert_eq!(
            entries_of(Kind::Wms, &wms),
            vec![Entry {
                name: "ortho".into(),
                title: "Orthophoto".into()
            }]
        );
        let wmts = json!({"layers": [{"id": "base", "title": "Base"}]});
        assert_eq!(entries_of(Kind::Wmts, &wmts)[0].name, "base");
        let wfs = json!({"feature_types": [{"name": "ns:b", "title": "Buildings"}]});
        assert_eq!(entries_of(Kind::Wfs, &wfs)[0].title, "Buildings");
        assert!(entries_of(Kind::Image, &wfs).is_empty());
        assert!(entries_of(Kind::Wms, &json!({})).is_empty());
    }

    #[test]
    fn the_add_call_follows_the_form() {
        let mut d = GisDialogs {
            url: " http://x/wms ".into(),
            ..Default::default()
        };
        assert!(add_params(&d).unwrap_err().contains("layer"));
        d.selected = vec!["a".into(), "b".into()];
        d.crs = "EPSG:3857".into();
        let (m, p) = add_params(&d).unwrap();
        assert_eq!(m, "gis.wms.add");
        assert_eq!(p["layers"], json!(["a", "b"]));
        assert_eq!(
            (p["url"].as_str(), p["crs"].as_str(), p["max_px"].as_u64()),
            (Some("http://x/wms"), Some("EPSG:3857"), Some(2048))
        );
        assert!(p.get("format").is_none());
        d.kind = Kind::Wmts;
        let (m, p) = add_params(&d).unwrap();
        assert_eq!((m, p["layer"].as_str()), ("gis.wmts.add", Some("a")));
        d.kind = Kind::Wfs;
        let (m, p) = add_params(&d).unwrap();
        assert_eq!((m, p["type_name"].as_str()), ("gis.wfs.import", Some("a")));
        d.selected.clear();
        assert!(add_params(&d).unwrap_err().contains("feature type"));
        d.kind = Kind::Image;
        assert!(add_params(&d).unwrap_err().contains("picture"));
        d.image_path = "/a/b.png".into();
        assert_eq!(add_params(&d).unwrap().0, "gis.geoimage.import");
        for k in Kind::ALL {
            assert!(!k.label().is_empty());
        }
    }

    #[test]
    fn finished_calls_have_text() {
        assert!(
            done_text(&json!({"stats": {"tiles": 4, "from_cache": 1, "crs": "EPSG:3857"}}))
                .contains("4 tiles")
        );
        assert_eq!(done_text(&json!({"imported": 12})), "12 objects imported");
        assert!(done_text(&json!({"width": 10, "height": 5})).contains("10 x 5"));
        assert_eq!(done_text(&json!({})), "Done");
    }
}
