//! Tiny text rendering helpers.
//!
//! Labels are rasterised once when the config is (re)loaded and kept as small
//! white-on-transparent bitmaps. The font file is dropped right afterwards, so
//! the multi-megabyte font never stays in RAM while the app idles.

use tiny_skia::{FillRule, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

pub use fontdue::Font;

pub fn load_font(path: &str) -> Option<Font> {
    let bytes = std::fs::read(path).ok()?;
    Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
}

/// Shorten long names so they fit inside the wheel hub.
pub fn shorten(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Render `text` as white, premultiplied-alpha pixels at `px` pixel height.
pub fn render_text(font: &Font, text: &str, px: f32) -> Option<Pixmap> {
    let lm = font.horizontal_line_metrics(px)?;
    let ascent = lm.ascent.ceil() as i32;
    let descent = (-lm.descent).ceil() as i32;
    let pad = 2;

    let mut glyphs = Vec::new();
    let mut pen = 0.0f32;
    for ch in text.chars() {
        let (m, bitmap) = font.rasterize(ch, px);
        glyphs.push((pen, m, bitmap));
        pen += m.advance_width;
    }

    let w = pen.ceil() as i32 + pad * 2;
    let h = ascent + descent + pad * 2;
    if w <= pad * 2 || h <= 0 {
        return None;
    }
    let mut pm = Pixmap::new(w as u32, h as u32)?;
    let data = pm.data_mut();

    for (x0, m, bitmap) in glyphs {
        let gx = (x0 + m.xmin as f32).round() as i32 + pad;
        let gy = pad + ascent - m.ymin - m.height as i32;
        for row in 0..m.height {
            for col in 0..m.width {
                let a = bitmap[row * m.width + col];
                if a == 0 {
                    continue;
                }
                let (x, y) = (gx + col as i32, gy + row as i32);
                if x < 0 || y < 0 || x >= w || y >= h {
                    continue;
                }
                let i = ((y * w + x) * 4) as usize;
                let a = a.max(data[i + 3]);
                // white, premultiplied
                data[i] = a;
                data[i + 1] = a;
                data[i + 2] = a;
                data[i + 3] = a;
            }
        }
    }
    Some(pm)
}

/// Stand-in icon (coloured disc with the first letter) for entries whose real
/// icon cannot be loaded. `size` is the edge length in pixels.
pub fn fallback_icon(font: Option<&Font>, name: &str, accent: [u8; 3], size: u32) -> Pixmap {
    let mut pm = Pixmap::new(size, size).expect("icon size must be non-zero");
    let c = size as f32 / 2.0;

    if let Some(disc) = PathBuilder::from_circle(c, c, c - 4.0) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(accent[0], accent[1], accent[2], 210);
        paint.anti_alias = true;
        pm.fill_path(
            &disc,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    if let (Some(font), Some(first)) = (font, name.chars().next()) {
        let letter: String = first.to_uppercase().collect();
        if let Some(glyph) = render_text(font, &letter, size as f32 * 0.5) {
            let x = c - glyph.width() as f32 / 2.0;
            let y = c - glyph.height() as f32 / 2.0;
            pm.draw_pixmap(
                x.round() as i32,
                y.round() as i32,
                glyph.as_ref(),
                &PixmapPaint {
                    quality: FilterQuality::Nearest,
                    ..PixmapPaint::default()
                },
                Transform::identity(),
                None,
            );
        }
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_keeps_short_names_and_ellipsises_long_ones() {
        assert_eq!(shorten("Paint", 22), "Paint");
        let s = shorten("A really quite long application name", 10);
        assert_eq!(s.chars().count(), 10);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn fallback_icon_without_font_is_a_plain_disc() {
        let pm = fallback_icon(None, "Notepad", [10, 20, 30], 96);
        assert_eq!((pm.width(), pm.height()), (96, 96));
        assert!(pm.pixels().iter().any(|p| p.alpha() > 0));
    }
}
