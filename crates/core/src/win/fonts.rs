//! List of installed font families, for the font picker in the settings app.

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Graphics::Gdi::*;

unsafe extern "system" fn collect(lf: *const LOGFONTW, _tm: *const TEXTMETRICW, _kind: u32, lparam: LPARAM) -> i32 {
    let out = &mut *(lparam.0 as *mut Vec<String>);
    let name = &(*lf).lfFaceName;
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let name = String::from_utf16_lossy(&name[..len]);
    // Names starting with '@' are vertical-writing duplicates of the normal families.
    if !name.is_empty() && !name.starts_with('@') {
        out.push(name);
    }
    1 // keep enumerating
}

/// Every installed font family once, sorted A-Z.
pub fn installed_families() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    unsafe {
        let hdc = GetDC(None);
        let lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
        EnumFontFamiliesExW(hdc, &lf, Some(collect), LPARAM(&mut out as *mut Vec<String> as isize), 0);
        ReleaseDC(None, hdc);
    }
    out.sort_by_key(|s| s.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}
