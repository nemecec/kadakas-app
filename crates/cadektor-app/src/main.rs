mod app;
#[cfg(not(target_arch = "wasm32"))]
mod agent;
mod canvas;
mod canvas3d;
mod databar;
mod file_io;
mod gpu3d;
mod hit;
#[cfg(not(target_arch = "wasm32"))]
mod io_dialog;
mod keymap;
mod panels;
mod plant_draw;
mod plant_ui;
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
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_title("Cadektor"),
        ..Default::default()
    };
    eframe::run_native("Cadektor", options, Box::new(move |cc| Ok(Box::new(app::CadektorApp::new(cc, port, allow_dirs.clone())))))
}

// The web entry point is not set up yet. The modules above are wasm compatible.
#[cfg(target_arch = "wasm32")]
fn main() {}
