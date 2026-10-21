// No console window for the release build.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("radial-launcher only runs on Windows. The platform-independent core can be tested anywhere with: cargo test --lib");
}
