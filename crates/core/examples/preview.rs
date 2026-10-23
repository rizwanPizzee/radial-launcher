//! Renders every wheel design to PNG so the look can be checked on any OS:
//!   cargo run -p radial-core --features fontdue-text --example preview -- out_dir [theme_name]
use radial_core::config::{Config, Style, Theme};
use radial_core::text::FontdueText;
use radial_core::wheel::{pick_icons, Session};
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| ".".into());
    let theme_name = args.next();
    let mut tr = FontdueText::from_files(
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    )
    .expect("DejaVu fonts not found");

    let size = 520u32;
    let sheet_scale = 0.75f32;
    let cell = (size as f32 * sheet_scale) as u32;
    let mut sheet = Pixmap::new(cell * 3, cell * 2).unwrap();
    sheet.fill(Color::from_rgba8(70, 86, 110, 255));

    for (n, style) in Style::ALL.into_iter().enumerate() {
        let mut cfg = Config::default();
        cfg.set_style(style);
        if let Some(name) = &theme_name {
            if let Some((_, t)) = Theme::presets().into_iter().find(|(l, _)| l.eq_ignore_ascii_case(name)) {
                cfg.theme = t;
            }
        }
        cfg.items[2].description = "Browse your files and folders".into();
        cfg.items[2].name = "File Explorer".into();
        let icons = pick_icons(&cfg, &mut tr, |_| None);
        let mut session = Session::new(&cfg, size, icons, &mut tr);

        for (tag, hover) in [("idle", None), ("hover", Some(2usize))] {
            let mut wheel = Pixmap::new(size, size).unwrap();
            session.render(&mut wheel, hover, &mut tr);
            let mut bg = Pixmap::new(size, size).unwrap();
            bg.fill(Color::from_rgba8(70, 86, 110, 255));
            bg.draw_pixmap(0, 0, wheel.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            bg.save_png(format!("{out}/{}_{tag}.png", style.label().to_lowercase())).unwrap();
            if tag == "hover" {
                let (col, row) = ((n % 3) as f32, (n / 3) as f32);
                sheet.draw_pixmap(
                    0,
                    0,
                    bg.as_ref(),
                    &PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() },
                    Transform::from_row(sheet_scale, 0.0, 0.0, sheet_scale, col * cell as f32, row * cell as f32),
                    None,
                );
            }
        }
    }
    sheet.save_png(format!("{out}/sheet.png")).unwrap();
    println!("wrote previews to {out}");
}
