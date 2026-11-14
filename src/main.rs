// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod canvas;
mod doc;
mod io;
mod raster;
mod text;
mod tools;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Paintpot")
            .with_app_id("paintpot")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Paintpot",
        options,
        Box::new(move |cc| Ok(Box::new(app::PaintApp::new(cc, file)))),
    )
}
