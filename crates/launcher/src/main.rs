// No console window for the release build.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    // Helper mode: extract app icons into the icon cache and exit. The resident launcher
    // starts this as a separate short-lived process so the shell/imaging code it needs is
    // never loaded into the long-running one.
    if std::env::args().any(|a| a == "--refresh-icons") {
        win::refresh_icons_and_exit();
    } else {
        win::run();
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("radial-launcher only runs on Windows. The core logic can be tested anywhere with: cargo test -p radial-core");
}
