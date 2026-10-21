use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

/// NUL-terminated UTF-16, for passing to `PCWSTR(buf.as_ptr())`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn message_box(title: &str, text: &str) {
    let (t, c) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(c.as_ptr()),
            PCWSTR(t.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
}
