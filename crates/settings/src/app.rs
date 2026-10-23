//! The settings window: live wheel preview on the left, tabs on the right.

use crate::platform::{self, StartApp};
use eframe::egui::{self, Color32, TextureHandle, TextureOptions, Vec2};
use radial_core::color::Color;
use radial_core::config::{self, Config, ItemConfig, Style, Theme, MAX_ITEMS, SETTINGS_ACTION};
use radial_core::iconcache;
use radial_core::text::TextRenderer;
use radial_core::wheel::{pick_icons, render_preview, Layout};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use tiny_skia::Pixmap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Apps,
    Style,
    Behavior,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backdrop {
    Dark,
    Light,
    Checker,
}

enum Picker {
    Closed,
    Loading(Receiver<Result<Vec<StartApp>, String>>),
    Ready { apps: Vec<StartApp>, filter: String },
    Failed(String),
}

struct Status {
    text: String,
    error: bool,
}

#[derive(PartialEq)]
struct PreviewKey {
    cfg: Config,
    hovered: Option<usize>,
    px: u32,
    icon_version: u32,
}

#[derive(PartialEq)]
struct ThumbKey {
    theme: Theme,
    icon_scale: u8,
    icon_version: u32,
    items: usize,
}

pub struct SettingsApp {
    _session: platform::Session,
    tr: Box<dyn TextRenderer>,

    cfg: Config,
    saved: Config,
    tab: Tab,
    selected: usize,

    icons: HashMap<u64, Option<Pixmap>>,
    pending_icons: Vec<String>,
    icon_version: u32,
    icon_tex: HashMap<u64, TextureHandle>,

    preview: Option<TextureHandle>,
    preview_key: Option<PreviewKey>,
    thumbs: Vec<TextureHandle>,
    thumb_key: Option<ThumbKey>,
    backdrop: Backdrop,

    status: Option<Status>,
    website: String,
    picker: Picker,
    autostart: bool,
    frames: u32,
    shot: Option<String>,
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.0[0], c.0[1], c.0[2], c.0[3])
}

fn from_c32(c: Color32) -> Color {
    Color(c.to_srgba_unmultiplied())
}

fn name_from_path(path: &str) -> String {
    let p = path.trim().trim_end_matches(['\\', '/']);
    let tail = p.rsplit(['\\', '/']).next().unwrap_or(p);
    let stem = tail.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(tail);
    stem.to_owned()
}

impl SettingsApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Config, shot: Option<String>) -> Self {
        // Windows system fonts as fallback: non-Latin app names and symbols then display properly.
        let mut fonts = egui::FontDefinitions::default();
        for (name, data) in platform::system_fonts() {
            fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(data)));
            if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                list.push(name);
            }
        }
        cc.egui_ctx.set_fonts(fonts);

        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = c32(initial.theme.hover).gamma_multiply(0.85);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = Vec2::new(8.0, 7.0);
            s.spacing.button_padding = Vec2::new(10.0, 5.0);
        });

        let mut icons: HashMap<u64, Option<Pixmap>> = HashMap::new();
        if let Some(cached) = iconcache::read(&iconcache::cache_path()) {
            for (k, v) in cached {
                icons.insert(k, Some(v));
            }
        }
        let mut app = SettingsApp {
            _session: platform::Session::new(),
            tr: platform::text_renderer(),
            saved: initial.clone(),
            cfg: initial,
            tab: Tab::Apps,
            selected: 0,
            icons,
            pending_icons: Vec::new(),
            icon_version: 0,
            icon_tex: HashMap::new(),
            preview: None,
            preview_key: None,
            thumbs: Vec::new(),
            thumb_key: None,
            backdrop: Backdrop::Dark,
            status: None,
            website: String::new(),
            picker: Picker::Closed,
            autostart: platform::autostart_enabled(),
            frames: 0,
            shot,
        };
        if let Ok(t) = std::env::var("RADIAL_SETTINGS_TAB") {
            app.tab = match t.as_str() {
                "style" => Tab::Style,
                "behavior" => Tab::Behavior,
                _ => Tab::Apps,
            };
        }
        app.queue_missing_icons();
        app
    }

    fn dirty(&self) -> bool {
        self.cfg != self.saved
    }

    // ------------------------------------------------------------ icons

    fn queue_missing_icons(&mut self) {
        for it in &self.cfg.items {
            if it.builtin().is_some() || it.path.trim().is_empty() {
                continue;
            }
            let key = iconcache::key_for(&it.path);
            if !self.icons.contains_key(&key) && !self.pending_icons.contains(&it.path) {
                self.pending_icons.push(it.path.clone());
            }
        }
    }

    /// Extract at most one missing icon per frame so the window never freezes.
    fn load_one_icon(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.pending_icons.pop() {
            let key = iconcache::key_for(&path);
            if !self.icons.contains_key(&key) {
                let icon = platform::load_icon(&path);
                self.icons.insert(key, icon);
                self.icon_version += 1;
            }
            ctx.request_repaint();
        }
    }

    fn icon_texture(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureId> {
        let key = iconcache::key_for(path);
        if let Some(t) = self.icon_tex.get(&key) {
            return Some(t.id());
        }
        let pm = self.icons.get(&key)?.as_ref()?;
        let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
        let tex = ctx.load_texture(format!("icon{key}"), img, TextureOptions::LINEAR);
        let id = tex.id();
        self.icon_tex.insert(key, tex);
        Some(id)
    }

    fn gear_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        const KEY: u64 = 0;
        if let Some(t) = self.icon_tex.get(&KEY) {
            return t.id();
        }
        let pm = radial_core::text::gear_icon(Color::rgb(200, 205, 215));
        let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
        let tex = ctx.load_texture("gear", img, TextureOptions::LINEAR);
        let id = tex.id();
        self.icon_tex.insert(KEY, tex);
        id
    }

    // ------------------------------------------------------------ items

    fn add_item(&mut self, mut item: ItemConfig) {
        if self.cfg.items.len() >= MAX_ITEMS {
            self.set_status(format!("The wheel holds at most {MAX_ITEMS} items."), true);
            return;
        }
        if item.name.trim().is_empty() {
            item.name = name_from_path(&item.path);
        }
        self.cfg.items.push(item);
        self.selected = self.cfg.items.len() - 1;
        self.tab = Tab::Apps;
        self.queue_missing_icons();
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status { text: text.into(), error });
    }

    // ------------------------------------------------------------ saving

    fn save(&mut self) {
        let cfg = self.cfg.clone().sanitized();
        if let Err(e) = config::save(&cfg) {
            self.set_status(format!("Could not save: {e}"), true);
            return;
        }
        // Refresh the icon cache from what we already extracted, so the launcher needs no shell code.
        let mut entries: Vec<(u64, &Pixmap)> = Vec::new();
        for it in &cfg.items {
            if it.builtin().is_some() {
                continue;
            }
            let key = iconcache::key_for(&it.path);
            if entries.iter().any(|(k, _)| *k == key) {
                continue;
            }
            if let Some(Some(pm)) = self.icons.get(&key) {
                entries.push((key, pm));
            }
        }
        let cache_ok = iconcache::write(&iconcache::cache_path(), &entries).is_ok();

        self.saved = cfg.clone();
        self.cfg = cfg;
        let running = platform::notify_launcher();
        let msg = match (running, cache_ok) {
            (true, true) => "Saved. The launcher is using the new settings.".to_owned(),
            (true, false) => "Saved, but the icon cache could not be written (letter icons will be used).".to_owned(),
            (false, _) => "Saved. The launcher is not running: start it to use the new settings.".to_owned(),
        };
        self.set_status(msg, !cache_ok);
    }

    // ------------------------------------------------------------ preview

    fn backdrop_paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        let p = ui.painter_at(rect);
        match self.backdrop {
            Backdrop::Dark => {
                p.rect_filled(rect, 6.0, Color32::from_rgb(44, 48, 58));
            }
            Backdrop::Light => {
                p.rect_filled(rect, 6.0, Color32::from_rgb(222, 226, 233));
            }
            Backdrop::Checker => {
                p.rect_filled(rect, 6.0, Color32::from_rgb(96, 104, 120));
                let tile = 22.0;
                let (cols, rows) = ((rect.width() / tile).ceil() as i32, (rect.height() / tile).ceil() as i32);
                for r in 0..rows {
                    for c in 0..cols {
                        if (r + c) % 2 == 0 {
                            let min = rect.min + Vec2::new(c as f32 * tile, r as f32 * tile);
                            p.rect_filled(egui::Rect::from_min_size(min, Vec2::splat(tile)).intersect(rect), 0.0, Color32::from_rgb(120, 128, 144));
                        }
                    }
                }
            }
        }
    }

    fn ui_preview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Preview on:");
            ui.selectable_value(&mut self.backdrop, Backdrop::Dark, "Dark");
            ui.selectable_value(&mut self.backdrop, Backdrop::Light, "Light");
            ui.selectable_value(&mut self.backdrop, Backdrop::Checker, "Pattern");
        });

        let avail = ui.available_size();
        let area = Vec2::new(avail.x.max(100.0), (avail.y - 26.0).max(100.0));
        let (outer, _) = ui.allocate_exact_size(area, egui::Sense::hover());
        self.backdrop_paint(ui, outer);

        let side = (self.cfg.size as f32).min(outer.width() - 16.0).min(outer.height() - 16.0).max(80.0);
        let rect = egui::Rect::from_center_size(outer.center(), Vec2::splat(side));
        let resp = ui.interact(rect, ui.make_persistent_id("wheel_preview"), egui::Sense::click());

        let ppp = ui.ctx().pixels_per_point();
        let px = (side * ppp).round().max(64.0) as u32;
        let n = self.cfg.items.len();

        // Which slice is under the pointer? Moving over the middle shows the idle look.
        let layout = Layout::new(px, self.cfg.style, self.cfg.thickness);
        let pointer = resp.hover_pos();
        let hit = pointer.and_then(|p| {
            let d = (p - rect.center()) * ppp;
            layout.hit_test(d.x, d.y, n)
        });
        let hovered = if pointer.is_some() { hit } else { (self.selected < n).then_some(self.selected) };
        if resp.clicked() {
            if let Some(i) = hit {
                self.selected = i;
                self.tab = Tab::Apps;
            }
        }

        let key = PreviewKey { cfg: self.cfg.clone(), hovered, px, icon_version: self.icon_version };
        if self.preview_key.as_ref() != Some(&key) || self.preview.is_none() {
            let icons_map = &self.icons;
            let icons = pick_icons(&self.cfg, &mut *self.tr, |it| icons_map.get(&iconcache::key_for(&it.path)).cloned().flatten());
            let pm = render_preview(&self.cfg, px, icons, hovered, &mut *self.tr);
            let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
            match self.preview.as_mut() {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.preview = Some(ui.ctx().load_texture("preview", img, TextureOptions::LINEAR)),
            }
            self.preview_key = Some(key);
        }
        if let Some(tex) = &self.preview {
            let alpha = (self.cfg.opacity as f32 * 2.55).round() as u8;
            ui.painter_at(outer).image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::from_white_alpha(alpha),
            );
        }
        ui.label(egui::RichText::new("Hover the preview to try it. Click a slice to edit that app.").weak().small());
    }

    // ------------------------------------------------------------ tabs

    fn ui_apps(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let n = self.cfg.items.len();
        ui.label(format!("{n} of {MAX_ITEMS} slots used. Slices run clockwise from the top."));

        let mut swap: Option<(usize, usize)> = None;
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical().max_height(250.0).auto_shrink([false, true]).show(ui, |ui| {
            for i in 0..n {
                let (name, path, is_builtin) = {
                    let it = &self.cfg.items[i];
                    (it.name.clone(), it.path.clone(), it.builtin().is_some())
                };
                let tex = if is_builtin { Some(self.gear_texture(&ctx)) } else { self.icon_texture(&ctx, &path) };
                ui.horizontal(|ui| {
                    match tex {
                        Some(id) => {
                            ui.image(egui::load::SizedTexture::new(id, Vec2::splat(22.0)));
                        }
                        None => {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 8.0, Color32::from_gray(90));
                        }
                    }
                    let label = if name.trim().is_empty() { "(unnamed)".to_owned() } else { name };
                    if ui.selectable_label(self.selected == i, label).clicked() {
                        self.selected = i;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            remove = Some(i);
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("Down").small()).on_hover_text("Move later").clicked() {
                            swap = Some((i, i + 1));
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Up").small()).on_hover_text("Move earlier").clicked() {
                            swap = Some((i, i - 1));
                        }
                    });
                });
            }
        });
        if let Some((a, b)) = swap {
            self.cfg.items.swap(a, b);
            self.selected = b;
        }
        if let Some(i) = remove {
            self.cfg.items.remove(i);
            if self.selected >= self.cfg.items.len() {
                self.selected = self.cfg.items.len().saturating_sub(1);
            }
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ App or file…").clicked() {
                if let Some(p) = platform::pick_file() {
                    self.add_item(ItemConfig::new("", &p));
                }
            }
            if ui.button("+ Folder…").clicked() {
                if let Some(p) = platform::pick_folder() {
                    self.add_item(ItemConfig::new("", &p));
                }
            }
            if ui.button("+ From Start menu…").clicked() {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(platform::list_start_apps());
                });
                self.picker = Picker::Loading(rx);
            }
            if ui.button("+ Settings button").clicked() {
                self.add_item(ItemConfig {
                    name: "Radial Settings".into(),
                    description: "Change the look and the apps".into(),
                    path: SETTINGS_ACTION.into(),
                    args: String::new(),
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Website:");
            let r = ui.add(egui::TextEdit::singleline(&mut self.website).hint_text("https://…").desired_width(190.0));
            let go = ui.button("Add").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go && !self.website.trim().is_empty() {
                let mut url = self.website.trim().to_owned();
                if !url.contains("://") {
                    url = format!("https://{url}");
                }
                let host = url.split("://").nth(1).unwrap_or(&url).trim_start_matches("www.").split('/').next().unwrap_or("").to_owned();
                self.add_item(ItemConfig::new(&host, &url));
                self.website.clear();
            }
        });

        ui.separator();
        let sel = self.selected;
        let Some(item) = self.cfg.items.get_mut(sel) else {
            ui.label("Add an app to get started.");
            return;
        };
        let before = item.path.clone();
        egui::Grid::new("item_editor").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut item.name).desired_width(280.0));
            ui.end_row();
            ui.label("Description");
            ui.add(egui::TextEdit::singleline(&mut item.description).hint_text("optional second line").desired_width(280.0));
            ui.end_row();
            if item.builtin().is_none() {
                ui.label("Opens");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut item.path).desired_width(212.0));
                    if ui.button("Browse…").clicked() {
                        if let Some(p) = platform::pick_file() {
                            item.path = p;
                        }
                    }
                });
                ui.end_row();
                ui.label("Arguments");
                ui.add(egui::TextEdit::singleline(&mut item.args).hint_text("optional").desired_width(280.0));
                ui.end_row();
            } else {
                ui.label("Action");
                ui.label("Opens this settings window");
                ui.end_row();
            }
        });
        match item.target_exists() {
            Some(false) => {
                ui.colored_label(Color32::from_rgb(240, 170, 70), "Not found: check that this path exists on this PC.");
            }
            Some(true) => {
                ui.colored_label(Color32::from_rgb(110, 200, 130), "Found");
            }
            None => {}
        }
        if item.path != before {
            self.queue_missing_icons();
        }
    }

    fn ui_style(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.ensure_thumbs(&ctx);

        ui.label(egui::RichText::new("Design").strong());
        let current = self.cfg.style;
        let mut chosen = None;
        egui::Grid::new("designs").num_columns(3).spacing([8.0, 8.0]).show(ui, |ui| {
            for (i, style) in Style::ALL.into_iter().enumerate() {
                let selected = current == style;
                ui.vertical(|ui| {
                    if let Some(t) = self.thumbs.get(i) {
                        let size = Vec2::splat(104.0);
                        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                        let fill = if selected { Color32::from_rgb(52, 64, 88) } else { Color32::from_rgb(34, 37, 44) };
                        ui.painter().rect_filled(rect, 8.0, fill);
                        if selected {
                            ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(2.0, Color32::from_rgb(110, 170, 255)), egui::StrokeKind::Inside);
                        }
                        ui.painter().image(
                            t.id(),
                            rect.shrink(4.0),
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        if resp.on_hover_text(style.blurb()).clicked() {
                            chosen = Some(style);
                        }
                    }
                    ui.add_sized([104.0, 16.0], egui::Label::new(egui::RichText::new(style.label()).strong()));
                });
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        if let Some(s) = chosen {
            self.cfg.set_style(s);
        }

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Colour theme").strong());
        ui.horizontal_wrapped(|ui| {
            for (name, theme) in Theme::presets() {
                let swatch = egui::RichText::new(name).color(c32(theme.hover));
                if ui.button(swatch).clicked() {
                    self.cfg.theme = theme;
                }
            }
        });

        let theme = &mut self.cfg.theme;
        egui::Grid::new("colours").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, label: &str, value: &mut Color| {
                ui.label(label);
                let mut c = c32(*value);
                if ui.color_edit_button_srgba(&mut c).changed() {
                    *value = from_c32(c);
                }
                ui.end_row();
            };
            row(ui, "Background (slices / ring)", &mut theme.background);
            row(ui, "Centre hub", &mut theme.hub);
            row(ui, "Highlight (hovered slice)", &mut theme.hover);
            row(ui, "Text on slices", &mut theme.text);
            row(ui, "Text in the hub", &mut theme.hub_text);
            row(ui, "Outlines and lines", &mut theme.border);
        });

        ui.add_space(4.0);
        let style = self.cfg.style;
        egui::Grid::new("sliders").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            ui.label("Size");
            ui.add(egui::Slider::new(&mut self.cfg.size, 300..=900).suffix(" px"));
            ui.end_row();
            ui.label("Opacity");
            ui.add(egui::Slider::new(&mut self.cfg.opacity, 20..=100).suffix(" %"));
            ui.end_row();
            ui.label(style.size_knob());
            ui.add(egui::Slider::new(&mut self.cfg.thickness, 15..=60).suffix(" %"));
            ui.end_row();
            if style != Style::Orbs && style != Style::Disc && style != Style::Band {
                ui.label("Gap between slices");
                ui.add(egui::Slider::new(&mut self.cfg.gap, 0.0..=12.0).suffix("°"));
                ui.end_row();
            }
            ui.label("Icon size");
            ui.add(egui::Slider::new(&mut self.cfg.icon_scale, 60..=140).suffix(" %"));
            ui.end_row();
            ui.label("Text size");
            ui.add(egui::Slider::new(&mut self.cfg.text_scale, 70..=140).suffix(" %"));
            ui.end_row();
        });
        match style {
            Style::Disc => {
                ui.add_enabled(false, egui::Checkbox::new(&mut true, "Show names on slices (always on for Disc)"));
            }
            Style::Orbs | Style::Halo => {
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "Show names on slices (name appears in the hub)"));
            }
            _ => {
                ui.checkbox(&mut self.cfg.labels, "Show names on slices");
            }
        }
        ui.checkbox(&mut self.cfg.animation, "Fade the wheel in");
    }

    fn ensure_thumbs(&mut self, ctx: &egui::Context) {
        let key = ThumbKey {
            theme: self.cfg.theme,
            icon_scale: self.cfg.icon_scale,
            icon_version: self.icon_version,
            items: self.cfg.items.len(),
        };
        if self.thumb_key.as_ref() == Some(&key) && self.thumbs.len() == Style::ALL.len() {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let px = (96.0 * ppp).round() as u32;
        let mut out = Vec::new();
        for (i, style) in Style::ALL.into_iter().enumerate() {
            let mut c = self.cfg.clone();
            c.set_style(style);
            c.items.truncate(8);
            let icons_map = &self.icons;
            let icons = pick_icons(&c, &mut *self.tr, |it| icons_map.get(&iconcache::key_for(&it.path)).cloned().flatten());
            let hov = if c.items.len() > 2 { Some(2) } else { None };
            let pm = render_preview(&c, px, icons, hov, &mut *self.tr);
            let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
            match self.thumbs.get_mut(i) {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => out.push(ctx.load_texture(format!("thumb{i}"), img, TextureOptions::LINEAR)),
            }
        }
        self.thumbs.extend(out);
        self.thumb_key = Some(key);
    }

    fn ui_behavior(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Opening").strong());
        ui.label("Hold Shift + the middle mouse button to open the wheel. Move toward a slice and release to open it. A quick tap keeps the wheel open so you can click a slice; right-click or Esc cancels.");
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Open the wheel");
            ui.selectable_value(&mut self.cfg.center, "screen".to_owned(), "in the centre of the screen");
            ui.selectable_value(&mut self.cfg.center, "cursor".to_owned(), "under the pointer");
        });
        ui.add_enabled(
            self.cfg.center == "screen",
            egui::Checkbox::new(&mut self.cfg.return_cursor, "Move the pointer back to where it was when the wheel closes"),
        );
        if self.cfg.center == "screen" && !self.cfg.return_cursor {
            ui.label(egui::RichText::new("The pointer stays where you released it.").weak().small());
        }
        egui::Grid::new("behavior_sliders").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            ui.label("Quick-tap time");
            ui.add(egui::Slider::new(&mut self.cfg.tap_ms, 100..=600).suffix(" ms"));
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.label(egui::RichText::new("After choosing").strong());
        ui.checkbox(&mut self.cfg.focus_launched, "Bring the opened app to the front");
        ui.label(egui::RichText::new("Fixes apps that open behind other windows.").weak().small());

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Windows").strong());
        let mut auto = self.autostart;
        if ui.checkbox(&mut auto, "Start Radial Launcher when I sign in to Windows").changed() {
            let exe = platform::launcher_exe().or_else(|| std::env::current_exe().ok()).unwrap_or_default();
            if platform::set_autostart(auto, &exe) {
                self.autostart = auto;
            } else {
                self.set_status("Could not change the startup setting.", true);
            }
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Reset everything to defaults").clicked() {
                self.cfg = Config::default();
                self.selected = 0;
                self.queue_missing_icons();
            }
            if ui.button("Open config folder").clicked() && cfg!(windows) {
                let _ = std::process::Command::new("explorer").arg(config::config_dir()).spawn();
            }
            if ui.button("Start launcher").clicked() {
                let ok = platform::start_launcher();
                self.set_status(if ok { "Launcher started." } else { "radial-launcher.exe was not found next to this app." }, !ok);
            }
        });
    }

    // ------------------------------------------------------------ start menu picker

    fn ui_picker(&mut self, ctx: &egui::Context) {
        if let Picker::Loading(rx) = &self.picker {
            match rx.try_recv() {
                Ok(Ok(apps)) => self.picker = Picker::Ready { apps, filter: String::new() },
                Ok(Err(e)) => self.picker = Picker::Failed(e),
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => self.picker = Picker::Failed("The list could not be loaded.".into()),
            }
        }
        if matches!(self.picker, Picker::Closed) {
            return;
        }

        let mut open = true;
        let mut to_add: Option<StartApp> = None;
        egui::Window::new("Add an app from the Start menu")
            .open(&mut open)
            .collapsible(false)
            .default_size([380.0, 440.0])
            .show(ctx, |ui| match &mut self.picker {
                Picker::Loading(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking up installed apps…");
                    });
                }
                Picker::Failed(e) => {
                    ui.colored_label(Color32::from_rgb(240, 120, 110), e.as_str());
                }
                Picker::Ready { apps, filter } => {
                    ui.add(egui::TextEdit::singleline(filter).hint_text("Search…").desired_width(f32::INFINITY));
                    let needle = filter.to_lowercase();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for app in apps.iter().filter(|a| needle.is_empty() || a.name.to_lowercase().contains(&needle)) {
                            if ui.add(egui::Button::new(&app.name).frame(false).min_size(Vec2::new(ui.available_width(), 24.0))).clicked() {
                                to_add = Some(app.clone());
                            }
                        }
                    });
                }
                Picker::Closed => {}
            });
        if let Some(app) = to_add {
            self.add_item(ItemConfig::new(&app.name, &app.path));
            self.picker = Picker::Closed;
        } else if !open {
            self.picker = Picker::Closed;
        }
    }

    // ------------------------------------------------------------ development screenshot

    #[cfg(feature = "fontdue-text")]
    fn screenshot_hook(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        if self.frames == 20 {
            let cb = egui::ScreenshotCallback::new(move |img| {
                if let Some(mut pm) = Pixmap::new(img.width() as u32, img.height() as u32) {
                    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(img.pixels.iter()) {
                        d.copy_from_slice(&[s.r(), s.g(), s.b(), 255]);
                    }
                    let _ = pm.save_png(&path);
                }
                std::process::exit(0);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
        ctx.request_repaint();
    }

    #[cfg(not(feature = "fontdue-text"))]
    fn screenshot_hook(&mut self, _ctx: &egui::Context) {
        let _ = &self.shot;
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        if self.frames == 3 {
            platform::raise_self();
        }
        self.load_one_icon(&ctx);

        egui::Panel::bottom("save_bar").show(ui, |ui| {
            ui.add_space(4.0);
            let dirty = self.dirty();
            ui.horizontal(|ui| {
                if ui.add_enabled(dirty, egui::Button::new(egui::RichText::new("Save").strong())).clicked() {
                    self.save();
                }
                if ui.add_enabled(dirty, egui::Button::new("Undo changes")).clicked() {
                    self.cfg = self.saved.clone();
                    self.selected = self.selected.min(self.cfg.items.len().saturating_sub(1));
                    self.queue_missing_icons();
                }
                if dirty {
                    ui.colored_label(Color32::from_rgb(240, 190, 80), "Unsaved changes");
                } else if let Some(s) = &self.status {
                    let color = if s.error { Color32::from_rgb(240, 130, 110) } else { Color32::from_rgb(120, 205, 140) };
                    ui.colored_label(color, &s.text);
                }
            });
            if dirty {
                if let Some(s) = self.status.as_ref().filter(|s| s.error) {
                    ui.colored_label(Color32::from_rgb(240, 130, 110), &s.text);
                }
            }
            ui.add_space(4.0);
        });

        egui::Panel::right("controls").exact_size(430.0).resizable(false).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Apps, "Apps");
                ui.selectable_value(&mut self.tab, Tab::Style, "Style");
                ui.selectable_value(&mut self.tab, Tab::Behavior, "Behavior");
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Apps => self.ui_apps(ui),
                Tab::Style => self.ui_style(ui),
                Tab::Behavior => self.ui_behavior(ui),
            });
        });

        egui::CentralPanel::default().show(ui, |ui| self.ui_preview(ui));

        self.ui_picker(&ctx);
        self.queue_missing_icons();
        self.screenshot_hook(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_derived_from_paths() {
        assert_eq!(name_from_path("C:\\Program Files\\Mozilla Firefox\\firefox.exe"), "firefox");
        assert_eq!(name_from_path("C:\\Users\\Me\\Documents\\"), "Documents");
        assert_eq!(name_from_path("D:\\tools\\app.v2.exe"), "app.v2");
        assert_eq!(name_from_path("/usr/bin/tool"), "tool");
        assert_eq!(name_from_path(".hidden"), ".hidden");
        assert_eq!(name_from_path(""), "");
    }

    #[test]
    fn colour_conversion_round_trips_opaque_colours() {
        for c in [Color::rgb(0, 0, 0), Color::rgb(255, 255, 255), Color::rgb(76, 154, 255), Color::rgb(1, 2, 3)] {
            assert_eq!(from_c32(c32(c)), c);
        }
    }
}
