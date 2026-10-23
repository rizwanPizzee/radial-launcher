// No console window for the release build.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod platform;

use eframe::egui;
use radial_core::config;

fn main() -> eframe::Result {
    if !platform::first_instance() {
        return Ok(());
    }

    let initial = match config::load_or_create() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error, starting with defaults: {e}");
            config::Config::default()
        }
    };
    let shot = std::env::var("RADIAL_SETTINGS_SHOT").ok();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Radial Launcher Settings")
            .with_inner_size([1060.0, 700.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Radial Launcher Settings",
        options,
        Box::new(move |cc| Ok(Box::new(app::SettingsApp::new(cc, initial, shot)))),
    )
}
