//! App icons via the shell's thumbnail factory.
//!
//! `IShellItemImageFactory` returns a premultiplied 32-bit bitmap for anything the
//! shell can parse: .exe/.lnk files, folders, URLs and `shell:AppsFolder\<AUMID>`
//! entries (Store apps) alike. We copy it into a small `Pixmap` and release the GDI
//! bitmap immediately, so nothing GDI-related stays alive while the app idles.

use super::util::wide;
use std::ffi::c_void;
use tiny_skia::Pixmap;
use windows::core::PCWSTR;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};

/// Expand `%VARS%` in a path.
pub fn expand(s: &str) -> String {
    let src = wide(s);
    unsafe {
        let need = ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), None) as usize;
        if need == 0 {
            return s.to_owned();
        }
        let mut buf = vec![0u16; need];
        let got = ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), Some(&mut buf)) as usize;
        if got == 0 || got > need {
            return s.to_owned();
        }
        String::from_utf16_lossy(&buf[..got.saturating_sub(1)])
    }
}

pub fn load_icon(path: &str, px: i32) -> Option<Pixmap> {
    unsafe {
        let p = wide(path);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(p.as_ptr()), None::<&IBindCtx>).ok()?;
        let hbm = factory
            .GetImage(
                SIZE { cx: px, cy: px },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;
        let pixmap = bitmap_to_pixmap(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        pixmap
    }
}

unsafe fn bitmap_to_pixmap(hbm: HBITMAP) -> Option<Pixmap> {
    let mut bm = BITMAP::default();
    let got = GetObjectW(
        HGDIOBJ(hbm.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut BITMAP as *mut c_void),
    );
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth as usize, bm.bmHeight as usize);

    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w as i32;
    bi.bmiHeader.biHeight = -(h as i32); // top-down
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB.0;

    let mut buf = vec![0u8; w * h * 4];
    let screen = GetDC(None);
    let lines = GetDIBits(
        screen,
        hbm,
        0,
        h as u32,
        Some(buf.as_mut_ptr() as *mut c_void),
        &mut bi,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, screen);
    if lines == 0 {
        return None;
    }

    // Some icons come back without an alpha channel: treat those as opaque.
    let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
    let mut pm = Pixmap::new(w as u32, h as u32)?;
    for (src, dst) in buf.chunks_exact(4).zip(pm.data_mut().chunks_exact_mut(4)) {
        let a = if has_alpha { src[3] } else { 255 };
        // BGRA -> RGBA; clamp colour to alpha so a malformed pixel can't overflow premultiplied range.
        dst[0] = src[2].min(a);
        dst[1] = src[1].min(a);
        dst[2] = src[0].min(a);
        dst[3] = a;
    }
    Some(pm)
}
