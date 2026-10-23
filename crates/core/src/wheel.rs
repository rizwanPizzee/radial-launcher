//! Wheel geometry, hit-testing and drawing of the six designs.
//!
//! Pure CPU rendering with tiny-skia and no OS calls, so the exact same code draws the
//! real overlay, the settings preview and the unit-test images.

use crate::color::Color;
use crate::config::{Builtin, Config, Style};
use crate::text::{blit_scaled, fallback_icon, gear_icon, render_tinted, shorten, TextRenderer};
use crate::ICON_PX;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use tiny_skia::{
    Color as SkColor, FillRule, FilterQuality, LineCap, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

// ------------------------------------------------------------------ layout

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// Canvas edge length in pixels.
    pub size: u32,
    /// Design unit: 1.0 at 520 px. All widths and font sizes scale with it.
    pub unit: f32,
    pub c: f32,
    /// Outer radius of the wheel (leaves room for the hover "pop").
    pub r: f32,
    /// Inner radius of the slices / hub edge.
    pub r_in: f32,
    /// Pointer closer than this to the centre selects nothing (cancel zone).
    pub dead: f32,
    pub style: Style,
}

impl Layout {
    pub fn new(size: u32, style: Style, thickness_pct: u8) -> Self {
        let unit = size as f32 / 520.0;
        let c = size as f32 / 2.0;
        let r = c - 14.0 * unit;
        let t = thickness_pct as f32 / 100.0;
        let r_in = match style {
            Style::Segments | Style::Band | Style::Arc | Style::Halo => r * (1.0 - t),
            Style::Orbs => r * 0.5,
            Style::Disc => r * t,
        };
        // The cancel zone is deliberately smaller than the hub: anywhere beyond it, the
        // direction alone picks the slice, like a game weapon wheel.
        let dead = r_in.min(r * 0.28);
        Layout { size, unit, c, r, r_in, dead, style }
    }

    /// Which slice is the pointer over? `dx`/`dy` are the pointer offset from the wheel
    /// centre in pixels (y grows downwards). Slice 0 is centred at 12 o'clock and slices
    /// run clockwise.
    pub fn hit_test(&self, dx: f32, dy: f32, n: usize) -> Option<usize> {
        if n == 0 || dx * dx + dy * dy < self.dead * self.dead {
            return None;
        }
        if n == 1 {
            return Some(0);
        }
        let sweep = TAU / n as f32;
        let t = (dy.atan2(dx) + FRAC_PI_2 + sweep / 2.0).rem_euclid(TAU);
        Some(((t / sweep) as usize).min(n - 1))
    }
}

// ------------------------------------------------------------------ drawing helpers

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0[0], c.0[1], c.0[2], c.0[3]);
    p.anti_alias = true;
    p
}

fn fill(pm: &mut Pixmap, path: &Path, c: Color) {
    if c.a() > 0 {
        pm.fill_path(path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

fn stroke(pm: &mut Pixmap, path: &Path, c: Color, width: f32, cap: LineCap) {
    if c.a() > 0 && width > 0.0 {
        let s = Stroke { width, line_cap: cap, ..Stroke::default() };
        pm.stroke_path(path, &paint(c), &s, Transform::identity(), None);
    }
}

fn circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
    PathBuilder::from_circle(cx, cy, r.max(0.5))
}

/// Annular sector between radii `r0`..`r1` and angles `a0`..`a1` (radians).
fn sector(c: f32, r0: f32, r1: f32, a0: f32, a1: f32) -> Option<Path> {
    let steps = (((a1 - a0).abs().to_degrees() / 2.0).ceil() as usize).max(2);
    let pt = |r: f32, i: usize| {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        (c + r * a.cos(), c + r * a.sin())
    };
    let mut pb = PathBuilder::new();
    for i in 0..=steps {
        let (x, y) = pt(r1, i);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    for i in (0..=steps).rev() {
        let (x, y) = pt(r0, i);
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

fn arc(c: f32, r: f32, a0: f32, a1: f32) -> Option<Path> {
    let steps = (((a1 - a0).abs().to_degrees() / 2.0).ceil() as usize).max(2);
    let mut pb = PathBuilder::new();
    for i in 0..=steps {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        let (x, y) = (c + r * a.cos(), c + r * a.sin());
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.finish()
}

fn line(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x1, y1);
    pb.finish()
}

/// Small triangle on the hub edge pointing at the hovered slice.
fn pointer(pm: &mut Pixmap, c: f32, angle: f32, tip_r: f32, size: f32, color: Color) {
    let (dx, dy) = (angle.cos(), angle.sin());
    let (px, py) = (-dy, dx);
    let base_r = tip_r - size * 1.15;
    let (bx, by) = (c + base_r * dx, c + base_r * dy);
    let mut pb = PathBuilder::new();
    pb.move_to(c + tip_r * dx, c + tip_r * dy);
    pb.line_to(bx + px * size * 0.75, by + py * size * 0.75);
    pb.line_to(bx - px * size * 0.75, by - py * size * 0.75);
    pb.close();
    if let Some(p) = pb.finish() {
        fill(pm, &p, color);
    }
}

fn draw_icon(pm: &mut Pixmap, icon: &Pixmap, cx: f32, cy: f32, target: f32, opacity: f32) {
    let k = target / ICON_PX as f32;
    let half = target / 2.0;
    pm.draw_pixmap(
        0,
        0,
        icon.as_ref(),
        &PixmapPaint { opacity, quality: FilterQuality::Bicubic, ..PixmapPaint::default() },
        Transform::from_row(k, 0.0, 0.0, k, cx - half, cy - half),
        None,
    );
}

// ------------------------------------------------------------------ session

pub struct WheelItem {
    /// `ICON_PX` square, premultiplied RGBA.
    pub icon: Pixmap,
    /// Name rendered for the slice (only when labels are shown).
    pub label: Option<Pixmap>,
}

struct HubText {
    title: Option<Pixmap>,
    desc: Option<Pixmap>,
}

/// Everything needed to draw one open wheel: the config, layout, icons and text bitmaps.
/// Text for the hub is rendered the first time a slice is hovered, so opening is fast and
/// nothing is kept around once the session is dropped.
pub struct Session {
    cfg: Config,
    lay: Layout,
    items: Vec<WheelItem>,
    hub: Vec<Option<HubText>>,
    /// Names on the slices: off when the setting is off or the slices are too narrow to read.
    labels_on: bool,
    page: usize,
    pages: usize,
}

/// Choose each item's icon: real icon if the caller has one, gear for built-in settings,
/// otherwise a generated letter icon.
pub fn pick_icons(
    cfg: &Config,
    tr: &mut dyn TextRenderer,
    mut lookup: impl FnMut(&crate::config::ItemConfig) -> Option<Pixmap>,
) -> Vec<Pixmap> {
    cfg.items
        .iter()
        .map(|it| match it.builtin() {
            Some(Builtin::Settings) if !it.has_custom_icon() => gear_icon(cfg.theme.text),
            Some(Builtin::Settings) => lookup(it).unwrap_or_else(|| gear_icon(cfg.theme.text)),
            None => lookup(it).unwrap_or_else(|| fallback_icon(tr, &it.name)),
        })
        .collect()
}

impl Session {
    /// `icons` must contain one `ICON_PX` square pixmap per configured item.
    pub fn new(cfg: &Config, size_px: u32, icons: Vec<Pixmap>, tr: &mut dyn TextRenderer) -> Session {
        let lay = Layout::new(size_px, cfg.style, cfg.thickness);
        let n = cfg.items.len().max(1) as f32;
        // Arc length available to one slice where its name sits. Beyond ~20 slices on one
        // wheel the names would shrink to unreadable specks, so they are dropped (icons stay).
        let r_label = match cfg.style {
            Style::Disc => lay.r_in + (lay.r - lay.r_in) * 0.52,
            _ => (lay.r_in + lay.r) / 2.0,
        };
        let readable = TAU * r_label / n >= 58.0 * lay.unit;
        let show_labels = (cfg.labels || cfg.style == Style::Disc) && readable;
        let label_px = 13.0 * lay.unit * cfg.text_scale as f32 / 100.0 * if cfg.style == Style::Disc { 1.05 } else { 1.0 };
        let max_chars = if cfg.style == Style::Disc { 12 } else { 14 };

        let items: Vec<WheelItem> = cfg
            .items
            .iter()
            .zip(icons)
            .map(|(it, icon)| WheelItem {
                icon,
                label: show_labels
                    .then(|| render_tinted(tr, &shorten(&it.name, max_chars), label_px, false, None, cfg.theme.text))
                    .flatten(),
            })
            .collect();
        let hub = (0..items.len()).map(|_| None).collect();
        Session { cfg: cfg.clone(), lay, items, hub, labels_on: show_labels, page: 0, pages: 1 }
    }

    /// Tell the wheel which page it shows; with more than one page it draws page dots in the hub.
    pub fn with_page(mut self, page: usize, pages: usize) -> Session {
        self.pages = pages.max(1);
        self.page = page.min(self.pages - 1);
        self
    }

    pub fn layout(&self) -> &Layout {
        &self.lay
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn hit_test(&self, dx: f32, dy: f32) -> Option<usize> {
        self.lay.hit_test(dx, dy, self.items.len())
    }

    fn ensure_hub(&mut self, i: usize, tr: &mut dyn TextRenderer) {
        if self.hub[i].is_some() {
            return;
        }
        let (u, ts) = (self.lay.unit, self.cfg.text_scale as f32 / 100.0);
        let it = &self.cfg.items[i];
        let hub_r = self.hub_radius();
        let color = self.cfg.theme.hub_text;
        let small = matches!(self.cfg.style, Style::Arc | Style::Orbs);
        let title = (!it.name.trim().is_empty())
            .then(|| render_tinted(tr, &shorten(&it.name, 26), (if small { 19.0 } else { 22.0 }) * u * ts, true, None, color))
            .flatten();
        let desc = (!it.description.trim().is_empty())
            .then(|| {
                let wrap = (hub_r * 1.5).max(40.0) as u32;
                render_tinted(tr, &it.description, 13.0 * u * ts, false, Some(wrap), color.faded(0.78))
            })
            .flatten();
        self.hub[i] = Some(HubText { title, desc });
    }

    fn hub_radius(&self) -> f32 {
        let (l, u) = (&self.lay, self.lay.unit);
        match l.style {
            Style::Segments => l.r_in - 6.0 * u,
            Style::Band => l.r_in - 4.0 * u,
            Style::Arc => l.r_in - 4.0 * u,
            Style::Halo => l.r_in - 4.0 * u,
            Style::Orbs => (l.r_in - 2.0 * u).max(20.0 * u),
            Style::Disc => l.r_in,
        }
    }

    /// Draw the wheel into `pm` (cleared first); `pm` must be `layout().size` square.
    pub fn render(&mut self, pm: &mut Pixmap, hovered: Option<usize>, tr: &mut dyn TextRenderer) {
        pm.fill(SkColor::TRANSPARENT);
        if self.items.is_empty() {
            return;
        }
        let hovered = hovered.filter(|&i| i < self.items.len());
        if let Some(h) = hovered {
            if self.cfg.style != Style::Disc {
                self.ensure_hub(h, tr);
            }
        }
        match self.cfg.style {
            Style::Segments => self.draw_ring(pm, hovered, RingKind::Segments),
            Style::Band => self.draw_ring(pm, hovered, RingKind::Band),
            Style::Arc => self.draw_ring(pm, hovered, RingKind::Arc),
            Style::Halo => self.draw_ring(pm, hovered, RingKind::Halo),
            Style::Orbs => self.draw_orbs(pm, hovered),
            Style::Disc => self.draw_disc(pm, hovered),
        }
        if self.pages > 1 {
            self.draw_page_dots(pm);
        }
    }

    /// One dot per page (a small progress bar when there are many pages) at the bottom of the hub.
    fn draw_page_dots(&self, pm: &mut Pixmap) {
        let (c, u) = (self.lay.c, self.lay.unit);
        let hub_r = self.hub_radius();
        let y = c + hub_r * if self.cfg.style == Style::Disc { 0.40 } else { 0.74 };
        let col = self.cfg.theme.hub_text;
        if self.pages <= 10 {
            let step = 11.0 * u;
            let x0 = c - step * (self.pages as f32 - 1.0) / 2.0;
            for i in 0..self.pages {
                let here = i == self.page;
                if let Some(p) = circle(x0 + step * i as f32, y, 2.6 * u * if here { 1.3 } else { 1.0 }) {
                    fill(pm, &p, col.faded(if here { 0.95 } else { 0.32 }));
                }
            }
        } else {
            let w = (hub_r * 0.9).min(90.0 * u);
            if let Some(p) = line(c - w / 2.0, y, c + w / 2.0, y) {
                stroke(pm, &p, col.faded(0.3), 3.0 * u, LineCap::Round);
            }
            let x = c - w / 2.0 + w * self.page as f32 / (self.pages - 1) as f32;
            if let Some(p) = circle(x, y, 4.0 * u) {
                fill(pm, &p, col.faded(0.95));
            }
        }
    }

    // ---- shared pieces

    fn mid(&self, i: usize) -> f32 {
        -FRAC_PI_2 + i as f32 * TAU / self.items.len() as f32
    }

    fn icon_opacity(&self, hovered: Option<usize>, i: usize) -> f32 {
        if hovered.is_some() && hovered != Some(i) {
            0.72
        } else {
            1.0
        }
    }

    fn icon_scale(&self) -> f32 {
        self.cfg.icon_scale as f32 / 100.0
    }

    /// Paint the hub contents: optional big icon, title and description, stacked and centred.
    fn draw_hub_contents(&self, pm: &mut Pixmap, hovered: Option<usize>, hub_r: f32, with_icon: bool) {
        let (c, u) = (self.lay.c, self.lay.unit);
        let Some(h) = hovered else {
            if let Some(dot) = circle(c, c, 3.0 * u) {
                fill(pm, &dot, self.cfg.theme.hub_text.faded(0.28));
            }
            return;
        };
        let Some(Some(text)) = self.hub.get(h) else { return };

        let max_w = hub_r * 1.7;
        let icon_h = if with_icon { hub_r * 0.55 } else { 0.0 };
        let gap = 6.0 * u;
        let mut total = 0.0;
        let mut parts = 0;
        if with_icon {
            total += icon_h;
            parts += 1;
        }
        let mut sizes = [0.0f32; 2];
        for (k, p) in [&text.title, &text.desc].iter().enumerate() {
            if let Some(p) = p {
                let s = (max_w / p.width() as f32).min(1.0);
                sizes[k] = s;
                total += p.height() as f32 * s;
                parts += 1;
            }
        }
        if parts == 0 {
            return;
        }
        total += gap * (parts - 1) as f32;

        let mut y = c - total / 2.0;
        if with_icon {
            draw_icon(pm, &self.items[h].icon, c, y + icon_h / 2.0, icon_h, 1.0);
            y += icon_h + gap;
        }
        for (k, p) in [&text.title, &text.desc].iter().enumerate() {
            if let Some(p) = p {
                let s = sizes[k];
                let (w, hh) = (p.width() as f32 * s, p.height() as f32 * s);
                blit_scaled(pm, p, c - w / 2.0, y, s);
                y += hh + gap;
            }
        }
    }

    // ---- the four ring designs

    fn draw_ring(&self, pm: &mut Pixmap, hovered: Option<usize>, kind: RingKind) {
        let (l, th) = (&self.lay, &self.cfg.theme);
        let (c, u, r_out, r_in) = (l.c, l.unit, l.r, l.r_in);
        let n = self.items.len();
        let sweep = TAU / n as f32;
        let gap = if n > 1 { self.cfg.gap.to_radians() } else { 0.0 };
        let pop = match kind {
            RingKind::Segments => 8.0 * u,
            RingKind::Halo => 6.0 * u,
            _ => 0.0,
        };

        // Wedges: idle ones first so the hovered one always lies on top.
        let order = (0..n).filter(|&i| Some(i) != hovered).chain(hovered);
        for i in order {
            let hot = hovered == Some(i);
            let mid = self.mid(i);
            let r1 = r_out + if hot { pop } else { 0.0 };
            let (a0, a1) = (mid - sweep / 2.0 + gap / 2.0, mid + sweep / 2.0 - gap / 2.0);
            let Some(path) = sector(c, r_in, r1, a0, a1) else { continue };

            let base = match (kind, hot) {
                (_, true) if kind != RingKind::Arc => th.hover,
                (RingKind::Arc, true) => th.background.mix(th.hover, 0.22),
                _ => th.background,
            };
            fill(pm, &path, base);
            match kind {
                RingKind::Segments => stroke(pm, &path, if hot { th.hover.shade(0.4) } else { th.border }, 1.2 * u, LineCap::Butt),
                RingKind::Arc => {
                    stroke(pm, &path, th.border, 1.0 * u, LineCap::Butt);
                    if hot {
                        if let Some(a) = arc(c, r_out - 3.5 * u, a0 + 0.01, a1 - 0.01) {
                            stroke(pm, &a, th.hover, 5.0 * u, LineCap::Butt);
                        }
                        if let Some(a) = arc(c, r_in + 3.0 * u, a0 + 0.01, a1 - 0.01) {
                            stroke(pm, &a, th.hub_text.faded(0.85), 1.6 * u, LineCap::Butt);
                        }
                    }
                }
                RingKind::Band | RingKind::Halo => {}
            }
        }
        if kind == RingKind::Band {
            // Thin dividers and rims.
            for i in 0..n {
                let a = self.mid(i) - sweep / 2.0;
                if gap == 0.0 && n > 1 {
                    if let Some(p) = line(c + r_in * a.cos(), c + r_in * a.sin(), c + r_out * a.cos(), c + r_out * a.sin()) {
                        stroke(pm, &p, th.border, 1.4 * u, LineCap::Butt);
                    }
                }
            }
            for r in [r_out, r_in] {
                if let Some(p) = circle(c, c, r) {
                    stroke(pm, &p, th.border, 1.4 * u, LineCap::Butt);
                }
            }
        }

        // Icons (and optional names).
        let ring_w = r_out - r_in;
        let labels = self.labels_on && kind != RingKind::Halo;
        for i in 0..n {
            let hot = hovered == Some(i);
            let mid = self.mid(i);
            let rm = (r_in + r_out) / 2.0 + if hot { pop / 2.0 } else { 0.0 };
            let (cx, cy) = (c + rm * mid.cos(), c + rm * mid.sin());
            let arc = TAU * rm / n as f32;
            let label = if labels { self.items[i].label.as_ref() } else { None };
            if let Some(lb) = label {
                // Icon above name as one stack centred in the wedge: unlike a name placed further
                // out along the wedge, it stays inside the slice on every side of the wheel
                // (side slices are wide radially, top and bottom ones along the arc).
                let avail = (arc * 0.86).min(ring_w * 0.92);
                let target = (ring_w * 0.40).min(avail * 0.72).min(64.0 * u) * self.icon_scale();
                let k = (avail / lb.width() as f32).min(1.0);
                let gap = 4.0 * u;
                let top = cy - (target + gap + lb.height() as f32 * k) / 2.0;
                draw_icon(pm, &self.items[i].icon, cx, top + target / 2.0, target, self.icon_opacity(hovered, i));
                blit_scaled(pm, lb, cx - lb.width() as f32 * k / 2.0, top + target + gap, k);
            } else {
                let target = (ring_w * if kind == RingKind::Halo { 0.62 } else { 0.56 }).min(64.0 * u).min(arc * 0.8) * self.icon_scale();
                draw_icon(pm, &self.items[i].icon, cx, cy, target, self.icon_opacity(hovered, i));
            }
        }

        // Hub.
        let hub_r = self.hub_radius();
        if let Some(p) = circle(c, c, hub_r) {
            fill(pm, &p, th.hub);
            stroke(pm, &p, th.border, 1.2 * u, LineCap::Butt);
        }
        if kind == RingKind::Arc {
            if let Some(p) = circle(c, c, hub_r - 7.0 * u) {
                stroke(pm, &p, th.border.faded(0.7), 1.0 * u, LineCap::Butt);
            }
        }
        self.draw_hub_contents(pm, hovered, hub_r - if kind == RingKind::Arc { 7.0 * u } else { 0.0 }, kind == RingKind::Halo);
        if let (Some(h), true) = (hovered, matches!(kind, RingKind::Segments | RingKind::Band)) {
            pointer(pm, c, self.mid(h), hub_r - 1.0 * u, 7.0 * u, th.hub_text.faded(0.9));
        }
    }

    // ---- orbs

    fn draw_orbs(&self, pm: &mut Pixmap, hovered: Option<usize>) {
        let (l, th) = (&self.lay, &self.cfg.theme);
        let (c, u) = (l.c, l.unit);
        let n = self.items.len();
        let t = self.cfg.thickness as f32 / 100.0;
        let mut ro = l.r * (0.12 + t * 0.25);
        if n > 1 {
            ro = ro.min(l.r * 0.78 * (PI / n as f32).sin() * 0.92 / 0.78);
        }
        ro = ro.min(l.r * 0.34);
        let r_ring = l.r - ro * 1.1 - 2.0 * u;

        if let Some(p) = circle(c, c, r_ring) {
            stroke(pm, &p, th.border.faded(0.8), 1.4 * u, LineCap::Butt);
        }

        let order = (0..n).filter(|&i| Some(i) != hovered).chain(hovered);
        for i in order {
            let hot = hovered == Some(i);
            let mid = self.mid(i);
            let (x, y) = (c + r_ring * mid.cos(), c + r_ring * mid.sin());
            let rr = ro * if hot { 1.1 } else { 1.0 };
            if let Some(p) = circle(x, y, rr) {
                fill(pm, &p, if hot { th.hover } else { th.background });
                stroke(pm, &p, if hot { th.hub_text.faded(0.95) } else { th.border.faded(1.6) }, if hot { 2.6 * u } else { 2.0 * u }, LineCap::Butt);
            }
            draw_icon(pm, &self.items[i].icon, x, y, rr * 1.2 * self.icon_scale(), self.icon_opacity(hovered, i));
        }

        let hub_r = self.hub_radius().min(r_ring - ro - 6.0 * u);
        if let Some(p) = circle(c, c, hub_r) {
            fill(pm, &p, th.hub);
            stroke(pm, &p, th.border, 1.2 * u, LineCap::Butt);
        }
        self.draw_hub_contents(pm, hovered, hub_r, false);
        if let Some(h) = hovered {
            pointer(pm, c, self.mid(h), hub_r - 1.0 * u, 7.0 * u, th.hub_text.faded(0.9));
        }
    }

    // ---- disc

    fn draw_disc(&self, pm: &mut Pixmap, hovered: Option<usize>) {
        let (l, th) = (&self.lay, &self.cfg.theme);
        let (c, u, r_out, r_in) = (l.c, l.unit, l.r, l.r_in);
        let n = self.items.len();
        let sweep = TAU / n as f32;

        if let Some(p) = circle(c, c, r_out) {
            fill(pm, &p, th.background);
        }
        if let Some(h) = hovered {
            let mid = self.mid(h);
            let path = if n == 1 {
                circle(c, c, r_out)
            } else {
                sector(c, r_in, r_out, mid - sweep / 2.0, mid + sweep / 2.0)
            };
            if let Some(p) = path {
                fill(pm, &p, th.hover.faded(0.62));
            }
        }
        if n > 1 {
            for i in 0..n {
                let a = self.mid(i) - sweep / 2.0;
                if let Some(p) = line(c + r_in * a.cos(), c + r_in * a.sin(), c + r_out * a.cos(), c + r_out * a.sin()) {
                    stroke(pm, &p, th.border, 1.3 * u, LineCap::Butt);
                }
            }
        }
        if let Some(p) = circle(c, c, r_out) {
            stroke(pm, &p, th.border.faded(1.5), 2.0 * u, LineCap::Butt);
        }

        // Icon above name, as one stack centred in the middle of each wedge. Stacking keeps
        // the text inside the wedge on every side of the wheel (side wedges are wide
        // radially, top/bottom wedges are wide along the arc).
        let ring_w = r_out - r_in;
        let gap = 6.0 * u;
        for i in 0..n {
            let mid = self.mid(i);
            let r_s = r_in + ring_w * 0.52;
            let (px, py) = (c + r_s * mid.cos(), c + r_s * mid.sin());
            let avail = (TAU * r_s / n as f32 * 0.86).min(ring_w * 0.86);
            let target = (ring_w * 0.30).min(avail * 0.75).min(60.0 * u) * self.icon_scale();

            let label = self.items[i].label.as_ref();
            let k = label.map_or(1.0, |lb| (avail / lb.width() as f32).min(1.0));
            let label_h = label.map_or(0.0, |lb| lb.height() as f32 * k);
            let total = target + if label.is_some() { gap + label_h } else { 0.0 };
            let top = py - total / 2.0;

            draw_icon(pm, &self.items[i].icon, px, top + target / 2.0, target, self.icon_opacity(hovered, i));
            if let Some(lb) = label {
                blit_scaled(pm, lb, px - lb.width() as f32 * k / 2.0, top + target + gap, k);
            }
        }

        // Centre hub: plain emblem, no text.
        if let Some(p) = circle(c, c, r_in) {
            fill(pm, &p, th.hub);
            stroke(pm, &p, th.border.faded(1.5), 1.6 * u, LineCap::Butt);
        }
        if let Some(p) = circle(c, c, r_in * 0.62) {
            stroke(pm, &p, th.border, 1.2 * u, LineCap::Butt);
        }
        if let Some(p) = circle(c, c, 3.0 * u) {
            fill(pm, &p, if hovered.is_some() { th.hover } else { th.hub_text.faded(0.5) });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RingKind {
    Segments,
    Band,
    Arc,
    Halo,
}

/// Small helper for the settings app: draw a mini preview of `cfg` into a square pixmap.
pub fn render_preview(cfg: &Config, size_px: u32, icons: Vec<Pixmap>, hovered: Option<usize>, tr: &mut dyn TextRenderer) -> Pixmap {
    render_preview_page(cfg, size_px, icons, hovered, 0, 1, tr)
}

/// Like [`render_preview`] for one page of a paged wheel: `cfg` holds that page's items only
/// (see `Config::page_config`) and the hub shows which page of `pages` this is.
pub fn render_preview_page(
    cfg: &Config,
    size_px: u32,
    icons: Vec<Pixmap>,
    hovered: Option<usize>,
    page: usize,
    pages: usize,
    tr: &mut dyn TextRenderer,
) -> Pixmap {
    let mut pm = Pixmap::new(size_px.max(8), size_px.max(8)).expect("preview size is non-zero");
    let mut s = Session::new(cfg, size_px.max(8), icons, tr).with_page(page, pages);
    s.render(&mut pm, hovered, tr);
    pm
}

/// Small wheel glyph (six slices, one highlighted) used for the tray icon.
pub fn tray_glyph(size: u32, accent: Color) -> Pixmap {
    let mut pm = Pixmap::new(size.max(8), size.max(8)).expect("glyph size is non-zero");
    let c = pm.width() as f32 / 2.0;
    let (r_out, r_in) = (c - 0.5, (c - 0.5) * 0.38);
    let sweep = TAU / 6.0;
    let gap = 7.0_f32.to_radians();
    for i in 0..6 {
        let mid = -FRAC_PI_2 + i as f32 * sweep;
        if let Some(path) = sector(c, r_in, r_out, mid - sweep / 2.0 + gap / 2.0, mid + sweep / 2.0 - gap / 2.0) {
            fill(&mut pm, &path, if i == 1 { accent.with_alpha(255) } else { Color::rgba(235, 238, 245, 235) });
        }
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    struct NoText;
    impl TextRenderer for NoText {
        fn render(&mut self, _: &str, _: f32, _: bool, _: Option<u32>) -> Option<Pixmap> {
            None
        }
    }

    /// Text renderer that returns a small block for any text, so label logic can be tested.
    struct Named;
    impl TextRenderer for Named {
        fn render(&mut self, text: &str, _: f32, _: bool, _: Option<u32>) -> Option<Pixmap> {
            (!text.trim().is_empty()).then(|| Pixmap::new(40, 12).unwrap())
        }
    }

    fn icons(cfg: &Config) -> Vec<Pixmap> {
        pick_icons(cfg, &mut NoText, |_| None)
    }

    fn lay(style: Style) -> Layout {
        Layout::new(520, style, style.preset().thickness)
    }

    #[test]
    fn centre_is_a_cancel_zone_and_far_flicks_still_select() {
        for style in Style::ALL {
            let l = lay(style);
            assert_eq!(l.hit_test(0.0, 0.0, 8), None, "{style:?}");
            assert_eq!(l.hit_test(5.0, -5.0, 8), None, "{style:?}");
            assert_eq!(l.hit_test(0.0, -5000.0, 8), Some(0), "{style:?}");
        }
    }

    #[test]
    fn slices_run_clockwise_from_the_top() {
        for style in Style::ALL {
            let l = lay(style);
            let d = 150.0;
            assert_eq!(l.hit_test(0.0, -d, 8), Some(0), "{style:?}");
            assert_eq!(l.hit_test(d, -d, 8), Some(1), "{style:?}");
            assert_eq!(l.hit_test(d, 0.0, 8), Some(2), "{style:?}");
            assert_eq!(l.hit_test(0.0, d, 8), Some(4), "{style:?}");
            assert_eq!(l.hit_test(-d, 0.0, 8), Some(6), "{style:?}");
            assert_eq!(l.hit_test(-d, -d, 8), Some(7), "{style:?}");
        }
    }

    #[test]
    fn slice_boundaries_and_odd_counts() {
        let l = lay(Style::Segments);
        let a = |deg: f32| (-FRAC_PI_2 + deg.to_radians()).sin_cos();
        let (s, c) = a(22.0);
        assert_eq!(l.hit_test(200.0 * c, 200.0 * s, 8), Some(0));
        let (s, c) = a(23.0);
        assert_eq!(l.hit_test(200.0 * c, 200.0 * s, 8), Some(1));
        assert_eq!(l.hit_test(0.0, -150.0, 5), Some(0));
        assert_eq!(l.hit_test(0.0, 150.0, 1), Some(0));
        assert_eq!(l.hit_test(0.0, 150.0, 0), None);
    }

    #[test]
    fn every_style_draws_and_reacts_to_hover_for_all_item_counts() {
        for style in Style::ALL {
            for n in [1usize, 2, 3, 5, 8, 12, 24, 60, 150] {
                let mut cfg = Config::default();
                cfg.set_style(style);
                cfg.items = (0..n).map(|i| crate::config::ItemConfig::new(&format!("App {i}"), "x")).collect();
                let size = 520;
                let mut s = Session::new(&cfg, size, icons(&cfg), &mut NoText);
                let (mut a, mut b) = (Pixmap::new(size, size).unwrap(), Pixmap::new(size, size).unwrap());
                s.render(&mut a, None, &mut NoText);
                s.render(&mut b, Some(n / 2), &mut NoText);
                assert!(a.pixels().iter().any(|p| p.alpha() > 0), "{style:?} n={n} drew nothing");
                assert_ne!(a.data(), b.data(), "{style:?} n={n} hover had no visible effect");
            }
        }
    }

    #[test]
    fn crowded_wheels_drop_names_but_keep_icons() {
        let mut cfg = Config::default();
        cfg.set_style(Style::Segments);
        cfg.labels = true;
        cfg.items = (0..8).map(|i| crate::config::ItemConfig::new(&format!("App {i}"), "x")).collect();
        assert!(Session::new(&cfg, 520, icons(&cfg), &mut Named).labels_on);
        cfg.items = (0..60).map(|i| crate::config::ItemConfig::new(&format!("App {i}"), "x")).collect();
        let s = Session::new(&cfg, 520, icons(&cfg), &mut Named);
        assert!(!s.labels_on);
        assert!(s.items.iter().all(|it| it.label.is_none()));
    }

    #[test]
    fn page_dots_appear_only_with_several_pages() {
        let cfg = Config::default();
        let size = 400;
        let draw = |page: usize, pages: usize| {
            let mut s = Session::new(&cfg, size, icons(&cfg), &mut NoText).with_page(page, pages);
            let mut pm = Pixmap::new(size, size).unwrap();
            s.render(&mut pm, None, &mut NoText);
            pm.data().to_vec()
        };
        let one = draw(0, 1);
        assert_ne!(one, draw(0, 3));
        assert_ne!(draw(0, 3), draw(1, 3));
        assert_ne!(draw(0, 40), draw(39, 40)); // progress-bar form
        assert_eq!(draw(5, 1), one); // page index is ignored without pages
    }

    #[test]
    fn empty_wheel_is_blank_and_out_of_range_hover_is_ignored() {
        let mut cfg = Config::default();
        cfg.items.clear();
        let mut s = Session::new(&cfg, 300, Vec::new(), &mut NoText);
        let mut pm = Pixmap::new(300, 300).unwrap();
        s.render(&mut pm, Some(3), &mut NoText);
        assert!(pm.pixels().iter().all(|p| p.alpha() == 0));

        let cfg = Config::default();
        let mut s = Session::new(&cfg, 300, icons(&cfg), &mut NoText);
        s.render(&mut pm, Some(99), &mut NoText); // must not panic
    }

    #[test]
    fn tray_glyph_draws() {
        let g = tray_glyph(16, Color::rgb(76, 154, 255));
        assert!(g.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn tiny_and_huge_sizes_do_not_panic() {
        for size in [64u32, 300, 900] {
            for style in Style::ALL {
                let mut cfg = Config::default();
                cfg.set_style(style);
                let mut s = Session::new(&cfg, size, icons(&cfg), &mut NoText);
                let mut pm = Pixmap::new(size, size).unwrap();
                s.render(&mut pm, Some(1), &mut NoText);
            }
        }
    }
}
