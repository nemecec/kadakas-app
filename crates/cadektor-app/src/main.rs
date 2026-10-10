// The web build leaves out the native features (agent server, files, GIS, scripts); their code is then unused.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

#[cfg(not(target_arch = "wasm32"))]
mod agent;
mod annot_ui;
mod app;
mod area_ui;
mod autosave;
#[cfg(not(target_arch = "wasm32"))]
mod autosave_fs;
mod autosave_ui;
mod canvas;
mod canvas3d;
mod data_ui;
mod databar;
mod file_io;
#[cfg(not(target_arch = "wasm32"))]
mod gis_dialogs;
mod gis_ui;
mod gpu3d;
mod grading_ui;
mod hit;
#[cfg(not(target_arch = "wasm32"))]
mod io_dialog;
mod irrigation_ui;
mod keymap;
mod panels;
mod plant_draw;
mod plant_sheet_ui;
mod plant_ui;
mod present_ui;
mod resource_ui;
mod rules_ui;
#[cfg(not(target_arch = "wasm32"))]
mod script_ui;
mod sheets_ui;
mod site_ui;
mod snap;
mod tess;
mod tools;
mod vertex;
mod view;
mod view3d;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let env = std::env::var("CADEKTOR_AGENT_PORT").ok();
    let port = agent::parse_agent_port(&args, env.as_deref());
    let allow_dirs = agent::parse_allow_dirs(&args);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("Cadektor"),
        ..Default::default()
    };
    eframe::run_native(
        "Cadektor",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::CadektorApp::new(
                cc,
                port,
                allow_dirs.clone(),
            )))
        }),
    )
}

/// The web entry point. `index.html` has a canvas with the id `cadektor_canvas`.
/// eframe asks for WebGPU first and falls back to WebGL2.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("no document");
        let canvas = document
            .get_element_by_id("cadektor_canvas")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("index.html needs a canvas with the id cadektor_canvas");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(app::CadektorApp::new_web(cc)))),
            )
            .await;
        if let Some(note) = document.get_element_by_id("loading_text") {
            match result {
                Ok(()) => note.remove(),
                Err(e) => note.set_inner_html(&format!(
                    "Cadektor could not start. This browser needs WebGPU or WebGL2. ({e:?})"
                )),
            }
        }
    });
}
