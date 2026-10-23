//! The settings window: live wheel preview on the left, tabs on the right.

use crate::platform::{self, StartApp};
use eframe::egui::{self, Color32, TextureHandle, TextureOptions, Vec2};
use radial_core::color::Color;
use radial_core::config::{self, Button, Config, ItemConfig, Modifier, Style, Theme, DEFAULT_PAGE_SIZE, SETTINGS_ACTION};
use radial_core::iconcache;
use radial_core::text::TextRenderer;
use radial_core::wheel::{pick_icons, render_preview, render_preview_page, Layout};
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
    page: usize,
}

#[derive(PartialEq)]
struct ThumbKey {
    theme: Theme,
    icon_scale: u8,
    icon_version: u32,
    items: usize,
    font: String,
}

/// An icon waiting to be extracted (one per frame, so the window never freezes).
struct PendingIcon {
    key: u64,
    item: ItemConfig,
}

pub struct SettingsApp {
    _session: platform::Session,
    tr: Box<dyn TextRenderer>,

    cfg: Config,
    saved: Config,
    tab: Tab,
    selected: usize,

    icons: HashMap<u64, Option<Pixmap>>,
    pending_icons: Vec<PendingIcon>,
    icon_version: u32,
    icon_tex: HashMap<u64, TextureHandle>,

    preview: Option<TextureHandle>,
    preview_key: Option<PreviewKey>,
    thumbs: Vec<TextureHandle>,
    thumb_key: Option<ThumbKey>,
    backdrop: Backdrop,
    preview_page: usize,
    last_selected: usize,
    list_filter: String,

    font_face: String,
    fonts: Option<Vec<String>>,
    /// Search text of the open font picker (`None` = closed).
    font_picker: Option<String>,
    applied_look: Option<(Theme, bool)>,
    dark: bool,

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

/// egui colours for the settings window, taken from the wheel's theme (or the default dark look).
/// Returns the visuals and whether they are dark.
fn themed_visuals(theme: &Theme, follow: bool) -> (egui::Visuals, bool) {
    let t = if follow { *theme } else { Theme::midnight() };
    let bg = t.background.with_alpha(255);
    let mut fg = t.text.with_alpha(255);
    let accent = t.hover.with_alpha(255);
    let dark = bg.luma() < 128.0;
    // Never let a clashing "text on slices" colour make the window unreadable.
    if (bg.luma() - fg.luma()).abs() < 90.0 {
        fg = if dark { Color::rgb(235, 238, 245) } else { Color::rgb(28, 32, 42) };
    }
    let on_accent = if accent.luma() < 140.0 { Color::rgb(255, 255, 255) } else { Color::rgb(20, 24, 32) };

    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let stroke = |c: Color, w: f32| egui::Stroke::new(w, c32(c));
    v.panel_fill = c32(bg);
    v.window_fill = c32(bg.shade(if dark { 0.05 } else { 0.5 }));
    v.extreme_bg_color = c32(if dark { bg.shade(-0.4) } else { bg.shade(0.6) });
    v.faint_bg_color = c32(bg.mix(fg, 0.05));
    v.hyperlink_color = c32(accent);
    v.selection.bg_fill = c32(accent);
    v.selection.stroke = stroke(on_accent, 1.0);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c32(bg);
    w.noninteractive.weak_bg_fill = c32(bg);
    w.noninteractive.bg_stroke = stroke(bg.mix(fg, 0.16), 1.0);
    w.noninteractive.fg_stroke = stroke(fg, 1.0);
    for (state, k) in [(&mut w.inactive, 0.10), (&mut w.hovered, 0.20), (&mut w.active, 0.30), (&mut w.open, 0.14)] {
        let fill = c32(bg.mix(fg, k));
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = stroke(bg.mix(fg, k + 0.12), 1.0);
        state.fg_stroke = stroke(fg, 1.0);
    }
    // `strong` headings use the "active" text colour: make them stand out a little.
    w.active.fg_stroke = stroke(fg.shade(if dark { 0.3 } else { -0.3 }), 1.0);
    (v, dark)
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

        let (visuals, dark) = themed_visuals(&initial.theme, initial.settings_follow_theme);
        let applied_look = Some((initial.theme, initial.settings_follow_theme));
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
        #[allow(unused_mut)]
        let mut initial = initial;
        #[cfg(feature = "fontdue-text")]
        if let Some(n) = std::env::var("RADIAL_SETTINGS_DEMO_ITEMS").ok().and_then(|v| v.parse::<usize>().ok()) {
            // Development aid: fill the wheel with many dummy apps to check the layout.
            initial.items = (0..n).map(|i| ItemConfig::new(&format!("Demo app {}", i + 1), &format!("C:\\demo\\app{i}.exe"))).collect();
        }
        let mut app = SettingsApp {
            _session: platform::Session::new(),
            tr: platform::text_renderer(&initial.font),
            font_face: initial.font.clone(),
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
            preview_page: 0,
            last_selected: 0,
            list_filter: String::new(),
            fonts: None,
            font_picker: None,
            applied_look,
            dark,
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
            let Some(key) = iconcache::item_key(it) else { continue };
            if !self.icons.contains_key(&key) && !self.pending_icons.iter().any(|p| p.key == key) {
                self.pending_icons.push(PendingIcon { key, item: it.clone() });
            }
        }
    }

    /// Extract at most one missing icon per frame so the window never freezes.
    fn load_one_icon(&mut self, ctx: &egui::Context) {
        if !self.pending_icons.is_empty() {
            let p = self.pending_icons.remove(0); // list order: what is on screen first
            if !self.icons.contains_key(&p.key) {
                let icon = platform::load_icon(&p.item);
                self.icons.insert(p.key, icon);
                self.icon_version += 1;
            }
            ctx.request_repaint();
        }
    }

    fn icon_texture(&mut self, ctx: &egui::Context, key: u64) -> Option<egui::TextureId> {
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
        if item.name.trim().is_empty() {
            item.name = name_from_path(&item.path);
        }
        self.cfg.items.push(item);
        self.selected = self.cfg.items.len() - 1;
        self.tab = Tab::Apps;
        self.queue_missing_icons();
    }

    fn warn(&self) -> Color32 {
        if self.dark { Color32::from_rgb(240, 170, 70) } else { Color32::from_rgb(168, 92, 0) }
    }

    fn good(&self) -> Color32 {
        if self.dark { Color32::from_rgb(110, 200, 130) } else { Color32::from_rgb(22, 118, 58) }
    }

    fn bad(&self) -> Color32 {
        if self.dark { Color32::from_rgb(240, 130, 110) } else { Color32::from_rgb(190, 48, 38) }
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
            let Some(key) = iconcache::item_key(it) else { continue };
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
        // The preview shows the page that holds the selected app, until the user flips pages.
        let pages = self.cfg.page_count();
        if self.selected != self.last_selected {
            self.preview_page = self.cfg.page_of(self.selected);
            self.last_selected = self.selected;
        }
        self.preview_page = self.preview_page.min(pages - 1);

        ui.horizontal(|ui| {
            ui.label("Preview on:");
            ui.selectable_value(&mut self.backdrop, Backdrop::Dark, "Dark");
            ui.selectable_value(&mut self.backdrop, Backdrop::Light, "Light");
            ui.selectable_value(&mut self.backdrop, Backdrop::Checker, "Pattern");
            if pages > 1 {
                ui.separator();
                if ui.small_button("<").on_hover_text("Previous page").clicked() {
                    self.preview_page = (self.preview_page + pages - 1) % pages;
                }
                ui.label(format!("Page {} of {}", self.preview_page + 1, pages));
                if ui.small_button(">").on_hover_text("Next page").clicked() {
                    self.preview_page = (self.preview_page + 1) % pages;
                }
            }
        });

        let page = self.preview_page;
        let range = self.cfg.page_range(page);
        let n = range.len();

        let avail = ui.available_size();
        let area = Vec2::new(avail.x.max(100.0), (avail.y - 26.0).max(100.0));
        let (outer, _) = ui.allocate_exact_size(area, egui::Sense::hover());
        self.backdrop_paint(ui, outer);

        let side = (self.cfg.size as f32).min(outer.width() - 16.0).min(outer.height() - 16.0).max(80.0);
        let rect = egui::Rect::from_center_size(outer.center(), Vec2::splat(side));
        let resp = ui.interact(rect, ui.make_persistent_id("wheel_preview"), egui::Sense::click());

        let ppp = ui.ctx().pixels_per_point();
        let px = (side * ppp).round().max(64.0) as u32;

        // Which slice is under the pointer? Moving over the middle shows the idle look.
        let layout = Layout::new(px, self.cfg.style, self.cfg.thickness);
        let pointer = resp.hover_pos();
        let hit = pointer.and_then(|p| {
            let d = (p - rect.center()) * ppp;
            layout.hit_test(d.x, d.y, n)
        });
        let hovered = if pointer.is_some() { hit } else { self.selected.checked_sub(range.start).filter(|&i| i < n) };
        if resp.clicked() {
            if let Some(i) = hit {
                self.selected = range.start + i;
                self.last_selected = self.selected;
                self.tab = Tab::Apps;
            }
        }

        let key = PreviewKey { cfg: self.cfg.clone(), hovered, px, icon_version: self.icon_version, page };
        if self.preview_key.as_ref() != Some(&key) || self.preview.is_none() {
            let page_cfg = self.cfg.page_config(page);
            let icons_map = &self.icons;
            let icons = pick_icons(&page_cfg, &mut *self.tr, |it| {
                iconcache::item_key(it).and_then(|k| icons_map.get(&k).cloned().flatten())
            });
            let pm = render_preview_page(&page_cfg, px, icons, hovered, page, pages, &mut *self.tr);
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
        let pages = self.cfg.page_count();
        ui.label(format!("{n} {}. Slices run clockwise from the top.", if n == 1 { "app" } else { "apps" }));
        if pages > 1 {
            ui.label(
                egui::RichText::new(format!(
                    "{} per wheel, {pages} pages. Scroll the mouse wheel while the wheel is open to flip pages.",
                    self.cfg.page_size
                ))
                .weak()
                .small(),
            );
        }
        if n > 8 {
            ui.add(egui::TextEdit::singleline(&mut self.list_filter).hint_text("Search the list…").desired_width(f32::INFINITY));
        } else {
            self.list_filter.clear();
        }
        let needle = self.list_filter.trim().to_lowercase();
        let filtering = !needle.is_empty();
        let shown: Vec<usize> =
            (0..n).filter(|&i| !filtering || self.cfg.items[i].name.to_lowercase().contains(&needle)).collect();

        let mut swap: Option<(usize, usize)> = None;
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical().max_height(250.0).auto_shrink([false, true]).show_rows(ui, 26.0, shown.len(), |ui, range| {
            for &i in &shown[range] {
                let (name, is_builtin, key) = {
                    let it = &self.cfg.items[i];
                    (it.name.clone(), it.builtin().is_some(), iconcache::item_key(it))
                };
                let tex = key.and_then(|k| self.icon_texture(&ctx, k)).or_else(|| is_builtin.then(|| self.gear_texture(&ctx)));
                ui.horizontal(|ui| {
                    match tex {
                        Some(id) => {
                            ui.image(egui::load::SizedTexture::new(id, Vec2::splat(22.0)));
                        }
                        None => {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 8.0, Color32::from_gray(110));
                        }
                    }
                    let label = if name.trim().is_empty() { "(unnamed)".to_owned() } else { name };
                    if ui.selectable_label(self.selected == i, label).clicked() {
                        self.selected = i;
                    }
                    if pages > 1 {
                        ui.label(egui::RichText::new(format!("p{}", self.cfg.page_of(i) + 1)).weak().small());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            remove = Some(i);
                        }
                        if ui.add_enabled(!filtering && i + 1 < n, egui::Button::new("Down").small()).on_hover_text("Move later").clicked() {
                            swap = Some((i, i + 1));
                        }
                        if ui.add_enabled(!filtering && i > 0, egui::Button::new("Up").small()).on_hover_text("Move earlier").clicked() {
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
                    ..Default::default()
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
        ui.horizontal(|ui| {
            if ui.add_enabled(self.selected < n, egui::Button::new("Duplicate selected")).clicked() {
                let mut copy = self.cfg.items[self.selected].clone();
                copy.name = format!("{} copy", copy.name);
                self.cfg.items.insert(self.selected + 1, copy);
                self.selected += 1;
            }
            if ui.add_enabled(n > 1, egui::Button::new("Sort A-Z")).clicked() {
                self.cfg.items.sort_by_key(|it| it.name.to_lowercase());
                self.selected = 0;
            }
        });

        ui.separator();
        let sel = self.selected;
        if sel >= self.cfg.items.len() {
            ui.label("Add an app to get started.");
            return;
        }
        let (key, is_builtin) = {
            let it = &self.cfg.items[sel];
            (iconcache::item_key(it), it.builtin().is_some())
        };
        let tex = key.and_then(|k| self.icon_texture(&ctx, k)).or_else(|| is_builtin.then(|| self.gear_texture(&ctx)));
        let (warn, good) = (self.warn(), self.good());
        let mut refresh_icon = false;
        let item = &mut self.cfg.items[sel];
        let before = (item.path.clone(), item.icon.clone());
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
                ui.label("Administrator");
                ui.checkbox(&mut item.admin, "Run as administrator (asks for permission)");
                ui.end_row();
            } else {
                ui.label("Action");
                ui.label("Opens this settings window");
                ui.end_row();
            }
            ui.label("Icon");
            ui.horizontal_wrapped(|ui| {
                match tex {
                    Some(id) => {
                        ui.image(egui::load::SizedTexture::new(id, Vec2::splat(32.0)));
                    }
                    None => {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 12.0, Color32::from_gray(110));
                    }
                }
                if ui.button("Choose picture…").on_hover_text("Use your own image (PNG, ICO, JPG...) instead of the app's icon").clicked() {
                    if let Some(p) = platform::pick_image() {
                        item.icon = p;
                    }
                }
                if item.has_custom_icon() && ui.button("Use app icon").clicked() {
                    item.icon.clear();
                }
                if ui.small_button("Refresh").on_hover_text("Read the icon again").clicked() {
                    refresh_icon = true;
                }
            });
            ui.end_row();
        });
        if item.has_custom_icon() {
            ui.label(egui::RichText::new(&item.icon).weak().small());
            if !std::path::Path::new(&config::expand_env(item.icon.trim())).exists() {
                ui.colored_label(warn, "Picture not found: the app's usual icon is shown instead.");
            }
        }
        match item.target_exists() {
            Some(false) => {
                ui.colored_label(warn, "Not found: check that this path exists on this PC.");
            }
            Some(true) => {
                ui.colored_label(good, "Found");
            }
            None => {}
        }
        let changed = (item.path.clone(), item.icon.clone()) != before;
        if refresh_icon {
            if let Some(k) = key {
                self.icons.remove(&k);
                self.icon_tex.remove(&k);
                self.icon_version += 1;
            }
        }
        if changed || refresh_icon {
            self.queue_missing_icons();
        }
    }

    fn ui_style(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.ensure_thumbs(&ctx);

        ui.label(egui::RichText::new("Design").strong());
        let current = self.cfg.style;
        let frame_idle = ui.visuals().extreme_bg_color;
        let frame_selected = ui.visuals().selection.bg_fill;
        let mut chosen = None;
        egui::Grid::new("designs").num_columns(3).spacing([8.0, 8.0]).show(ui, |ui| {
            for (i, style) in Style::ALL.into_iter().enumerate() {
                let selected = current == style;
                ui.vertical(|ui| {
                    if let Some(t) = self.thumbs.get(i) {
                        let size = Vec2::splat(104.0);
                        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                        ui.painter().rect_filled(rect, 8.0, frame_idle);
                        if selected {
                            ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(2.5, frame_selected), egui::StrokeKind::Inside);
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

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Font").strong());
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.cfg.font).hint_text("Segoe UI (default)").desired_width(190.0));
            if ui.button("Choose…").clicked() {
                if self.fonts.is_none() {
                    self.fonts = Some(platform::font_families());
                }
                self.font_picker = Some(String::new());
            }
            if !self.cfg.font.is_empty() && ui.button("Default").clicked() {
                self.cfg.font.clear();
            }
        });
        ui.label(egui::RichText::new("Any font installed on this PC. Used for the names on the wheel.").weak().small());

        ui.add_space(6.0);
        ui.checkbox(&mut self.cfg.settings_follow_theme, "Use these colours for this settings window too");
    }

    fn ensure_thumbs(&mut self, ctx: &egui::Context) {
        let key = ThumbKey {
            theme: self.cfg.theme,
            icon_scale: self.cfg.icon_scale,
            icon_version: self.icon_version,
            items: self.cfg.items.len(),
            font: self.cfg.font.clone(),
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
            c.page_size = 0;
            let icons_map = &self.icons;
            let icons = pick_icons(&c, &mut *self.tr, |it| iconcache::item_key(it).and_then(|k| icons_map.get(&k).cloned().flatten()));
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
        ui.label(format!(
            "Hold {} to open the wheel. Move toward a slice and release to open it. A quick tap keeps the wheel open so you can click a slice; right-click or Esc cancels.",
            self.cfg.trigger_label()
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label("Button");
            egui::ComboBox::from_id_salt("trigger_button").selected_text(self.cfg.trigger_button.label()).show_ui(ui, |ui| {
                for b in Button::ALL {
                    ui.selectable_value(&mut self.cfg.trigger_button, b, b.label());
                }
            });
            ui.label("with");
            let middle = self.cfg.trigger_button == Button::Middle;
            egui::ComboBox::from_id_salt("trigger_key").selected_text(self.cfg.trigger_key.label()).show_ui(ui, |ui| {
                for k in Modifier::ALL {
                    if !(middle && k == Modifier::None) {
                        ui.selectable_value(&mut self.cfg.trigger_key, k, k.label());
                    }
                }
            });
        });
        if self.cfg.trigger_button == Button::Middle && self.cfg.trigger_key == Modifier::None {
            self.cfg.trigger_key = Modifier::Shift; // a bare middle click must keep working elsewhere
        }
        if self.cfg.trigger_key == Modifier::None {
            ui.label(
                egui::RichText::new("This side button will only open the wheel: it no longer goes back/forward in other programs.")
                    .weak()
                    .small(),
            );
        }
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
        ui.label(egui::RichText::new("Many apps").strong());
        let mut paged = self.cfg.page_size != 0;
        if ui.checkbox(&mut paged, "Split the wheel into pages").changed() {
            self.cfg.page_size = if paged { DEFAULT_PAGE_SIZE } else { 0 };
        }
        if paged {
            egui::Grid::new("page_slider").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                ui.label("Slices per page");
                ui.add(egui::Slider::new(&mut self.cfg.page_size, 4..=24));
                ui.end_row();
            });
            ui.label(egui::RichText::new("Scroll the mouse wheel while the wheel is open to flip pages.").weak().small());
        } else {
            ui.label(egui::RichText::new("Every app shares one wheel. Names are hidden when the slices get too narrow.").weak().small());
        }

        ui.add_space(6.0);
        ui.label(egui::RichText::new("After choosing").strong());
        ui.checkbox(&mut self.cfg.focus_launched, "Bring the opened app to the front");
        ui.label(egui::RichText::new("Fixes apps that open behind other windows.").weak().small());

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Games and full-screen apps").strong());
        ui.checkbox(&mut self.cfg.pause_in_fullscreen, "Stay out of the way while a full-screen app is in front");
        ui.label(
            egui::RichText::new("Checked every 2 seconds. Also pauses for full-screen video and F11 browser windows.").weak().small(),
        );

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

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Back up").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export settings…").clicked() {
                if let Some(path) = platform::pick_save_settings("radial-launcher-settings.json") {
                    match std::fs::write(&path, self.cfg.clone().sanitized().to_json()) {
                        Ok(()) => self.set_status(format!("Exported to {path}"), false),
                        Err(e) => self.set_status(format!("Could not export: {e}"), true),
                    }
                }
            }
            if ui.button("Import settings…").clicked() {
                if let Some(path) = platform::pick_settings_file() {
                    let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| Config::parse(&s).map_err(|e| e.to_string()));
                    match parsed {
                        Ok(c) => {
                            self.cfg = c;
                            self.selected = 0;
                            self.queue_missing_icons();
                            self.set_status("Imported. Press Save to use it.", false);
                        }
                        Err(e) => self.set_status(format!("Could not import: {e}"), true),
                    }
                }
            }
        });
        ui.label(egui::RichText::new("Picture and app paths are kept as they are, so they only work on a PC that has the same files.").weak().small());

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

        let bad = self.bad();
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
                    ui.colored_label(bad, e.as_str());
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

    // ------------------------------------------------------------ font picker

    /// Window with every installed font. It stays open while you click through the fonts, so the
    /// preview shows each one at once.
    fn ui_font_picker(&mut self, ctx: &egui::Context) {
        let Some(mut filter) = self.font_picker.take() else { return };
        let fonts = self.fonts.take().unwrap_or_default();
        let mut open = true;
        let mut chosen: Option<String> = None;
        egui::Window::new("Choose a font").open(&mut open).collapsible(false).default_size([340.0, 420.0]).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search…").desired_width(f32::INFINITY));
            let needle = filter.to_lowercase();
            let list: Vec<&String> = fonts.iter().filter(|f| needle.is_empty() || f.to_lowercase().contains(&needle)).collect();
            egui::ScrollArea::vertical().show_rows(ui, 24.0, list.len(), |ui, range| {
                for f in &list[range] {
                    if ui.selectable_label(self.cfg.font == **f, f.as_str()).clicked() {
                        chosen = Some((*f).clone());
                    }
                }
            });
        });
        if let Some(f) = chosen {
            self.cfg.font = f;
        }
        self.fonts = Some(fonts);
        if open {
            self.font_picker = Some(filter);
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

        // Window colours follow the wheel's theme (live, while you edit it).
        let look = (self.cfg.theme, self.cfg.settings_follow_theme);
        if self.applied_look != Some(look) {
            let (visuals, dark) = themed_visuals(&look.0, look.1);
            ctx.set_visuals(visuals);
            self.dark = dark;
            self.applied_look = Some(look);
        }
        // New font: the wheel text is rendered by a new renderer for that font.
        if self.cfg.font != self.font_face {
            self.tr = platform::text_renderer(&self.cfg.font);
            self.font_face = self.cfg.font.clone();
        }
        let (warn, good, bad) = (self.warn(), self.good(), self.bad());

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
                    ui.colored_label(warn, "Unsaved changes");
                } else if let Some(s) = &self.status {
                    ui.colored_label(if s.error { bad } else { good }, &s.text);
                }
            });
            if dirty {
                if let Some(s) = self.status.as_ref().filter(|s| s.error) {
                    ui.colored_label(bad, &s.text);
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
        self.ui_font_picker(&ctx);
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
