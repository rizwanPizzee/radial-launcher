//! Wheel geometry, hit-testing and drawing. Pure CPU rendering with tiny-skia;
//! no OS calls, so it is unit-tested and previewed on any platform.

use std::f32::consts::{FRAC_PI_2, TAU};
use tiny_skia::{
    Color, FillRule, FilterQuality, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Stroke,
    Transform,
};

/// Edge length (px) of the square icon bitmap kept in memory per item.
pub const ICON_PX: u32 = 96;

pub struct WheelItem {
    /// `ICON_PX` x `ICON_PX`, premultiplied RGBA.
    pub icon: Pixmap,
    /// Pre-rendered white label shown in the hub while this slice is hovered.
    pub label: Option<Pixmap>,
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// Canvas edge length in physical pixels.
    pub size: u32,
    /// DPI scale (1.0 = 96 dpi).
    pub scale: f32,
    pub r_out: f32,
    pub r_in: f32,
}

impl Layout {
    pub fn new(size: u32, scale: f32) -> Self {
        let half = size as f32 / 2.0;
        // Leave room for the "pop out" of the hovered slice.
        let r_out = half - 14.0 * scale;
        Layout {
            size,
            scale,
            r_out,
            r_in: r_out * 0.36,
        }
    }

    pub fn centre(&self) -> f32 {
        self.size as f32 / 2.0
    }

    /// Which slice is the pointer over? `dx`/`dy` are the pointer offset from the
    /// wheel centre in pixels (y grows downwards). Slice 0 is centred at 12 o'clock
    /// and slices run clockwise. Inside the hub there is no selection (cancel zone).
    pub fn hit_test(&self, dx: f32, dy: f32, n: usize) -> Option<usize> {
        if n == 0 || dx * dx + dy * dy < self.r_in * self.r_in {
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

/// Scale any icon bitmap to exactly `ICON_PX` x `ICON_PX`, keeping its aspect ratio.
pub fn fit_icon(src: &Pixmap) -> Pixmap {
    if src.width() == ICON_PX && src.height() == ICON_PX {
        return src.clone();
    }
    let mut out = Pixmap::new(ICON_PX, ICON_PX).expect("ICON_PX is non-zero");
    let k = (ICON_PX as f32 / src.width() as f32).min(ICON_PX as f32 / src.height() as f32);
    let tx = (ICON_PX as f32 - src.width() as f32 * k) / 2.0;
    let ty = (ICON_PX as f32 - src.height() as f32 * k) / 2.0;
    out.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        },
        Transform::from_row(k, 0.0, 0.0, k, tx, ty),
        None,
    );
    out
}

/// Annular sector between radii `r0`..`r1` and angles `a0`..`a1` (radians).
fn sector(cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32) -> Option<Path> {
    let steps = (((a1 - a0).abs().to_degrees() / 2.0).ceil() as usize).max(2);
    let point = |r: f32, i: usize| {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        (cx + r * a.cos(), cy + r * a.sin())
    };
    let mut pb = PathBuilder::new();
    for i in 0..=steps {
        let (x, y) = point(r1, i);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    for i in (0..=steps).rev() {
        let (x, y) = point(r0, i);
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

fn fill(pm: &mut Pixmap, path: &Path, rgba: [u8; 4]) {
    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p.anti_alias = true;
    pm.fill_path(path, &p, FillRule::Winding, Transform::identity(), None);
}

fn outline(pm: &mut Pixmap, path: &Path, rgba: [u8; 4], width: f32) {
    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p.anti_alias = true;
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pm.stroke_path(path, &p, &stroke, Transform::identity(), None);
}

/// Draw the whole wheel into `pm` (cleared first). `pm` must be `layout.size` square.
pub fn render(
    pm: &mut Pixmap,
    lay: &Layout,
    items: &[WheelItem],
    hovered: Option<usize>,
    accent: [u8; 3],
) {
    pm.fill(Color::TRANSPARENT);
    let n = items.len();
    if n == 0 {
        return;
    }
    let s = lay.scale;
    let c = lay.centre();
    let sweep = TAU / n as f32;
    let gap = if n > 1 { 2.4_f32.to_radians() } else { 0.0 };

    for (i, item) in items.iter().enumerate() {
        let mid = -FRAC_PI_2 + i as f32 * sweep;
        let hot = hovered == Some(i);
        let r1 = if hot { lay.r_out + 8.0 * s } else { lay.r_out };
        let (a0, a1) = (mid - sweep / 2.0 + gap / 2.0, mid + sweep / 2.0 - gap / 2.0);

        if let Some(path) = sector(c, c, lay.r_in, r1, a0, a1) {
            let base = if hot {
                [accent[0], accent[1], accent[2], 238]
            } else {
                [22, 24, 30, 214]
            };
            fill(pm, &path, base);
            outline(
                pm,
                &path,
                [255, 255, 255, if hot { 96 } else { 34 }],
                1.0 * s,
            );
        }

        // Icon, centred in the slice.
        let rm = (lay.r_in + lay.r_out) / 2.0;
        let (px, py) = (c + rm * mid.cos(), c + rm * mid.sin());
        let target = ((lay.r_out - lay.r_in) * 0.56).min(60.0 * s);
        let k = target / ICON_PX as f32 * if hot { 1.14 } else { 1.0 };
        let half = ICON_PX as f32 * k / 2.0;
        let paint = PixmapPaint {
            opacity: if hovered.is_some() && !hot { 0.72 } else { 1.0 },
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        };
        pm.draw_pixmap(
            0,
            0,
            item.icon.as_ref(),
            &paint,
            Transform::from_row(k, 0.0, 0.0, k, px - half, py - half),
            None,
        );
    }

    // Centre hub: shows the hovered app's name, or a dot when nothing is selected.
    let hub_r = lay.r_in - 6.0 * s;
    if let Some(circle) = PathBuilder::from_circle(c, c, hub_r) {
        fill(pm, &circle, [16, 17, 21, 230]);
        outline(pm, &circle, [255, 255, 255, 40], 1.0 * s);
    }
    match hovered.and_then(|i| items[i].label.as_ref()) {
        Some(label) => {
            let k = ((hub_r * 1.7) / label.width() as f32).min(1.0);
            let (w, h) = (label.width() as f32 * k, label.height() as f32 * k);
            pm.draw_pixmap(
                0,
                0,
                label.as_ref(),
                &PixmapPaint {
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                },
                Transform::from_row(k, 0.0, 0.0, k, c - w / 2.0, c - h / 2.0),
                None,
            );
        }
        None => {
            if let Some(dot) = PathBuilder::from_circle(c, c, 3.0 * s) {
                fill(pm, &dot, [255, 255, 255, 70]);
            }
        }
    }
}

/// Small wheel glyph (six slices, one highlighted) used for the tray icon.
pub fn render_glyph(size: u32, accent: [u8; 3]) -> Pixmap {
    let mut pm = Pixmap::new(size, size).expect("glyph size must be non-zero");
    let c = size as f32 / 2.0;
    let r_out = c - 0.5;
    let r_in = r_out * 0.38;
    let sweep = TAU / 6.0;
    let gap = 7.0_f32.to_radians();
    for i in 0..6 {
        let mid = -FRAC_PI_2 + i as f32 * sweep;
        if let Some(path) = sector(
            c,
            c,
            r_in,
            r_out,
            mid - sweep / 2.0 + gap / 2.0,
            mid + sweep / 2.0 - gap / 2.0,
        ) {
            let col = if i == 1 {
                [accent[0], accent[1], accent[2], 255]
            } else {
                [235, 238, 245, 235]
            };
            fill(&mut pm, &path, col);
        }
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lay() -> Layout {
        Layout::new(520, 1.0)
    }

    #[test]
    fn hub_is_a_cancel_zone() {
        assert_eq!(lay().hit_test(0.0, 0.0, 8), None);
        assert_eq!(lay().hit_test(10.0, -10.0, 8), None);
    }

    #[test]
    fn eight_slices_run_clockwise_from_the_top() {
        let l = lay();
        let d = 150.0;
        assert_eq!(l.hit_test(0.0, -d, 8), Some(0)); // up
        assert_eq!(l.hit_test(d, -d, 8), Some(1)); // up-right
        assert_eq!(l.hit_test(d, 0.0, 8), Some(2)); // right
        assert_eq!(l.hit_test(0.0, d, 8), Some(4)); // down
        assert_eq!(l.hit_test(-d, 0.0, 8), Some(6)); // left
        assert_eq!(l.hit_test(-d, -d, 8), Some(7)); // up-left
    }

    #[test]
    fn slice_edges_are_stable_and_far_flicks_still_select() {
        let l = lay();
        // just either side of the boundary between slice 0 and slice 1 (22.5 deg from the top)
        let a0 = (
            -FRAC_PI_2 + 22.0_f32.to_radians(),
            -FRAC_PI_2 + 23.0_f32.to_radians(),
        );
        assert_eq!(
            l.hit_test(200.0 * a0.0.cos(), 200.0 * a0.0.sin(), 8),
            Some(0)
        );
        assert_eq!(
            l.hit_test(200.0 * a0.1.cos(), 200.0 * a0.1.sin(), 8),
            Some(1)
        );
        // a flick far beyond the wheel still counts
        assert_eq!(l.hit_test(0.0, -5000.0, 8), Some(0));
    }

    #[test]
    fn odd_counts_and_single_item() {
        let l = lay();
        assert_eq!(l.hit_test(0.0, -150.0, 5), Some(0));
        assert_eq!(l.hit_test(0.0, 150.0, 1), Some(0));
        assert_eq!(l.hit_test(0.0, 150.0, 0), None);
    }

    #[test]
    fn render_draws_something_and_hover_changes_pixels() {
        let l = lay();
        let items: Vec<WheelItem> = (0..8)
            .map(|_| WheelItem {
                icon: Pixmap::new(ICON_PX, ICON_PX).unwrap(),
                label: None,
            })
            .collect();
        let mut a = Pixmap::new(l.size, l.size).unwrap();
        let mut b = Pixmap::new(l.size, l.size).unwrap();
        render(&mut a, &l, &items, None, [76, 154, 255]);
        render(&mut b, &l, &items, Some(2), [76, 154, 255]);
        assert!(a.pixels().iter().any(|p| p.alpha() > 0));
        assert_ne!(a.data(), b.data());
    }

    #[test]
    fn fit_icon_normalises_size() {
        let src = Pixmap::new(256, 256).unwrap();
        let out = fit_icon(&src);
        assert_eq!((out.width(), out.height()), (ICON_PX, ICON_PX));
    }
}
