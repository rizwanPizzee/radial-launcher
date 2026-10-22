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
    /// Colour of the wheel slices as `#RRGGBB`.
    pub wheel_color: String,
    /// Colour of the centre hub as `#RRGGBB`.
    pub hub_color: String,
    /// Wheel design: segments, band, orbs, halo, arc, or disc.
    pub design: String,
    /// Bring the selected app to the foreground after launching it.
    pub focus_app: bool,
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
            restore_cursor: false,
            accent: "#4C9AFF".into(),
            wheel_color: "#16181E".into(),
            hub_color: "#101115".into(),
            design: "segments".into(),
            focus_app: true,
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
        if !matches!(
            self.design.to_ascii_lowercase().as_str(),
            "segments" | "band" | "orbs" | "halo" | "arc" | "disc"
        ) {
            self.design = "segments".into();
        } else {
            self.design = self.design.to_ascii_lowercase();
        }
        self.accent = Self::normalize_color(&self.accent, "#4C9AFF");
        self.wheel_color = Self::normalize_color(&self.wheel_color, "#16181E");
        self.hub_color = Self::normalize_color(&self.hub_color, "#101115");
        self
    }

    fn normalize_color(color: &str, fallback: &str) -> String {
        let hex = color.trim().trim_start_matches('#');
        if hex.len() == 6 {
            if u32::from_str_radix(hex, 16).is_ok() {
                return format!("#{hex}").to_ascii_uppercase();
            }
        }
        fallback.into()
    }

    pub fn color_rgb(color: &str, fallback: [u8; 3]) -> [u8; 3] {
        let hex = color.trim().trim_start_matches('#');
        if hex.len() == 6 {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return [(v >> 16) as u8, (v >> 8) as u8, v as u8];
            }
        }
        fallback
    }

    pub fn accent_rgb(&self) -> [u8; 3] {
        Self::color_rgb(&self.accent, [76, 154, 255])
    }

    pub fn wheel_rgb(&self) -> [u8; 3] {
        Self::color_rgb(&self.wheel_color, [22, 24, 30])
    }

    pub fn hub_rgb(&self) -> [u8; 3] {
        Self::color_rgb(&self.hub_color, [16, 17, 21])
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

pub fn save(cfg: &Config) -> Result<(), String> {
    let path = config_path();
    std::fs::create_dir_all(config_dir()).map_err(|e| format!("{}\n{e}", path.display()))?;
    let json = serde_json::to_vec_pretty(&cfg.clone().sanitized())
        .map_err(|e| format!("{}\n{e}", path.display()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json).map_err(|e| format!("{}\n{e}", temp.display()))?;
    std::fs::rename(&temp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("{}\n{e}", path.display())
    })
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

    #[test]
    fn customization_fields_have_safe_defaults_and_invalid_values_are_sanitized() {
        let cfg = Config::parse(
            r##"{"design":"unknown","accent":"nope","wheel_color":"#abcdef","restore_cursor":false}"##,
        )
        .unwrap();
        assert_eq!(cfg.design, "segments");
        assert_eq!(cfg.accent, "#4C9AFF");
        assert_eq!(cfg.wheel_color, "#ABCDEF");
        assert!(!cfg.restore_cursor);
        assert!(cfg.focus_app);
    }
}
