//! Notification-area icon and its right-click menu.

use super::util::wide;
use radial_launcher::wheel;
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 20;
const TRAY_ID: u32 = 1;

pub const CMD_ENABLED: u32 = 1;
pub const CMD_EDIT: u32 = 2;
pub const CMD_RELOAD: u32 = 3;
pub const CMD_AUTOSTART: u32 = 4;
pub const CMD_QUIT: u32 = 5;

/// Build an HICON from the wheel glyph. The caller owns the handle.
unsafe fn make_icon(accent: [u8; 3]) -> HICON {
    let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
    let glyph = wheel::render_glyph(size, accent);

    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = size as i32;
    bi.bmiHeader.biHeight = -(size as i32);
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let Ok(color) = CreateDIBSection(HDC::default(), &bi, DIB_RGB_COLORS, &mut bits, None, 0)
    else {
        return HICON::default();
    };
    if !bits.is_null() {
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
        for (s, d) in glyph.data().chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            // tiny-skia is premultiplied RGBA; icons want straight-alpha BGRA.
            let a = s[3] as u32;
            let un = |c: u8| {
                if a == 0 {
                    0
                } else {
                    ((c as u32 * 255 + a / 2) / a).min(255) as u8
                }
            };
            d[0] = un(s[2]);
            d[1] = un(s[1]);
            d[2] = un(s[0]);
            d[3] = s[3];
        }
    }
    let mask_bytes = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
    let mask = CreateBitmap(
        size as i32,
        size as i32,
        1,
        1,
        Some(mask_bytes.as_ptr() as *const c_void),
    );

    let info = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = CreateIconIndirect(&info).unwrap_or_default();
    let _ = DeleteObject(HGDIOBJ(color.0));
    let _ = DeleteObject(HGDIOBJ(mask.0));
    icon
}

pub unsafe fn add(hwnd: HWND, accent: [u8; 3]) {
    let icon = make_icon(accent);
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (i, c) in "Radial Launcher (Shift + Middle mouse)"
        .encode_utf16()
        .take(127)
        .enumerate()
    {
        nid.szTip[i] = c;
    }
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    // The shell keeps its own copy of the icon bitmap.
    if !icon.is_invalid() {
        let _ = DestroyIcon(icon);
    }
}

pub unsafe fn remove(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

/// Show the context menu at the pointer; returns the chosen command (0 = none).
pub unsafe fn show_menu(hwnd: HWND, enabled: bool, autostart: bool) -> u32 {
    let Ok(menu) = CreatePopupMenu() else {
        return 0;
    };
    let item = |id: u32, text: &str, checked: bool| {
        let t = wide(text);
        let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(t.as_ptr()));
    };
    item(CMD_ENABLED, "Enabled", enabled);
    item(CMD_AUTOSTART, "Start with Windows", autostart);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    item(CMD_EDIT, "Edit config...", false);
    item(CMD_RELOAD, "Reload config", false);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    item(CMD_QUIT, "Quit", false);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // Required so the menu closes when the user clicks elsewhere.
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );
    let _ = PostMessageW(hwnd, WM_NULL, None, None);
    let _ = DestroyMenu(menu);
    cmd.0 as u32
}
