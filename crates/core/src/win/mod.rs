//! Windows-only helpers shared by the launcher and the settings app.
pub mod autostart;
pub mod fonts;
pub mod gdi_text;
pub mod icons;
pub mod ipc;

use windows::core::PCWSTR;

/// NUL-terminated UTF-16 for passing to `PCWSTR(buf.as_ptr())`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
}
