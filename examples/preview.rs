//! Renders the wheel to PNG files so the look can be checked on any OS:
//!   cargo run --example preview -- /path/to/font.ttf out_dir
use radial_launcher::{config::Config, text, wheel};
use tiny_skia::{Color, Pixmap};

fn main() {
    let mut args = std::env::args().skip(1);
    let font_path = args
        .next()
        .unwrap_or_else(|| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
    let out_dir = args.next().unwrap_or_else(|| ".".into());

    let cfg = Config::default();
    let accent = cfg.accent_rgb();
    let font = text::load_font(&font_path);
    let lay = wheel::Layout::new(cfg.size, 1.0);

    let items: Vec<wheel::WheelItem> = cfg
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            // Vary hue a little so the slices are distinguishable in the preview.
            let a = [
                accent[0].wrapping_add((i as u8).wrapping_mul(23)),
                accent[1].wrapping_sub((i as u8).wrapping_mul(11)),
                accent[2],
            ];
            wheel::WheelItem {
                icon: text::fallback_icon(font.as_ref(), &it.name, a, wheel::ICON_PX),
                label: font
                    .as_ref()
                    .and_then(|f| text::render_text(f, &text::shorten(&it.name, 22), 26.0)),
            }
        })
        .collect();

    // Composite over a mid-grey "desktop" so the translucency is visible.
    for (name, hovered) in [("idle", None), ("hover", Some(2usize))] {
        let mut wheel_px = Pixmap::new(lay.size, lay.size).unwrap();
        wheel::render(&mut wheel_px, &lay, &items, hovered, accent);
        let mut bg = Pixmap::new(lay.size, lay.size).unwrap();
        bg.fill(Color::from_rgba8(70, 86, 110, 255));
        bg.draw_pixmap(
            0,
            0,
            wheel_px.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        bg.save_png(format!("{out_dir}/wheel_{name}.png")).unwrap();
    }
    let tray = wheel::render_glyph(32, accent);
    tray.save_png(format!("{out_dir}/tray.png")).unwrap();
    println!("wrote previews to {out_dir}");
}
