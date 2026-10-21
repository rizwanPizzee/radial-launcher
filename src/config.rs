//! JSON configuration stored in `%APPDATA%\RadialLauncher\config.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_ITEMS: usize = 12;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ItemConfig {
    /// Text shown in the middle of the wheel while the slice is hovered.
    pub name: String,
    /// Executable, document, URL, or `shell:AppsFolder\<AppUserModelID>` for Store apps.
    /// Environment variables such as `%SystemRoot%` are expanded.
    pub path: String,
    /// Optional command-line arguments.
    #[serde(default)]
    pub args: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Wheel diameter in 96-dpi pixels (scaled automatically on high-DPI displays).
    pub size: u32,
    /// `"screen"`: wheel opens in the middle of the monitor (pointer is moved there,
    /// like a game inventory wheel). `"cursor"`: wheel opens under the pointer.
    pub center: String,
    /// With `center = "screen"`: put the pointer back where it was when the wheel closes.
    pub restore_cursor: bool,
    /// Highlight colour as `#RRGGBB`.
    pub accent: String,
    /// 1 to 12 entries, laid out clockwise starting at the top.
    pub items: Vec<ItemConfig>,
}

fn item(name: &str, path: &str) -> ItemConfig {
    ItemConfig {
        name: name.into(),
        path: path.into(),
        args: String::new(),
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            size: 520,
            center: "screen".into(),
            restore_cursor: true,
            accent: "#4C9AFF".into(),
            items: vec![
                item("Notepad", "%SystemRoot%\\System32\\notepad.exe"),
                item("Calculator", "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
                item("File Explorer", "%SystemRoot%\\explorer.exe"),
                item("Microsoft Edge", "%ProgramFiles(x86)%\\Microsoft\\Edge\\Application\\msedge.exe"),
                item("Paint", "%SystemRoot%\\System32\\mspaint.exe"),
                item("Task Manager", "%SystemRoot%\\System32\\Taskmgr.exe"),
                item("Command Prompt", "%SystemRoot%\\System32\\cmd.exe"),
                item(
                    "Settings",
                    "shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel",
                ),
            ],
        }
    }
}

impl Config {
    /// Clamp values into ranges the renderer supports.
    pub fn sanitized(mut self) -> Self {
        self.size = self.size.clamp(300, 900);
        self.items.truncate(MAX_ITEMS);
        if self.center != "cursor" {
            self.center = "screen".into();
        }
        self
    }

    pub fn accent_rgb(&self) -> [u8; 3] {
        let hex = self.accent.trim().trim_start_matches('#');
        if hex.len() == 6 {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return [(v >> 16) as u8, (v >> 8) as u8, v as u8];
            }
        }
        [76, 154, 255]
    }

    pub fn parse(text: &str) -> Result<Config, serde_json::Error> {
        // Older Notepad versions save UTF-8 with a BOM, which serde_json rejects.
        serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')).map(Config::sanitized)
    }
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("RadialLauncher")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Read the config file; write the defaults there first if it does not exist yet.
pub fn load_or_create() -> Result<Config, String> {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text).map_err(|e| format!("{}\n{e}", path.display())),
        Err(_) => {
            let cfg = Config::default();
            let _ = std::fs::create_dir_all(config_dir());
            if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                let _ = std::fs::write(&path, json);
            }
            Ok(cfg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_json() {
        let cfg = Config::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        assert_eq!(Config::parse(&json).unwrap(), cfg);
    }

    #[test]
    fn bom_and_partial_files_are_accepted() {
        let cfg = Config::parse("\u{feff}{ \"size\": 600 }").unwrap();
        assert_eq!(cfg.size, 600);
        assert_eq!(cfg.items.len(), 8); // missing keys fall back to defaults
    }

    #[test]
    fn values_are_clamped() {
        let cfg = Config::parse("{ \"size\": 5000, \"center\": \"nonsense\" }").unwrap();
        assert_eq!(cfg.size, 900);
        assert_eq!(cfg.center, "screen");
    }

    #[test]
    fn accent_parsing() {
        let mut cfg = Config::default();
        cfg.accent = "#FF8000".into();
        assert_eq!(cfg.accent_rgb(), [255, 128, 0]);
        cfg.accent = "garbage".into();
        assert_eq!(cfg.accent_rgb(), [76, 154, 255]);
    }
}
