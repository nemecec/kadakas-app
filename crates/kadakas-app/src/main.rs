// The web build leaves out the native features (agent server, files, GIS, scripts); their code is then unused.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

mod about;
#[cfg(not(target_arch = "wasm32"))]
mod agent;
mod agent_view;
mod agent_view_ui;
mod annot_ui;
mod app;
mod area_ui;
mod array_ui;
mod autosave;
#[cfg(not(target_arch = "wasm32"))]
mod autosave_fs;
mod autosave_ui;
#[cfg(not(target_arch = "wasm32"))]
mod bcf_dialog;
mod canvas;
mod canvas3d;
mod collab_ui;
mod compare_draw;
mod compare_ui;
mod data_ui;
mod databar;
#[cfg(not(target_arch = "wasm32"))]
mod drawings_ui;
mod enum_names;
mod feedback_ui;
mod file_io;
#[cfg(not(target_arch = "wasm32"))]
mod gis_dialogs;
mod gis_ui;
mod gpu3d;
mod grading_ui;
mod icon_size;
mod icons;
#[cfg(not(target_arch = "wasm32"))]
mod io_dialog;
mod irrigation_ui;
mod issues_ui;
mod joint_ui;
mod keymap;
mod layout;
mod locale;
mod open_gate;
mod panels;
mod picture_ui;
mod plan_draw;
mod plan_worker;
mod plant_draw;
mod plant_sheet_ui;
mod plant_ui;
mod plugin_ui;
mod prefs;
mod prefs_ui;
mod present_ui;
mod profile_store;
mod profiles;
mod quick_search;
mod resource_ui;
mod rules_ui;
mod screenshot;
#[cfg(not(target_arch = "wasm32"))]
mod script_ui;
mod sheets_ui;
mod site_ui;
mod snap_ui;
mod solid_ui;
mod style_ui;
mod tess;
mod theme;
mod units_ui;
#[cfg(not(target_arch = "wasm32"))]
mod update_ui;
mod versions_ui;
mod vertex;
mod view;
mod view3d;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let env = std::env::var("KADAKAS_AGENT_PORT").ok();
    let port = agent::parse_agent_port(&args, env.as_deref());
    let allow_dirs = agent::parse_allow_dirs(&args);
    // Collaboration settings (`--collab-url`, `--collab-token`, `--allow-collab-url` and the
    // variables). Then the token variables leave the environment, so no child process (a Python
    // script, a converter) can read them.
    let collab = kadakas_ui_core::collab::CollabArgs::parse(&args, &|k| std::env::var(k).ok());
    kadakas_sys::process::remove_secrets_from_env();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("Kadakas"),
        ..Default::default()
    };
    eframe::run_native(
        "Kadakas",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::KadakasApp::new(
                cc,
                port,
                allow_dirs.clone(),
                collab.clone(),
            )))
        }),
    )
}

/// The web entry point. `index.html` has a canvas with the id `kadakas_canvas`.
/// eframe asks for WebGPU first and falls back to WebGL2.
#[cfg(target_arch = "wasm32")]
fn main() {
    use kadakas_l10n::tr;
    use wasm_bindgen::JsCast as _;

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("no document");
        let canvas = document
            .get_element_by_id("kadakas_canvas")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("index.html needs a canvas with the id kadakas_canvas");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(app::KadakasApp::new_web(cc)))),
            )
            .await;
        if let Some(note) = document.get_element_by_id("loading_text") {
            match result {
                Ok(()) => note.remove(),
                Err(e) => {
                    note.set_inner_html(&tr!("main-could-not-start", error = format!("{e:?}")))
                }
            }
        }
    });
}
