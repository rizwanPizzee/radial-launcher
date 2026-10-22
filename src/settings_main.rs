#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "win/util.rs"]
mod util;

#[cfg(windows)]
const WM_RELOAD_CONFIG: u32 = 0x8000 + 21;

#[cfg(windows)]
#[path = "win/settings.rs"]
mod settings;

#[cfg(windows)]
fn main() {
    settings::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("radial-launcher-settings only runs on Windows.");
}
