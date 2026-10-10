//! Agent API for GIS services: map backgrounds (WMS, WMTS), WFS import, geoimages, coordinate
//! conversion and the coordinate readout. Native only.
//!
//! Methods: `gis.sources`, `gis.cache.clear`, `gis.georef.set`, `gis.coords`, `gis.reproject`,
//! `gis.wms.capabilities|add|refresh`, `gis.wmts.capabilities|add`, `gis.wfs.capabilities|import`,
//! `gis.geoimage.import`, `gis.job.status|cancel`.
//!
//! # Network policy
//!
//! An agent may only fetch addresses on a list that the owner of the app sets with `--allow-url`
//! (the default list is empty: nothing is allowed). The user's own dialogs in the app are trusted.
//! Network calls run on a worker thread. In `cadektor serve` they wait for the answer by default. In
//! the app they return `{job, status: "running"}` at once, and `gis.job.status` gives the result.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cadektor_core::DVec2;
use cadektor_core::command::Command;
use cadektor_core::georef::Georeference;
use cadektor_core::model::{Attributes, ClassId, Document, LayerId, ObjectId, Shape};
use cadektor_crs::Crs;
use cadektor_gis::GisError;
use cadektor_gis::cache::{DEFAULT_LIMIT, TileCache};
use cadektor_gis::http::{HttpClient, UrlPolicy};
use cadektor_gis::job::Job;
use cadektor_gis::mosaic::{MapOutput, ViewRequest};
use cadektor_gis::wfs::{self, WfsSource};
use cadektor_gis::wms::{self, WmsSource};
use cadektor_gis::wmts::{self, WmtsSource};
use cadektor_io::ImportOptions;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::errors::{app, bad};
use crate::{RpcError, Session};

pub const GIS_SOURCES: (&str, &str) = (
    "gis.sources",
    "The network policy and the tile cache. Returns {allowed_urls: [..], restricted, cache: {dir, limit_bytes, files, bytes}, blocking}. Agents may fetch only addresses on allowed_urls (the owner sets them with --allow-url; the default is none).",
);
pub const GIS_CACHE_CLEAR: (&str, &str) = (
    "gis.cache.clear",
    "Delete the cached map tiles. Returns {removed}.",
);
pub const GIS_GEOREF_SET: (&str, &str) = (
    "gis.georef.set",
    "Set the georeference of the document: where the model origin lies on the earth. params: {crs ('EPSG:3067' or WKT/PROJ text; a projected CRS in meters), origin? ([easting, northing] in meters, default [0, 0]), rotation? (radians, default 0)}. Returns {georeference}. Use gis.reproject to change the CRS of a document that has objects.",
);
pub const GIS_COORDS: (&str, &str) = (
    "gis.coords",
    "The map position of a model position (mm): the coordinate readout of the Data bar. params: {x, y}. Returns {crs, easting, northing, lon, lat} in meters and degrees. Fails if the document has no CRS.",
);
pub const GIS_REPROJECT: (&str, &str) = (
    "gis.reproject",
    "Change the CRS of the document. All objects and the georeference are converted with proj4rs in one undo step. params: {crs ('EPSG:32635'), dry_run? (true: only the report), origin? ([easting, northing] in the new CRS for the model origin; default: the old origin converted and rounded to a meter)}. Returns {applied, report: {from_crs, to_crs, old_origin, new_origin, old_rotation_deg, new_rotation_deg, scale_factor, objects_changed, changed: {kind: n}, max_shift_mm, skipped: [{id, kind, reason}], warnings}}. Heights (Z) do not change. Angles in the drawing do not change; lengths change by scale_factor. Annotation objects keep their positions (see warnings).",
);
pub const GIS_WMS_CAPABILITIES: (&str, &str) = (
    "gis.wms.capabilities",
    "Read the GetCapabilities of a WMS (1.3.0 or 1.1.1). params: {url, wait?}. The address must be on the allow list. Returns {title, version, formats, getmap_url, layers: [{name, title, abstract_text, crs, geographic_bbox, bboxes, styles, depth}]}; a layer without a name is a group.",
);
pub const GIS_WMS_ADD: (&str, &str) = (
    "gis.wms.add",
    "Add a WMS map as an image underlay (drawn below the vector objects, in sheets and PDF too). The view is cut into tiles (cached on disk), requested in the document CRS, or in EPSG:3857 / EPSG:4326 when the server has no better, and warped into the model frame. params: {url, layers: [names], styles?: [names], crs?, format?, transparent? (default true), version?, bbox? ([min x, min y, max x, max y] in model mm; default: the drawing plus 5 percent), max_px? (longest side of the picture, default 2048, at most 4096), name?, opacity? (0 to 1), layer?, class?, wait?}. The document needs a CRS (gis.georef.set). Returns {id, stats: {tiles, from_cache, bytes, crs, reprojected, service}, width, height} or {job, status} when the call does not wait.",
);
pub const GIS_WMS_REFRESH: (&str, &str) = (
    "gis.wms.refresh",
    "Fetch an underlay again for a new view (WMS and WMTS). params: {id, bbox?, max_px?, wait?}. The source is kept in the data of the object (key gis_source). Returns like gis.wms.add.",
);
pub const GIS_WMTS_CAPABILITIES: (&str, &str) = (
    "gis.wmts.capabilities",
    "Read the GetCapabilities of a WMTS. params: {url, wait?}. Returns {title, kvp_url, layers: [{id, title, formats, styles, default_style, matrix_sets, resource_urls, wgs84_bbox}], matrix_sets: [{id, crs, levels}]}.",
);
pub const GIS_WMTS_ADD: (&str, &str) = (
    "gis.wmts.add",
    "Add a WMTS map as an image underlay. params: {url, layer, matrix_set?, style?, format?, bbox?, max_px?, name?, opacity?, layer_name?, class?, wait?} (bbox, max_px, name, opacity, class as in gis.wms.add; the design layer is 'layer_name' or 'design_layer' because 'layer' is the WMTS layer). Returns like gis.wms.add.",
);
pub const GIS_WFS_CAPABILITIES: (&str, &str) = (
    "gis.wfs.capabilities",
    "Read the GetCapabilities of a WFS 2.0. params: {url, wait?}. Returns {title, version, getfeature_url, output_formats, feature_types: [{name, title, default_crs, other_crs, wgs84_bbox}]}.",
);
pub const GIS_WFS_IMPORT: (&str, &str) = (
    "gis.wfs.import",
    "Import the features of a WFS 2.0 feature type (GetFeature as GeoJSON or GML 3.2 simple features) as vector objects. They go through the importer of GeoJSON and Shapefile files, so the georeference, the layers, classes and the object data (the properties of the features) work the same way. params: {url, type_name, crs?, bbox? ([min x, min y, max x, max y] in model mm), max_features? (default 5000), format? ('geojson' or 'gml'), options? (as in import.file: layer, class_field, origin, ...), caps? (false: skip GetCapabilities), wait?}. The server is asked page by page (1000 features each) up to max_features (default 5000, at most 50000); truncated is true when the limit was reached and the server may have more. Returns like import.file: {imported, skipped, truncated, report, georeference} or {job, status}.",
);
pub const GIS_GEOIMAGE_IMPORT: (&str, &str) = (
    "gis.geoimage.import",
    "Import a georeferenced picture as an image underlay: a PNG or JPEG with a world file next to it (.pgw, .jgw, .wld), or a GeoTIFF (8 or 16 bit gray, RGB, RGBA). A .prj file next to the picture gives the CRS. params: {path, crs? (when the picture has none), max_px? (default 2048), name?, opacity?, layer?, class?}. A document without a georeference gets one from the picture. Returns {id, width, height, georeference?, notes}. The path must be in an allowed folder.",
);
pub const GIS_JOB_STATUS: (&str, &str) = (
    "gis.job.status",
    "State of a network call that did not wait. params: {job, wait? (seconds to wait, default 0)}. Returns {status: running | done | failed, result?, error?}. The result is applied to the document when this call sees it done.",
);
pub const GIS_JOB_CANCEL: (&str, &str) = (
    "gis.job.cancel",
    "Stop waiting for a network call. params: {job}. Returns {cancelled}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "network_policy": "Agents may fetch only addresses on the allow list that the owner sets (cadektor serve --allow-url HOST_OR_PREFIX, repeatable; the app has the same flag and a field in the map source dialog). The default list is empty, so every gis network call fails with a message that names the flag. Only http and https; addresses with a user name or password are refused; redirects are checked against the list again; answers are limited in size and time. File access (gis.geoimage.import) follows the folder list (--allow-dir).",
        "underlay": "An underlay (shape kind 'underlay') is a georeferenced raster on a layer: min, max (model mm), width, height, png (base64), opacity. It is drawn below the vector objects in the canvas, in SVG renders and in sheets and PDF (as an embedded image, at most 2048 pixels on a side there). It does not appear in 3D views. IFC export skips it.",
        "examples": [
            {"method": "gis.georef.set", "params": {"crs": "EPSG:3067", "origin": [385000, 6672000]}},
            {"method": "gis.wms.capabilities", "params": {"url": "https://maps.example.org/wms"}},
            {"method": "gis.wms.add", "params": {"url": "https://maps.example.org/wms", "layers": ["orthophoto"], "bbox": [-20000, -20000, 120000, 80000], "max_px": 2048, "name": "Aerial"}},
            {"method": "gis.wmts.add", "params": {"url": "https://tiles.example.org/wmts", "layer": "background", "opacity": 0.7}},
            {"method": "gis.wfs.import", "params": {"url": "https://features.example.org/wfs", "type_name": "ns:buildings", "bbox": [-20000, -20000, 120000, 80000], "options": {"class_field": "type"}}},
            {"method": "gis.geoimage.import", "params": {"path": "/data/plan.png", "crs": "EPSG:3067"}},
            {"method": "gis.reproject", "params": {"crs": "EPSG:32635", "dry_run": true}},
            {"method": "gis.coords", "params": {"x": 10000, "y": 5000}}
        ]
    })
}

/// State of the GIS calls of a session.
pub struct GisState {
    /// Which addresses agents may fetch. Default: none.
    pub urls: UrlPolicy,
    /// Folder of the tile cache. Default: the user cache folder.
    pub cache_dir: Option<PathBuf>,
    pub cache_limit: u64,
    /// Time limit of one request.
    pub timeout: Duration,
    /// True (default): a network call waits for its answer. The app sets false so the UI never waits.
    pub blocking: bool,
    next_job: u64,
    jobs: Vec<Entry>,
    /// Counts the documents of the session. A job result is applied only to the document it was
    /// started for.
    pub(crate) epoch: u64,
}

impl Default for GisState {
    fn default() -> Self {
        GisState {
            urls: UrlPolicy::deny_all(),
            cache_dir: None,
            cache_limit: DEFAULT_LIMIT,
            timeout: Duration::from_secs(20),
            blocking: true,
            next_job: 1,
            jobs: Vec::new(),
            epoch: 0,
        }
    }
}

/// What a job brings back.
enum Outcome {
    Json(Value),
    Map(Box<MapJob>),
    Features {
        fetched: wfs::Fetched,
        options: ImportOptions,
    },
}

struct MapJob {
    out: MapOutput,
    source: Value,
    name: String,
    layer: Option<LayerId>,
    class: Option<ClassId>,
    opacity: f64,
    replace: Option<ObjectId>,
}

struct Entry {
    id: u64,
    kind: &'static str,
    job: Job<Outcome>,
    /// Applied by `gis_poll` (jobs of the user's dialogs), not by the caller.
    auto: bool,
    done: Option<Result<Value, String>>,
    reported: bool,
    /// The document (see `GisState::epoch`) and its georeference when the job started.
    epoch: u64,
    georef: Option<Georeference>,
    /// Set to stop the worker at its next request.
    cancel: Arc<AtomicBool>,
}

/// A job as the UI shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct JobInfo {
    pub id: u64,
    pub label: String,
    pub running: bool,
}

fn gis_err(e: GisError) -> RpcError {
    match e {
        GisError::Param(_) => bad(e),
        GisError::Url(_) => RpcError::policy(e),
        GisError::Limit(_) => RpcError::limit(e),
        GisError::Format(_) | GisError::Crs(_) => RpcError::validation(e),
        e => app(e),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(m: &Map<String, Value>) -> Result<T, RpcError> {
    serde_json::from_value(Value::Object(m.clone())).map_err(bad)
}

pub fn is_gis_method(method: &str) -> bool {
    method.starts_with("gis.")
}

/// Default folder of the tile cache.
pub fn default_cache_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CADEKTOR_GIS_CACHE").filter(|v| !v.is_empty()) {
        return PathBuf::from(d);
    }
    cadektor_sys::dirs::cache_dir().join("tiles")
}

impl Session {
    fn tile_cache(&self) -> TileCache {
        TileCache::new(
            self.gis.cache_dir.clone().unwrap_or_else(default_cache_dir),
            self.gis.cache_limit,
        )
    }

    /// The jobs for the UI.
    pub fn gis_jobs(&self) -> Vec<JobInfo> {
        self.gis
            .jobs
            .iter()
            .filter(|e| e.auto && !e.reported)
            .map(|e| JobInfo {
                id: e.id,
                label: e.kind.to_string(),
                running: e.done.is_none(),
            })
            .collect()
    }

    /// Applies the results of the jobs that the user's dialogs started. Call it once per frame; it
    /// never blocks. Returns (job, label, result) for each job that ended.
    pub fn gis_poll(&mut self) -> Vec<(u64, String, Result<Value, String>)> {
        let ids: Vec<u64> = self
            .gis
            .jobs
            .iter()
            .filter(|e| e.auto && !e.reported)
            .map(|e| e.id)
            .collect();
        let mut out = Vec::new();
        for id in ids {
            self.settle(id);
            if let Some(e) = self.gis.jobs.iter_mut().find(|e| e.id == id)
                && let Some(r) = e.done.clone()
            {
                e.reported = true;
                out.push((id, e.kind.to_string(), r));
            }
        }
        self.gis.jobs.retain(|e| !(e.reported && e.auto));
        out
    }

    /// Takes the result of a finished job and applies it.
    fn settle(&mut self, id: u64) {
        let Some(pos) = self.gis.jobs.iter().position(|e| e.id == id) else {
            return;
        };
        if self.gis.jobs[pos].done.is_some() || !self.gis.jobs[pos].job.is_done() {
            return;
        }
        let r = self.gis.jobs[pos].job.take();
        let result = match r {
            Some(Ok(outcome)) => match self.stale(pos, &outcome) {
                Some(why) => Err(why.to_string()),
                None => self.apply(outcome).map_err(|e| e.message),
            },
            Some(Err(e)) => Err(e.to_string()),
            None => return,
        };
        self.gis.jobs[pos].done = Some(result);
    }

    /// Why the result of a job must not be applied, if so: it was made for another document or
    /// for another georeference than the one the session has now.
    fn stale(&self, pos: usize, outcome: &Outcome) -> Option<&'static str> {
        if matches!(outcome, Outcome::Json(_)) {
            return None;
        }
        let e = &self.gis.jobs[pos];
        if e.epoch != self.gis.epoch {
            Some("another document was opened while the call ran, so its result was dropped")
        } else if e.georef != self.editor.doc().georef {
            Some(
                "the georeference changed while the call ran, so its result was dropped. Run the call again",
            )
        } else {
            None
        }
    }

    /// Starts a network job. `trusted` calls come from the user's dialogs.
    fn submit<F>(
        &mut self,
        kind: &'static str,
        trusted: bool,
        wait: Option<bool>,
        f: F,
    ) -> Result<Value, RpcError>
    where
        F: FnOnce(&HttpClient, &TileCache) -> Result<Outcome, GisError> + Send + 'static,
    {
        let policy = if trusted {
            UrlPolicy::unrestricted()
        } else {
            self.gis.urls.clone()
        };
        let timeout = self.gis.timeout;
        let cache = self.tile_cache();
        let cancel = Arc::new(AtomicBool::new(false));
        let http = HttpClient::new(policy, timeout).with_cancel(cancel.clone());
        let job = Job::spawn(kind, move || f(&http, &cache));
        let id = self.gis.next_job;
        self.gis.next_job += 1;
        // Old finished jobs go.
        self.gis.jobs.retain(|e| e.done.is_none() || !e.reported);
        if self.gis.jobs.len() > 32 {
            self.gis.jobs.remove(0);
        }
        self.gis.jobs.push(Entry {
            id,
            kind,
            job,
            auto: trusted,
            done: None,
            reported: false,
            epoch: self.gis.epoch,
            georef: self.editor.doc().georef.clone(),
            cancel,
        });
        let wait = if trusted {
            false
        } else {
            wait.unwrap_or(self.gis.blocking)
        };
        if wait {
            self.wait_for(id, timeout * 3 + Duration::from_secs(10))
        } else {
            Ok(json!({"job": id, "status": "running"}))
        }
    }

    fn wait_for(&mut self, id: u64, limit: Duration) -> Result<Value, RpcError> {
        let end = std::time::Instant::now() + limit;
        loop {
            self.settle(id);
            if let Some(e) = self.gis.jobs.iter().find(|e| e.id == id)
                && let Some(r) = &e.done
            {
                return match r.clone() {
                    Ok(v) => Ok(v),
                    Err(msg) => Err(app(msg)),
                };
            }
            if std::time::Instant::now() >= end {
                return Ok(json!({"job": id, "status": "running"}));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Applies what a job brought back and returns the answer for the caller.
    fn apply(&mut self, outcome: Outcome) -> Result<Value, RpcError> {
        match outcome {
            Outcome::Json(v) => Ok(v),
            Outcome::Features { fetched, options } => {
                let ctx = cadektor_io::Context::of(self.editor.doc());
                let result = wfs::import(&fetched, &ctx, &options).map_err(gis_err)?;
                self.editor
                    .execute(&result.command)
                    .map_err(RpcError::from)?;
                self.prune_selection();
                Ok(json!({
                    "imported": result.report.total(),
                    "skipped": result.report.skipped_total(),
                    "truncated": fetched.truncated,
                    "report": result.report,
                    "georeference": self.editor.doc().georef,
                }))
            }
            Outcome::Map(job) => {
                let MapJob {
                    out,
                    source,
                    name,
                    layer,
                    class,
                    opacity,
                    replace,
                } = *job;
                let mut u = out.underlay;
                if !name.is_empty() {
                    u.name = name;
                }
                u.opacity = opacity;
                let (w, h) = (u.width, u.height);
                let shape = Shape::Underlay(Box::new(u));
                let data = std::collections::BTreeMap::from([("gis_source".to_string(), source)]);
                let id = match replace {
                    Some(id) => {
                        self.editor
                            .execute(&Command::Batch {
                                commands: vec![
                                    Command::ModifyObject {
                                        id,
                                        shape: Some(shape),
                                        layer: None,
                                        class: None,
                                        attrs: None,
                                    },
                                    Command::SetData {
                                        id,
                                        set: data,
                                        remove: Vec::new(),
                                    },
                                ],
                            })
                            .map_err(RpcError::from)?;
                        id
                    }
                    None => {
                        let next = ObjectId(self.editor.doc().next_id());
                        self.editor
                            .execute(&Command::Batch {
                                commands: vec![
                                    Command::AddObject {
                                        shape,
                                        layer,
                                        class,
                                        attrs: Attributes::default(),
                                    },
                                    Command::SetData {
                                        id: next,
                                        set: data,
                                        remove: Vec::new(),
                                    },
                                ],
                            })
                            .map_err(RpcError::from)?;
                        next
                    }
                };
                self.prune_selection();
                Ok(json!({"id": id, "stats": out.stats, "width": w, "height": h}))
            }
        }
    }

    /// A call that comes from the user's own dialog: the allow list for agents does not apply, and
    /// the call never waits (see [`Session::gis_poll`]).
    pub fn gis_call_trusted(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        self.gis_call(method, p, true)
    }

    pub(crate) fn gis_call(
        &mut self,
        method: &str,
        p: Value,
        trusted: bool,
    ) -> Result<Value, RpcError> {
        let p = if p.is_null() { json!({}) } else { p };
        let m = p
            .as_object()
            .ok_or_else(|| bad("params must be an object"))?;
        match method {
            "gis.sources" => {
                let c = self.tile_cache();
                let (files, bytes) = c.stats();
                Ok(json!({
                    "allowed_urls": self.gis.urls.entries(),
                    "restricted": self.gis.urls.is_restricted(),
                    "cache": {"dir": c.dir(), "limit_bytes": c.limit(), "files": files, "bytes": bytes},
                    "blocking": self.gis.blocking,
                }))
            }
            "gis.cache.clear" => Ok(json!({"removed": self.tile_cache().clear()})),
            "gis.georef.set" => self.georef_set(m),
            "gis.coords" => self.coords(m),
            "gis.reproject" => self.reproject(m),
            "gis.wms.capabilities" | "gis.wmts.capabilities" | "gis.wfs.capabilities" => {
                self.capabilities(method, m, trusted)
            }
            "gis.wms.add" => self.wms_add(m, trusted),
            "gis.wms.refresh" => self.refresh(m, trusted),
            "gis.wmts.add" => self.wmts_add(m, trusted),
            "gis.wfs.import" => self.wfs_import(m, trusted),
            "gis.geoimage.import" => self.geoimage_import(m, trusted),
            "gis.job.status" => self.job_status(m),
            "gis.job.cancel" => {
                let id = job_id(m)?;
                match self.gis.jobs.iter_mut().find(|e| e.id == id) {
                    Some(e) => {
                        e.cancel.store(true, Ordering::SeqCst);
                        e.job.cancel();
                        e.done = Some(Err("cancelled".into()));
                        Ok(json!({"cancelled": true}))
                    }
                    None => Err(bad(format!("no job {id}"))),
                }
            }
            other => Err(RpcError::method_not_found(other)),
        }
    }

    fn job_status(&mut self, m: &Map<String, Value>) -> Result<Value, RpcError> {
        let id = job_id(m)?;
        if !self.gis.jobs.iter().any(|e| e.id == id) {
            return Err(bad(format!("no job {id} (it may be too old)")));
        }
        let secs = m
            .get("wait")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .clamp(0.0, 120.0);
        let end = std::time::Instant::now() + Duration::from_secs_f64(secs);
        loop {
            self.settle(id);
            let e = self.gis.jobs.iter().find(|e| e.id == id).expect("exists");
            match &e.done {
                Some(Ok(v)) => return Ok(json!({"status": "done", "result": v})),
                Some(Err(msg)) => return Ok(json!({"status": "failed", "error": msg})),
                None if std::time::Instant::now() >= end => {
                    return Ok(json!({"status": "running"}));
                }
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    // ------------------------------------------------------------ document

    fn georef_set(&mut self, m: &Map<String, Value>) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct P {
            crs: String,
            origin: Option<[f64; 2]>,
            #[serde(default)]
            rotation: f64,
        }
        let P {
            crs,
            origin,
            rotation,
        } = parse(m)?;
        let c = Crs::parse(&crs).map_err(bad)?;
        if c.is_geographic() {
            return Err(bad("the document CRS must be a projected CRS (meters)"));
        }
        let origin = origin.map_or(DVec2::ZERO, |o| DVec2::new(o[0], o[1]));
        let g = Georeference {
            crs: c.spec.clone(),
            origin,
            rotation,
            user_origin: self
                .editor
                .doc()
                .georef
                .as_ref()
                .map_or(DVec2::ZERO, |g| g.user_origin),
        };
        if !g.is_finite() {
            return Err(bad("origin and rotation must be finite numbers"));
        }
        self.editor
            .execute(&Command::SetGeoreference {
                georef: Some(g.clone()),
            })
            .map_err(RpcError::from)?;
        Ok(json!({"georeference": g}))
    }

    fn coords(&mut self, m: &Map<String, Value>) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct P {
            x: f64,
            y: f64,
        }
        let P { x, y } = parse(m)?;
        let g = self
            .editor
            .doc()
            .georef
            .as_ref()
            .ok_or_else(|| app("the document has no CRS. Set it with gis.georef.set"))?;
        let r = cadektor_gis::coords::Reader::new(g)
            .ok_or_else(|| app("the document has no usable CRS"))?
            .at(DVec2::new(x, y))
            .ok_or_else(|| app("this position cannot be converted"))?;
        Ok(serde_json::to_value(r).expect("json"))
    }

    fn reproject(&mut self, m: &Map<String, Value>) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct P {
            crs: String,
            #[serde(default)]
            dry_run: bool,
            origin: Option<[f64; 2]>,
        }
        let P {
            crs,
            dry_run,
            origin,
        } = parse(m)?;
        let opts = cadektor_gis::reproject::ReprojectOptions {
            origin: origin.map(|o| DVec2::new(o[0], o[1])),
        };
        let plan =
            cadektor_gis::reproject::plan(self.editor.doc(), &crs, &opts).map_err(gis_err)?;
        let mut applied = false;
        if let (false, Some(cmd)) = (dry_run, &plan.command) {
            self.editor.execute(cmd).map_err(RpcError::from)?;
            self.prune_selection();
            applied = true;
        }
        Ok(json!({"applied": applied, "report": plan.report}))
    }

    // ------------------------------------------------------------ services

    fn capabilities(
        &mut self,
        method: &str,
        m: &Map<String, Value>,
        trusted: bool,
    ) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct P {
            url: String,
            wait: Option<bool>,
        }
        let P { url, wait } = parse(m)?;
        self.check_url(&url, trusted)?;
        let kind: &'static str = match method {
            "gis.wms.capabilities" => "WMS capabilities",
            "gis.wmts.capabilities" => "WMTS capabilities",
            _ => "WFS capabilities",
        };
        let method = method.to_string();
        self.submit(kind, trusted, wait, move |http, _| {
            let v = match method.as_str() {
                "gis.wms.capabilities" => json!(wms::fetch_capabilities(http, &url)?),
                "gis.wmts.capabilities" => {
                    let c = wmts::fetch_capabilities(http, &url)?;
                    let sets: Vec<Value> = c
                        .matrix_sets
                        .iter()
                        .map(|s| json!({"id": s.id, "crs": s.crs, "levels": s.matrices.len()}))
                        .collect();
                    json!({"title": c.title, "kvp_url": c.kvp_url, "layers": c.layers, "matrix_sets": sets})
                }
                _ => json!(wfs::fetch_capabilities(http, &url)?),
            };
            Ok(Outcome::Json(v))
        })
    }

    fn check_url(&self, url: &str, trusted: bool) -> Result<(), RpcError> {
        if trusted {
            UrlPolicy::unrestricted().check(url)
        } else {
            self.gis.urls.check(url)
        }
        .map_err(gis_err)
    }

    /// The georeference and CRS of the document, or the message that tells how to set them.
    fn doc_frame(&self) -> Result<Georeference, RpcError> {
        self.editor
            .doc()
            .georef
            .clone()
            .filter(|g| !g.crs.trim().is_empty())
            .ok_or_else(|| {
                app("the document has no CRS. Set it with gis.georef.set (or import a georeferenced file) before you add a map")
            })
    }

    /// The view for a map: the box that the call names, else the drawing plus 5 percent.
    fn view_of(
        &self,
        bbox: Option<[f64; 4]>,
        max_px: Option<u32>,
    ) -> Result<ViewRequest, RpcError> {
        let (lo, hi) = match bbox {
            Some(b) => (DVec2::new(b[0], b[1]), DVec2::new(b[2], b[3])),
            None => {
                let doc = self.editor.doc();
                let mut it = doc
                    .objects
                    .values()
                    .filter(|o| {
                        !matches!(
                            o.shape,
                            Shape::Underlay(_)
                                | Shape::Group { .. }
                                | Shape::Viewport(_)
                                | Shape::TitleBlock(_)
                        )
                    })
                    .filter(|o| {
                        doc.layer(o.layer)
                            .is_some_and(|l| l.kind == cadektor_core::model::LayerKind::Design)
                    })
                    .map(|o| doc.object_bounds(o))
                    .filter(|(a, b)| a.is_finite() && b.is_finite());
                let first = it.next().ok_or_else(|| {
                    bad("the drawing is empty: give bbox ([min x, min y, max x, max y] in model mm)")
                })?;
                let (lo, hi) = it.fold(first, |(lo, hi), (a, b)| (lo.min(a), hi.max(b)));
                let pad = ((hi - lo).max_element() * 0.05).max(1000.0);
                (lo - pad, hi + pad)
            }
        };
        let mut v = ViewRequest::new(lo, hi);
        if let Some(px) = max_px {
            v.target_px = px;
        }
        v.check().map_err(gis_err)?;
        Ok(v)
    }

    fn wms_add(&mut self, m: &Map<String, Value>, trusted: bool) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct Extra {
            bbox: Option<[f64; 4]>,
            max_px: Option<u32>,
            name: Option<String>,
            opacity: Option<f64>,
            layer_name: Option<String>,
            design_layer: Option<String>,
            class: Option<ClassId>,
            wait: Option<bool>,
        }
        let source: WmsSource = parse(m)?;
        let mut rest = m.clone();
        // The design layer: for WMS the key 'layer' is not used by the source, so it names the design layer.
        let layer = rest.remove("layer");
        let ex: Extra = parse(&rest)?;
        self.check_url(&source.url, trusted)?;
        let design = self.design_layer(layer.as_ref(), ex.layer_name.or(ex.design_layer))?;
        let g = self.doc_frame()?;
        let view = self.view_of(ex.bbox, ex.max_px)?;
        let src_json = json!({"kind": "wms", "source": source, "bbox": [view.min.x, view.min.y, view.max.x, view.max.y], "max_px": view.target_px});
        let opacity = check_opacity(ex.opacity)?;
        let name = ex.name.unwrap_or_default();
        let class = ex.class;
        self.submit("WMS map", trusted, ex.wait, move |http, cache| {
            let doc_crs = Crs::parse(&g.crs).map_err(GisError::Crs)?;
            let caps = wms::fetch_capabilities(http, &source.url)?;
            let out = wms::fetch_map(http, Some(cache), &source, &caps, &doc_crs, &g, &view)?;
            Ok(Outcome::Map(Box::new(MapJob {
                out,
                source: src_json,
                name,
                layer: design,
                class,
                opacity,
                replace: None,
            })))
        })
    }

    fn wmts_add(&mut self, m: &Map<String, Value>, trusted: bool) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct Extra {
            bbox: Option<[f64; 4]>,
            max_px: Option<u32>,
            name: Option<String>,
            opacity: Option<f64>,
            layer_name: Option<String>,
            design_layer: Option<String>,
            class: Option<ClassId>,
            wait: Option<bool>,
        }
        let mut s = m.clone();
        for k in [
            "bbox",
            "max_px",
            "name",
            "opacity",
            "layer_name",
            "design_layer",
            "class",
            "wait",
        ] {
            s.remove(k);
        }
        let source: WmtsSource = parse(&s)?;
        let ex: Extra = parse(m)?;
        self.check_url(&source.url, trusted)?;
        let design = self.design_layer(None, ex.layer_name.or(ex.design_layer))?;
        let g = self.doc_frame()?;
        let view = self.view_of(ex.bbox, ex.max_px)?;
        let src_json = json!({"kind": "wmts", "source": source, "bbox": [view.min.x, view.min.y, view.max.x, view.max.y], "max_px": view.target_px});
        let opacity = check_opacity(ex.opacity)?;
        let name = ex.name.unwrap_or_default();
        let class = ex.class;
        self.submit("WMTS map", trusted, ex.wait, move |http, cache| {
            let doc_crs = Crs::parse(&g.crs).map_err(GisError::Crs)?;
            let caps = wmts::fetch_capabilities(http, &source.url)?;
            let out = wmts::fetch_map(http, Some(cache), &source, &caps, &doc_crs, &g, &view)?;
            Ok(Outcome::Map(Box::new(MapJob {
                out,
                source: src_json,
                name,
                layer: design,
                class,
                opacity,
                replace: None,
            })))
        })
    }

    fn refresh(&mut self, m: &Map<String, Value>, trusted: bool) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct P {
            id: ObjectId,
            bbox: Option<[f64; 4]>,
            max_px: Option<u32>,
            wait: Option<bool>,
        }
        let P {
            id,
            bbox,
            max_px,
            wait,
        } = parse(m)?;
        let o = self
            .editor
            .doc()
            .object(id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let Shape::Underlay(u) = &o.shape else {
            return Err(bad(format!("object {id} is not an underlay")));
        };
        let stored = o.data.get("gis_source").cloned().ok_or_else(|| {
            bad("this underlay has no source to fetch again (it came from a file)")
        })?;
        let (name, opacity) = (u.name.clone(), u.opacity);
        let g = self.doc_frame()?;
        let old_bbox: Option<[f64; 4]> = stored
            .get("bbox")
            .and_then(|b| serde_json::from_value(b.clone()).ok());
        let old_px = stored
            .get("max_px")
            .and_then(Value::as_u64)
            .map(|x| x as u32);
        let view = self.view_of(bbox.or(old_bbox), max_px.or(old_px))?;
        let kind = stored
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("wms")
            .to_string();
        let src = stored.get("source").cloned().unwrap_or(Value::Null);
        let src_json = json!({"kind": kind, "source": src, "bbox": [view.min.x, view.min.y, view.max.x, view.max.y], "max_px": view.target_px});
        let url = src
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.check_url(&url, trusted)?;
        self.submit("map refresh", trusted, wait, move |http, cache| {
            let doc_crs = Crs::parse(&g.crs).map_err(GisError::Crs)?;
            let out = if kind == "wmts" {
                let source: WmtsSource =
                    serde_json::from_value(src).map_err(|e| GisError::Param(e.to_string()))?;
                let caps = wmts::fetch_capabilities(http, &source.url)?;
                wmts::fetch_map(http, Some(cache), &source, &caps, &doc_crs, &g, &view)?
            } else {
                let source: WmsSource =
                    serde_json::from_value(src).map_err(|e| GisError::Param(e.to_string()))?;
                let caps = wms::fetch_capabilities(http, &source.url)?;
                wms::fetch_map(http, Some(cache), &source, &caps, &doc_crs, &g, &view)?
            };
            Ok(Outcome::Map(Box::new(MapJob {
                out,
                source: src_json,
                name,
                layer: None,
                class: None,
                opacity,
                replace: Some(id),
            })))
        })
    }

    fn wfs_import(&mut self, m: &Map<String, Value>, trusted: bool) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct Extra {
            #[serde(default)]
            options: Value,
            caps: Option<bool>,
            wait: Option<bool>,
        }
        let mut s = m.clone();
        for k in ["options", "caps", "wait"] {
            s.remove(k);
        }
        let source: WfsSource = parse(&s)?;
        let ex: Extra = parse(m)?;
        self.check_url(&source.url, trusted)?;
        let mut options = ImportOptions::from_json(&ex.options).map_err(bad)?;
        if options.layer.is_none() {
            options.layer = Some(
                source
                    .type_name
                    .rsplit(':')
                    .next()
                    .unwrap_or(&source.type_name)
                    .to_string(),
            );
        }
        let doc_frame = self
            .editor
            .doc()
            .georef
            .clone()
            .filter(|g| !g.crs.trim().is_empty());
        let use_caps = ex.caps.unwrap_or(true);
        self.submit("WFS import", trusted, ex.wait, move |http, _| {
            let caps = if use_caps {
                Some(wfs::fetch_capabilities(http, &source.url)?)
            } else {
                None
            };
            let doc_crs = match &doc_frame {
                Some(g) => Some((Crs::parse(&g.crs).map_err(GisError::Crs)?, g.clone())),
                None => None,
            };
            let fetched = wfs::fetch_features(
                http,
                &source,
                caps.as_ref(),
                doc_crs.as_ref().map(|(c, g)| (c, g)),
            )?;
            Ok(Outcome::Features { fetched, options })
        })
    }

    fn geoimage_import(
        &mut self,
        m: &Map<String, Value>,
        trusted: bool,
    ) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        struct P {
            path: PathBuf,
            crs: Option<String>,
            max_px: Option<u32>,
            name: Option<String>,
            opacity: Option<f64>,
            layer: Option<Value>,
            layer_name: Option<String>,
            class: Option<ClassId>,
        }
        let p: P = parse(m)?;
        let path = if trusted {
            p.path.clone()
        } else {
            self.check_path(&p.path, false, &["png", "jpg", "jpeg", "tif", "tiff"])
                .map_err(RpcError::policy)?
        };
        let mut img =
            cadektor_gis::geoimage::load_file(&path, p.crs.as_deref()).map_err(gis_err)?;
        if let Some(n) = p.name {
            img.name = n;
        }
        let max_px = p.max_px.unwrap_or(2048);
        if max_px == 0 || max_px > cadektor_core::underlay::MAX_SIDE_PX {
            return Err(bad(format!(
                "max_px must be between 1 and {}",
                cadektor_core::underlay::MAX_SIDE_PX
            )));
        }
        let placed = cadektor_gis::geoimage::place(&img, self.editor.doc().georef.as_ref(), max_px)
            .map_err(gis_err)?;
        let design = self.design_layer(p.layer.as_ref(), p.layer_name)?;
        let mut u = placed.underlay;
        u.opacity = check_opacity(p.opacity)?;
        u.source = format!(
            "{}.{}",
            img.name,
            path.extension().map_or("", |e| e.to_str().unwrap_or(""))
        );
        let (w, h) = (u.width, u.height);
        let next = ObjectId(self.editor.doc().next_id());
        let mut commands = Vec::new();
        if let Some(g) = &placed.new_georef {
            commands.push(Command::SetGeoreference {
                georef: Some(g.clone()),
            });
        }
        commands.push(Command::AddObject {
            shape: Shape::Underlay(Box::new(u)),
            layer: design,
            class: p.class,
            attrs: Attributes::default(),
        });
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({
            "id": next,
            "width": w,
            "height": h,
            "georeference": placed.new_georef,
            "notes": placed.notes,
        }))
    }

    /// The design layer for a new underlay: by ID or name. `None`: the active layer.
    fn design_layer(
        &self,
        by_id: Option<&Value>,
        by_name: Option<String>,
    ) -> Result<Option<LayerId>, RpcError> {
        let doc: &Document = self.editor.doc();
        if let Some(Value::Number(n)) = by_id {
            let id = LayerId(n.as_u64().ok_or_else(|| bad("layer must be an ID"))?);
            return doc
                .layer(id)
                .map(|_| Some(id))
                .ok_or_else(|| bad(format!("layer {id} does not exist")));
        }
        let name = by_name.or_else(|| by_id.and_then(Value::as_str).map(str::to_string));
        match name {
            None => Ok(None),
            Some(n) => doc
                .layers
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(&n))
                .map(|l| Some(l.id))
                .ok_or_else(|| bad(format!("no layer named {n}"))),
        }
    }
}

fn job_id(m: &Map<String, Value>) -> Result<u64, RpcError> {
    m.get("job")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("missing 'job'"))
}

fn check_opacity(o: Option<f64>) -> Result<f64, RpcError> {
    match o {
        None => Ok(1.0),
        Some(v) if (0.0..=1.0).contains(&v) => Ok(v),
        Some(_) => Err(bad("opacity must be between 0 and 1")),
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn cache_dir_follows_the_environment() {
        let d = default_cache_dir();
        assert!(d.ends_with("tiles") || std::env::var_os("CADEKTOR_GIS_CACHE").is_some());
    }

    #[test]
    fn opacity_and_job_ids() {
        assert_eq!(check_opacity(None).unwrap(), 1.0);
        assert_eq!(check_opacity(Some(0.5)).unwrap(), 0.5);
        assert!(check_opacity(Some(2.0)).is_err());
        assert!(job_id(&Map::new()).is_err());
    }
}
