//! App icons via the shell's thumbnail factory.
//!
//! `IShellItemImageFactory` returns a premultiplied 32-bit bitmap for anything the shell
//! can parse: .exe/.lnk files, folders, URLs and `shell:AppsFolder\<AUMID>` entries
//! (Store apps) alike. This pulls in a lot of shell/imaging code, so it only ever runs in
//! short-lived processes (settings app, `radial-launcher --refresh-icons`), never in the
//! resident launcher.

use super::{pcwstr, wide};
use crate::config::{expand_env, Config, ItemConfig};
use crate::iconcache::{self, fit_icon};
use crate::ICON_PX;
use std::ffi::c_void;
use tiny_skia::Pixmap;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, IBindCtx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};

/// RAII guard: COM initialised on this thread for as long as it lives.
pub struct ComGuard(bool);

impl ComGuard {
    pub fn new() -> ComGuard {
        ComGuard(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() })
    }
}

impl Default for ComGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// The shell icon for `path` (environment variables are expanded), fitted to `ICON_PX`.
/// COM must be initialised on the calling thread (see [`ComGuard`]).
pub fn load_icon(path: &str) -> Option<Pixmap> {
    load(path, true)
}

/// A picture chosen by the user (PNG, JPG, ICO, BMP, GIF...): the shell's thumbnail of the file,
/// which keeps transparency. Falls back to the file's icon, so an .exe/.dll works too.
pub fn load_picture(path: &str) -> Option<Pixmap> {
    load(path, false).or_else(|| load(path, true))
}

/// The icon the wheel shows for `item`: its custom picture if it has one, otherwise the icon of
/// the target itself. `None` for built-in actions without a picture.
pub fn load_for_item(item: &ItemConfig) -> Option<Pixmap> {
    if item.has_custom_icon() {
        load_picture(item.icon.trim())
    } else if item.builtin().is_some() || item.path.trim().is_empty() {
        None
    } else {
        load_icon(&item.path)
    }
}

fn load(path: &str, icon_only: bool) -> Option<Pixmap> {
    unsafe {
        let p = wide(&expand_env(path.trim()));
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(pcwstr(&p), None::<&IBindCtx>).ok()?;
        let px = ICON_PX as i32;
        let flags = if icon_only { SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK } else { SIIGBF_BIGGERSIZEOK };
        let hbm = factory.GetImage(SIZE { cx: px, cy: px }, flags).ok()?;
        let pixmap = bitmap_to_pixmap(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        pixmap.map(|p| fit_icon(&p))
    }
}

unsafe fn bitmap_to_pixmap(hbm: HBITMAP) -> Option<Pixmap> {
    let mut bm = BITMAP::default();
    let got = GetObjectW(HGDIOBJ(hbm.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut c_void));
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmWidth > 1024 || bm.bmHeight > 1024 {
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
    let lines = GetDIBits(screen, hbm, 0, h as u32, Some(buf.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, screen);
    if lines == 0 {
        return None;
    }

    // Some icons come back without an alpha channel: treat those as opaque.
    let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
    let mut pm = Pixmap::new(w as u32, h as u32)?;
    for (src, dst) in buf.chunks_exact(4).zip(pm.data_mut().chunks_exact_mut(4)) {
        let a = if has_alpha { src[3] } else { 255 };
        // BGRA -> RGBA; clamp colour to alpha so a malformed pixel cannot leave the premultiplied range.
        dst[0] = src[2].min(a);
        dst[1] = src[1].min(a);
        dst[2] = src[0].min(a);
        dst[3] = a;
    }
    Some(pm)
}

/// Extract the icon of every item in `cfg` and write the icon cache file. Returns how many
/// icons were found. Initialises COM itself.
pub fn refresh_cache(cfg: &Config) -> std::io::Result<usize> {
    let _com = ComGuard::new();
    let mut found: Vec<(u64, Pixmap)> = Vec::new();
    for item in &cfg.items {
        let Some(key) = iconcache::item_key(item) else { continue };
        if found.iter().any(|(k, _)| *k == key) {
            continue;
        }
        if let Some(icon) = load_for_item(item) {
            found.push((key, icon));
        }
    }
    let refs: Vec<(u64, &Pixmap)> = found.iter().map(|(k, p)| (*k, p)).collect();
    iconcache::write(&iconcache::cache_path(), &refs)?;
    Ok(found.len())
}
