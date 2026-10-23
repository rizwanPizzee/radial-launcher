//! Everything that differs between Windows (the real thing) and other systems (where the app
//! only runs for development and screenshots).

use std::path::PathBuf;

#[cfg(windows)]
pub use std::os::windows::process::CommandExt;

/// `Command::creation_flags` exists only on Windows; elsewhere it does nothing.
#[cfg(not(windows))]
#[allow(dead_code)]
pub trait CommandExt {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}
#[cfg(not(windows))]
impl CommandExt for std::process::Command {
    fn creation_flags(&mut self, _flags: u32) -> &mut Self {
        self
    }
}

/// An app in the Start menu.
#[derive(Clone, Debug)]
pub struct StartApp {
    pub name: String,
    /// Value to use as the item's `path`.
    pub path: String,
}

#[cfg(windows)]
#[path = "platform_win.rs"]
mod imp;

#[cfg(not(windows))]
mod imp {
    use super::*;
    use radial_core::config::ItemConfig;
    use radial_core::text::TextRenderer;
    use tiny_skia::Pixmap;

    pub struct Session;
    impl Session {
        pub fn new() -> Session {
            Session
        }
    }
    pub fn text_renderer(_face: &str) -> Box<dyn TextRenderer> {
        #[cfg(feature = "fontdue-text")]
        if let Some(t) = radial_core::text::FontdueText::from_files(
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ) {
            return Box::new(t);
        }
        struct NoText;
        impl TextRenderer for NoText {
            fn render(&mut self, _: &str, _: f32, _: bool, _: Option<u32>) -> Option<Pixmap> {
                None
            }
        }
        Box::new(NoText)
    }
    /// Development stand-in: only PNG pictures can be loaded here (no shell icons off Windows).
    pub fn load_icon(item: &ItemConfig) -> Option<Pixmap> {
        #[cfg(feature = "fontdue-text")]
        if item.has_custom_icon() {
            let pm = Pixmap::load_png(item.icon.trim()).ok()?;
            return Some(radial_core::iconcache::fit_icon(&pm));
        }
        let _ = item;
        None
    }
    pub fn font_families() -> Vec<String> {
        ["Arial", "Bahnschrift", "Calibri", "Cascadia Code", "Consolas", "Georgia", "Segoe UI", "Tahoma", "Verdana"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    pub fn pick_image() -> Option<String> {
        None
    }
    /// A settings file to import.
    pub fn pick_settings_file() -> Option<String> {
        None
    }
    /// Where to save an exported settings file.
    pub fn pick_save_settings(_default_name: &str) -> Option<String> {
        None
    }
    pub fn system_fonts() -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }
    pub fn pick_file() -> Option<String> {
        None
    }
    pub fn pick_folder() -> Option<String> {
        None
    }
    pub fn list_start_apps() -> Result<Vec<StartApp>, String> {
        Ok(["Calculator", "Firefox", "Notepad", "Paint", "Terminal", "Visual Studio Code"]
            .iter()
            .map(|n| StartApp { name: n.to_string(), path: format!("shell:AppsFolder\\{n}") })
            .collect())
    }
    pub fn autostart_enabled() -> bool {
        false
    }
    pub fn set_autostart(_on: bool, _exe: &std::path::Path) -> bool {
        false
    }
    pub fn notify_launcher() -> bool {
        false
    }
    pub fn first_instance() -> bool {
        true
    }
    pub fn raise_self() {}
}

pub use imp::*;

/// The launcher exe that sits next to this one.
pub fn launcher_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join(if cfg!(windows) { "radial-launcher.exe" } else { "radial-launcher" });
    p.exists().then_some(p)
}

/// Start the launcher if it is not running (the single-instance check inside it makes this safe).
pub fn start_launcher() -> bool {
    launcher_exe().map(|p| std::process::Command::new(p).spawn().is_ok()).unwrap_or(false)
}
