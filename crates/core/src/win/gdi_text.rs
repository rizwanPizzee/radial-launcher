//! Text rendering with GDI: uses the system UI font (Segoe UI) with automatic font
//! fallback for other scripts, and keeps no font data in this process's memory.
//!
//! The text is drawn white-on-black into a temporary 32-bit DIB; the green channel is the
//! anti-aliasing coverage, which becomes the alpha of a white, premultiplied [`Pixmap`].

use super::{pcwstr, wide};
use crate::text::TextRenderer;
use std::ffi::c_void;
use tiny_skia::Pixmap;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::*;

pub struct GdiText {
    dc: HDC,
}

impl GdiText {
    pub fn new() -> GdiText {
        GdiText { dc: unsafe { CreateCompatibleDC(HDC::default()) } }
    }
}

impl Default for GdiText {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GdiText {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.dc);
        }
    }
}

const MAX_DIM: i32 = 2048;

impl TextRenderer for GdiText {
    fn render(&mut self, text: &str, px: f32, bold: bool, wrap_width: Option<u32>) -> Option<Pixmap> {
        if text.trim().is_empty() || self.dc.is_invalid() {
            return None;
        }
        unsafe {
            let face = wide("Segoe UI");
            let font = CreateFontW(
                -(px.round().max(6.0) as i32),
                0,
                0,
                0,
                if bold { 600 } else { 400 },
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                ANTIALIASED_QUALITY.0 as u32,
                0,
                pcwstr(&face),
            );
            if font.is_invalid() {
                return None;
            }
            let old_font = SelectObject(self.dc, HGDIOBJ(font.0));
            let result = self.draw(text, wrap_width);
            SelectObject(self.dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
            result
        }
    }
}

impl GdiText {
    unsafe fn draw(&mut self, text: &str, wrap_width: Option<u32>) -> Option<Pixmap> {
        let mut chars: Vec<u16> = text.encode_utf16().collect();
        let flags = match wrap_width {
            Some(_) => DT_NOPREFIX | DT_WORDBREAK | DT_CENTER,
            None => DT_NOPREFIX | DT_SINGLELINE,
        };

        // Measure.
        let mut rc = RECT { left: 0, top: 0, right: wrap_width.map_or(MAX_DIM, |w| w as i32).clamp(8, MAX_DIM), bottom: 0 };
        DrawTextW(self.dc, &mut chars, &mut rc, flags | DT_CALCRECT);
        let pad = 2;
        let (tw, th) = ((rc.right - rc.left).clamp(1, MAX_DIM), (rc.bottom - rc.top).clamp(1, MAX_DIM));
        let (w, h) = (tw + pad * 2, th + pad * 2);

        // Draw white text on a black 32-bit DIB.
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(HDC::default(), &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(dib.0));
            return None;
        }
        let old_bmp = SelectObject(self.dc, HGDIOBJ(dib.0));
        SetBkMode(self.dc, TRANSPARENT);
        SetTextColor(self.dc, COLORREF(0x00FF_FFFF));
        let mut draw_rc = RECT { left: pad, top: pad, right: pad + tw, bottom: pad + th };
        DrawTextW(self.dc, &mut chars, &mut draw_rc, flags);
        let _ = GdiFlush();

        let src = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
        let mut pm = Pixmap::new(w as u32, h as u32);
        if let Some(pm) = pm.as_mut() {
            for (s, d) in src.chunks_exact(4).zip(pm.data_mut().chunks_exact_mut(4)) {
                let a = s[1]; // grey-scale anti-aliasing: B = G = R = coverage
                d.copy_from_slice(&[a, a, a, a]);
            }
        }
        SelectObject(self.dc, old_bmp);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        pm
    }
}
