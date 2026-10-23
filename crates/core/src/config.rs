//! Configuration stored as JSON in `%APPDATA%\RadialLauncher\config.json`.
//!
//! Every field has a default, so partial files (and files written by older versions)
//! keep working. The settings app is the normal way to edit this; the file stays
//! human-readable for people who prefer a text editor.

use crate::color::Color;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_ITEMS: usize = 12;
/// Special `path` value: opens the settings app instead of launching a program.
pub const SETTINGS_ACTION: &str = "@settings";

// ------------------------------------------------------------------ style

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// Separated wedges that pop out when hovered, big hub with name + description.
    Segments,
    /// Thick ring of wedges split by thin lines, with a pointer on the hub.
    Band,
    /// Round icon bubbles on a thin ring, name + description in the middle.
    Orbs,
    /// Slim light ring of icons around a big hub that shows the hovered app.
    Halo,
    /// Dark ring with an accent arc marking the hovered wedge.
    Arc,
    /// Full disc cut into wedges, each with its icon and name.
    Disc,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StylePreset {
    pub thickness: u8,
    pub gap: f32,
    pub labels: bool,
}

impl Style {
    pub const ALL: [Style; 6] = [Style::Segments, Style::Band, Style::Orbs, Style::Halo, Style::Arc, Style::Disc];

    pub fn label(self) -> &'static str {
        match self {
            Style::Segments => "Segments",
            Style::Band => "Band",
            Style::Orbs => "Orbs",
            Style::Halo => "Halo",
            Style::Arc => "Arc",
            Style::Disc => "Disc",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Style::Segments => "Separate wedges that pop out",
            Style::Band => "Thick ring split by thin lines",
            Style::Orbs => "Round bubbles on a ring",
            Style::Halo => "Slim ring, big info hub",
            Style::Arc => "Dark ring with accent arc",
            Style::Disc => "Full disc with names",
        }
    }

    /// What the "thickness" slider controls for this style.
    pub fn size_knob(self) -> &'static str {
        match self {
            Style::Orbs => "Bubble size",
            Style::Disc => "Centre hub size",
            _ => "Ring thickness",
        }
    }

    /// Sensible defaults applied when the user switches to this style.
    pub fn preset(self) -> StylePreset {
        match self {
            Style::Segments => StylePreset { thickness: 46, gap: 3.0, labels: false },
            Style::Band => StylePreset { thickness: 36, gap: 0.0, labels: false },
            Style::Orbs => StylePreset { thickness: 40, gap: 0.0, labels: false },
            Style::Halo => StylePreset { thickness: 20, gap: 2.0, labels: false },
            Style::Arc => StylePreset { thickness: 38, gap: 1.5, labels: false },
            Style::Disc => StylePreset { thickness: 24, gap: 0.0, labels: true },
        }
    }
}

// ------------------------------------------------------------------ theme

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Theme {
    /// Fill of the wheel's slices / ring (alpha = see-through amount).
    pub background: Color,
    /// Fill of the centre hub.
    pub hub: Color,
    /// Highlight colour of the hovered slice.
    pub hover: Color,
    /// Text colour on the slices.
    pub text: Color,
    /// Text colour inside the hub.
    pub hub_text: Color,
    /// Outlines and divider lines.
    pub border: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::midnight()
    }
}

impl Theme {
    pub fn midnight() -> Theme {
        Theme {
            background: Color::rgba(22, 24, 30, 214),
            hub: Color::rgba(16, 17, 21, 235),
            hover: Color::rgb(76, 154, 255),
            text: Color::rgb(232, 234, 240),
            hub_text: Color::rgb(255, 255, 255),
            border: Color::rgba(255, 255, 255, 40),
        }
    }

    pub fn presets() -> Vec<(&'static str, Theme)> {
        vec![
            ("Midnight", Theme::midnight()),
            (
                "Cream",
                Theme {
                    background: Color::rgba(243, 233, 220, 245),
                    hub: Color::rgba(46, 46, 49, 242),
                    hover: Color::rgb(45, 156, 255),
                    text: Color::rgb(43, 43, 46),
                    hub_text: Color::rgb(255, 255, 255),
                    border: Color::rgba(0, 0, 0, 50),
                },
            ),
            (
                "Crimson",
                Theme {
                    background: Color::rgba(30, 28, 30, 228),
                    hub: Color::rgba(8, 8, 8, 245),
                    hover: Color::rgb(210, 36, 44),
                    text: Color::rgb(230, 224, 220),
                    hub_text: Color::rgb(242, 237, 232),
                    border: Color::rgba(255, 255, 255, 45),
                },
            ),
            (
                "Graphite",
                Theme {
                    background: Color::rgba(58, 58, 60, 236),
                    hub: Color::rgba(30, 30, 32, 245),
                    hover: Color::rgb(150, 150, 158),
                    text: Color::rgb(237, 237, 237),
                    hub_text: Color::rgb(255, 255, 255),
                    border: Color::rgba(255, 255, 255, 50),
                },
            ),
            (
                "Ocean",
                Theme {
                    background: Color::rgba(14, 38, 58, 226),
                    hub: Color::rgba(8, 22, 36, 242),
                    hover: Color::rgb(31, 200, 219),
                    text: Color::rgb(216, 241, 255),
                    hub_text: Color::rgb(255, 255, 255),
                    border: Color::rgba(180, 230, 255, 50),
                },
            ),
            (
                "Forest",
                Theme {
                    background: Color::rgba(22, 42, 30, 226),
                    hub: Color::rgba(12, 26, 18, 242),
                    hover: Color::rgb(92, 203, 110),
                    text: Color::rgb(225, 245, 229),
                    hub_text: Color::rgb(255, 255, 255),
                    border: Color::rgba(200, 255, 210, 45),
                },
            ),
            (
                "Violet",
                Theme {
                    background: Color::rgba(36, 26, 58, 226),
                    hub: Color::rgba(22, 15, 38, 242),
                    hover: Color::rgb(166, 107, 255),
                    text: Color::rgb(234, 223, 255),
                    hub_text: Color::rgb(255, 255, 255),
                    border: Color::rgba(220, 200, 255, 45),
                },
            ),
            (
                "Light",
                Theme {
                    background: Color::rgba(250, 250, 252, 240),
                    hub: Color::rgba(255, 255, 255, 250),
                    hover: Color::rgb(59, 130, 246),
                    text: Color::rgb(31, 36, 48),
                    hub_text: Color::rgb(31, 36, 48),
                    border: Color::rgba(0, 0, 0, 40),
                },
            ),
        ]
    }
}

// ------------------------------------------------------------------ items

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ItemConfig {
    /// Shown in the hub while the slice is hovered.
    pub name: String,
    /// Smaller second line in the hub (optional).
    pub description: String,
    /// Executable, document, folder, URL, `shell:AppsFolder\<AppUserModelID>`,
    /// or `@settings`. `%ENV%` variables are expanded.
    pub path: String,
    /// Optional command-line arguments.
    pub args: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Settings,
}

impl ItemConfig {
    pub fn new(name: &str, path: &str) -> Self {
        ItemConfig { name: name.into(), path: path.into(), ..Default::default() }
    }

    pub fn builtin(&self) -> Option<Builtin> {
        (self.path.trim() == SETTINGS_ACTION).then_some(Builtin::Settings)
    }

    /// `Some(true/false)` if we can tell whether the target exists, `None` if it
    /// cannot be checked cheaply (URLs, `shell:` locations, built-in actions).
    pub fn target_exists(&self) -> Option<bool> {
        let p = self.path.trim();
        if p.is_empty() {
            return Some(false);
        }
        if self.builtin().is_some() || p.contains("://") || p.to_ascii_lowercase().starts_with("shell:") {
            return None;
        }
        let expanded = expand_env(p);
        let path = Path::new(&expanded);
        if path.exists() {
            return Some(true);
        }
        // A bare name such as `notepad.exe` is looked up on PATH, like the shell does.
        if !expanded.contains(['\\', '/']) {
            if let Some(paths) = std::env::var_os("PATH") {
                let found = std::env::split_paths(&paths).any(|d| d.join(&expanded).exists());
                return Some(found);
            }
        }
        Some(false)
    }
}

/// Expand `%NAME%` environment variables (unknown names are left as they are).
pub fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------ config

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Wheel diameter at 100 % display scaling.
    pub size: u32,
    pub style: Style,
    pub theme: Theme,
    /// Overall opacity of the wheel in percent.
    pub opacity: u8,
    /// Ring thickness / bubble size / hub size, depending on the style (percent).
    pub thickness: u8,
    /// Gap between wedges in degrees.
    pub gap: f32,
    /// Icon size in percent.
    pub icon_scale: u8,
    /// Text size in percent.
    pub text_scale: u8,
    /// Show the app name on each slice (always on for the Disc style).
    pub labels: bool,
    /// Short fade-in when the wheel opens.
    pub animation: bool,
    /// `"screen"`: the wheel opens in the middle of the monitor and the pointer is moved
    /// there. `"cursor"`: it opens under the pointer.
    pub center: String,
    /// Move the pointer back to where it was before the wheel opened. Off by default:
    /// the pointer stays where you released the button.
    pub return_cursor: bool,
    /// Releasing the button sooner than this (ms) keeps the wheel open for a click.
    pub tap_ms: u32,
    /// Bring a launched app to the front when Windows opens it behind other windows.
    pub focus_launched: bool,
    /// 1 to 12 entries, clockwise from the top.
    pub items: Vec<ItemConfig>,

    /// Read from files written by version 1 only; never written back.
    #[serde(default, skip_serializing)]
    accent: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let p = Style::Segments.preset();
        Config {
            size: 520,
            style: Style::Segments,
            theme: Theme::midnight(),
            opacity: 100,
            thickness: p.thickness,
            gap: p.gap,
            icon_scale: 100,
            text_scale: 100,
            labels: p.labels,
            animation: true,
            center: "screen".into(),
            return_cursor: false,
            tap_ms: 250,
            focus_launched: true,
            items: vec![
                ItemConfig::new("Notepad", "%SystemRoot%\\System32\\notepad.exe"),
                ItemConfig::new("Calculator", "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
                ItemConfig::new("File Explorer", "%SystemRoot%\\explorer.exe"),
                ItemConfig::new("Microsoft Edge", "%ProgramFiles(x86)%\\Microsoft\\Edge\\Application\\msedge.exe"),
                ItemConfig::new("Paint", "%SystemRoot%\\System32\\mspaint.exe"),
                ItemConfig::new("Task Manager", "%SystemRoot%\\System32\\Taskmgr.exe"),
                ItemConfig::new("Command Prompt", "%SystemRoot%\\System32\\cmd.exe"),
                ItemConfig {
                    name: "Radial Settings".into(),
                    description: "Change the look and the apps".into(),
                    path: SETTINGS_ACTION.into(),
                    args: String::new(),
                },
            ],
            accent: None,
        }
    }
}

impl Config {
    /// Switch design and apply that design's recommended sizes.
    pub fn set_style(&mut self, style: Style) {
        let p = style.preset();
        self.style = style;
        self.thickness = p.thickness;
        self.gap = p.gap;
        self.labels = p.labels;
    }

    /// Clamp values into ranges the renderer supports and migrate old settings.
    pub fn sanitized(mut self) -> Self {
        if let Some(hex) = self.accent.take() {
            if let Some(c) = Color::from_hex(&hex) {
                self.theme.hover = c;
            }
        }
        self.size = self.size.clamp(300, 900);
        self.opacity = self.opacity.clamp(20, 100);
        self.thickness = self.thickness.clamp(15, 60);
        self.gap = if self.gap.is_finite() { self.gap.clamp(0.0, 12.0) } else { 0.0 };
        self.icon_scale = self.icon_scale.clamp(60, 140);
        self.text_scale = self.text_scale.clamp(70, 140);
        self.tap_ms = self.tap_ms.clamp(100, 600);
        self.items.truncate(MAX_ITEMS);
        if self.center != "cursor" {
            self.center = "screen".into();
        }
        self
    }

    pub fn parse(text: &str) -> Result<Config, serde_json::Error> {
        // Older Notepad versions save UTF-8 with a BOM, which serde_json rejects.
        serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')).map(Config::sanitized)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ files

fn dir_from_env(var: &str, fallback_var: &str) -> PathBuf {
    std::env::var_os(var)
        .or_else(|| std::env::var_os(fallback_var))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("RadialLauncher")
}

/// Settings folder (`%APPDATA%\RadialLauncher`; override with `RADIAL_CONFIG_DIR`).
pub fn config_dir() -> PathBuf {
    match std::env::var_os("RADIAL_CONFIG_DIR") {
        Some(d) => PathBuf::from(d),
        None => dir_from_env("APPDATA", "HOME"),
    }
}

/// Cache folder for generated files such as the icon cache (`%LOCALAPPDATA%\RadialLauncher`).
pub fn cache_dir() -> PathBuf {
    match std::env::var_os("RADIAL_CACHE_DIR") {
        Some(d) => PathBuf::from(d),
        None => dir_from_env("LOCALAPPDATA", "HOME"),
    }
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
            let _ = save(&cfg);
            Ok(cfg)
        }
    }
}

/// Write the config atomically (temp file + rename) so a crash can never leave half a file.
pub fn save(cfg: &Config) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("config.json.tmp");
    std::fs::write(&tmp, cfg.to_json())?;
    std::fs::rename(&tmp, config_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_json() {
        let cfg = Config::default();
        assert_eq!(Config::parse(&cfg.to_json()).unwrap(), cfg);
    }

    #[test]
    fn partial_and_bom_files_are_accepted() {
        let cfg = Config::parse("\u{feff}{ \"size\": 600 }").unwrap();
        assert_eq!(cfg.size, 600);
        assert_eq!(cfg.items.len(), 8);
        assert!(!cfg.return_cursor);
    }

    #[test]
    fn version_1_files_are_migrated() {
        let old = r##"{ "size": 520, "center": "screen", "restore_cursor": true, "accent": "#FF8000", "items": [] }"##;
        let cfg = Config::parse(old).unwrap();
        assert_eq!(cfg.theme.hover, Color::rgb(255, 128, 0));
        assert!(!cfg.return_cursor, "old restore_cursor must not re-enable pointer restore");
        assert!(!cfg.to_json().contains("accent"));
    }

    #[test]
    fn values_are_clamped() {
        let cfg = Config::parse(r#"{ "size": 5000, "center": "x", "opacity": 0, "gap": 99, "tap_ms": 1 }"#).unwrap();
        assert_eq!((cfg.size, cfg.opacity, cfg.tap_ms), (900, 20, 100));
        assert_eq!(cfg.gap, 12.0);
        assert_eq!(cfg.center, "screen");
    }

    #[test]
    fn style_names_and_colours_in_json() {
        let cfg = Config::parse(r##"{ "style": "orbs", "theme": { "hover": "#00FF00" } }"##).unwrap();
        assert_eq!(cfg.style, Style::Orbs);
        assert_eq!(cfg.theme.hover, Color::rgb(0, 255, 0));
        assert_eq!(cfg.theme.background, Theme::midnight().background);
        assert!(Config::parse(r#"{ "style": "nope" }"#).is_err());
        assert!(Config::parse(r##"{ "theme": { "hover": "red" } }"##).is_err());
    }

    #[test]
    fn set_style_applies_preset() {
        let mut cfg = Config::default();
        cfg.set_style(Style::Disc);
        assert!(cfg.labels);
        assert_eq!(cfg.thickness, Style::Disc.preset().thickness);
    }

    #[test]
    fn env_expansion() {
        std::env::set_var("RL_TEST_VAR", "C:\\Tools");
        assert_eq!(expand_env("%RL_TEST_VAR%\\a.exe"), "C:\\Tools\\a.exe");
        assert_eq!(expand_env("100%"), "100%");
        assert_eq!(expand_env("%NO_SUCH_VAR_XYZ%\\a"), "%NO_SUCH_VAR_XYZ%\\a");
        assert_eq!(expand_env("a%%b"), "a%%b");
    }

    #[test]
    fn target_checks() {
        assert_eq!(ItemConfig::new("x", "https://example.com").target_exists(), None);
        assert_eq!(ItemConfig::new("x", SETTINGS_ACTION).target_exists(), None);
        assert_eq!(ItemConfig::new("x", "").target_exists(), Some(false));
        assert_eq!(ItemConfig::new("x", "/definitely/not/here.exe").target_exists(), Some(false));
        assert_eq!(ItemConfig::new("x", "/").target_exists(), Some(true));
        assert!(ItemConfig::new("x", SETTINGS_ACTION).builtin().is_some());
    }

    #[test]
    fn save_and_load_use_the_override_dir() {
        let dir = std::env::temp_dir().join(format!("rl-test-{}", std::process::id()));
        std::env::set_var("RADIAL_CONFIG_DIR", &dir);
        let mut cfg = Config::default();
        cfg.size = 640;
        cfg.theme.hover = Color::rgb(1, 2, 3);
        save(&cfg).unwrap();
        assert_eq!(load_or_create().unwrap(), cfg.clone().sanitized());
        std::fs::write(config_path(), "{ not json").unwrap();
        assert!(load_or_create().is_err());
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("RADIAL_CONFIG_DIR");
    }
}
