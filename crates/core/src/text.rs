//! Text and generated icons.
//!
//! Text is rendered through the [`TextRenderer`] trait so the wheel code stays
//! platform independent: on Windows the launcher and settings app use GDI (system
//! fonts, any script, nothing loaded into RAM), while previews and tests on other
//! systems use the optional `fontdue` implementation.

use crate::color::Color;
use crate::ICON_PX;
use std::f32::consts::{PI, TAU};
use tiny_skia::{FillRule, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

pub trait TextRenderer {
    /// White-on-transparent, premultiplied coverage bitmap of `text` at `px` pixel height.
    /// With `wrap_width`, words wrap so lines stay within that many pixels (centred).
    fn render(&mut self, text: &str, px: f32, bold: bool, wrap_width: Option<u32>) -> Option<Pixmap>;
}

/// Multiply a white coverage bitmap by `color` (premultiplied result).
pub fn tint(pm: &mut Pixmap, color: Color) {
    let [r, g, b, a] = color.0;
    for px in pm.data_mut().chunks_exact_mut(4) {
        let cov = px[3] as u32 * a as u32 / 255;
        px[0] = (cov * r as u32 / 255) as u8;
        px[1] = (cov * g as u32 / 255) as u8;
        px[2] = (cov * b as u32 / 255) as u8;
        px[3] = cov as u8;
    }
}

pub fn render_tinted(tr: &mut dyn TextRenderer, text: &str, px: f32, bold: bool, wrap: Option<u32>, color: Color) -> Option<Pixmap> {
    let mut pm = tr.render(text, px, bold, wrap)?;
    tint(&mut pm, color);
    Some(pm)
}

/// Shorten long names so they fit.
pub fn shorten(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0[0], c.0[1], c.0[2], c.0[3]);
    p.anti_alias = true;
    p
}

const PALETTE: [[u8; 3]; 8] = [
    [76, 154, 255],
    [255, 138, 76],
    [92, 203, 110],
    [166, 107, 255],
    [255, 92, 130],
    [31, 200, 219],
    [240, 190, 60],
    [150, 160, 180],
];

/// Stand-in icon (coloured disc with the first letter) for entries whose real icon is not available.
pub fn fallback_icon(tr: &mut dyn TextRenderer, name: &str) -> Pixmap {
    let size = ICON_PX;
    let mut pm = Pixmap::new(size, size).expect("ICON_PX is non-zero");
    let c = size as f32 / 2.0;
    let hash = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let [r, g, b] = PALETTE[hash as usize % PALETTE.len()];

    if let Some(disc) = PathBuilder::from_circle(c, c, c - 4.0) {
        pm.fill_path(&disc, &paint(Color::rgba(r, g, b, 235)), FillRule::Winding, Transform::identity(), None);
    }
    if let Some(first) = name.trim().chars().next() {
        let letter: String = first.to_uppercase().collect();
        if let Some(glyph) = render_tinted(tr, &letter, size as f32 * 0.5, true, None, Color::rgb(255, 255, 255)) {
            let x = (c - glyph.width() as f32 / 2.0).round() as i32;
            let y = (c - glyph.height() as f32 / 2.0).round() as i32;
            pm.draw_pixmap(x, y, glyph.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    pm
}

/// Gear icon for the built-in "settings" slice.
pub fn gear_icon(color: Color) -> Pixmap {
    let size = ICON_PX;
    let mut pm = Pixmap::new(size, size).expect("ICON_PX is non-zero");
    let c = size as f32 / 2.0;
    let (r_out, r_in, teeth) = (c * 0.86, c * 0.64, 8usize);

    let mut pb = PathBuilder::new();
    for t in 0..teeth {
        let base = t as f32 * TAU / teeth as f32 - PI / 2.0;
        let step = TAU / teeth as f32;
        // Each tooth: rise, flat top, fall, flat valley.
        let pts = [
            (base - step * 0.30, r_in),
            (base - step * 0.17, r_out),
            (base + step * 0.17, r_out),
            (base + step * 0.30, r_in),
        ];
        for (k, (a, r)) in pts.iter().enumerate() {
            let (x, y) = (c + r * a.cos(), c + r * a.sin());
            if t == 0 && k == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
    }
    pb.close();
    // Axle hole, cut out with the even-odd rule.
    let hole = c * 0.30;
    pb.push_circle(c, c, hole);
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint(color), FillRule::EvenOdd, Transform::identity(), None);
    }
    pm
}

/// Draw `src` scaled by `k` with its top-left at (x, y).
pub fn blit_scaled(dst: &mut Pixmap, src: &Pixmap, x: f32, y: f32, k: f32) {
    let quality = if (k - 1.0).abs() < 0.001 { FilterQuality::Nearest } else { FilterQuality::Bilinear };
    let (x, y) = if quality == FilterQuality::Nearest { (x.round(), y.round()) } else { (x, y) };
    dst.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &PixmapPaint { quality, ..PixmapPaint::default() },
        Transform::from_row(k, 0.0, 0.0, k, x, y),
        None,
    );
}

// ---------------------------------------------------------------- fontdue (previews/tests)

#[cfg(feature = "fontdue-text")]
pub use fontdue_impl::FontdueText;

#[cfg(feature = "fontdue-text")]
mod fontdue_impl {
    use super::TextRenderer;
    use fontdue::{Font, FontSettings};
    use tiny_skia::Pixmap;

    pub struct FontdueText {
        regular: Font,
        bold: Font,
    }

    impl FontdueText {
        pub fn from_files(regular: &str, bold: &str) -> Option<Self> {
            let load = |p: &str| Font::from_bytes(std::fs::read(p).ok()?, FontSettings::default()).ok();
            Some(FontdueText { regular: load(regular)?, bold: load(bold).or_else(|| load(regular))? })
        }
    }

    impl TextRenderer for FontdueText {
        fn render(&mut self, text: &str, px: f32, bold: bool, wrap_width: Option<u32>) -> Option<Pixmap> {
            let font = if bold { &self.bold } else { &self.regular };
            let lm = font.horizontal_line_metrics(px)?;
            let (ascent, descent) = (lm.ascent.ceil() as i32, (-lm.descent).ceil() as i32);
            let line_h = ascent + descent + 1;
            let adv = |s: &str| s.chars().map(|c| font.metrics(c, px).advance_width).sum::<f32>();

            // Greedy word wrap.
            let mut lines: Vec<String> = Vec::new();
            match wrap_width {
                None => lines.push(text.to_owned()),
                Some(maxw) => {
                    let mut cur = String::new();
                    for word in text.split_whitespace() {
                        let trial = if cur.is_empty() { word.to_owned() } else { format!("{cur} {word}") };
                        if !cur.is_empty() && adv(&trial) > maxw as f32 {
                            lines.push(std::mem::take(&mut cur));
                            cur = word.to_owned();
                        } else {
                            cur = trial;
                        }
                    }
                    if !cur.is_empty() {
                        lines.push(cur);
                    }
                }
            }
            if lines.is_empty() {
                return None;
            }

            let pad = 2;
            let w = lines.iter().map(|l| adv(l)).fold(0.0f32, f32::max).ceil() as i32 + pad * 2;
            let h = line_h * lines.len() as i32 + pad * 2;
            let mut pm = Pixmap::new(w.max(1) as u32, h.max(1) as u32)?;
            let data = pm.data_mut();
            for (li, line) in lines.iter().enumerate() {
                let mut pen = if wrap_width.is_some() { (w as f32 - adv(line)) / 2.0 } else { pad as f32 };
                let top = pad + li as i32 * line_h;
                for ch in line.chars() {
                    let (m, bitmap) = font.rasterize(ch, px);
                    let gx = (pen + m.xmin as f32).round() as i32;
                    let gy = top + ascent - m.ymin - m.height as i32;
                    for row in 0..m.height {
                        for col in 0..m.width {
                            let a = bitmap[row * m.width + col];
                            let (x, y) = (gx + col as i32, gy + row as i32);
                            if a == 0 || x < 0 || y < 0 || x >= w || y >= h {
                                continue;
                            }
                            let i = ((y * w + x) * 4) as usize;
                            let a = a.max(data[i + 3]);
                            data[i..i + 4].copy_from_slice(&[a, a, a, a]);
                        }
                    }
                    pen += m.advance_width;
                }
            }
            Some(pm)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoText;
    impl TextRenderer for NoText {
        fn render(&mut self, _: &str, _: f32, _: bool, _: Option<u32>) -> Option<Pixmap> {
            None
        }
    }

    #[test]
    fn shorten_keeps_short_names_and_ellipsises_long_ones() {
        assert_eq!(shorten("Paint", 22), "Paint");
        let s = shorten("A really quite long application name", 10);
        assert_eq!(s.chars().count(), 10);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn fallback_and_gear_icons_have_pixels_and_right_size() {
        let a = fallback_icon(&mut NoText, "Notepad");
        assert_eq!((a.width(), a.height()), (ICON_PX, ICON_PX));
        assert!(a.pixels().iter().any(|p| p.alpha() > 0));
        let g = gear_icon(Color::rgb(255, 255, 255));
        assert!(g.pixels().iter().any(|p| p.alpha() > 0));
        // the axle hole is transparent
        let mid = (ICON_PX / 2) as usize;
        assert_eq!(g.pixels()[mid * ICON_PX as usize + mid].alpha(), 0);
    }

    #[test]
    fn tint_scales_colour_and_alpha() {
        let mut pm = Pixmap::new(1, 1).unwrap();
        pm.data_mut().copy_from_slice(&[255, 255, 255, 255]);
        tint(&mut pm, Color::rgba(200, 100, 0, 128));
        let p = pm.pixels()[0];
        assert_eq!(p.alpha(), 128);
        assert!(p.red() > p.green() && p.green() > p.blue());
    }
}
