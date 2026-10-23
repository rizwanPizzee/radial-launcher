//! Making sure a launched program ends up in front of everything else.
//!
//! Windows only lets a process take the foreground if it recently received input, so a
//! program started by a background tool often opens *behind* other windows. We counter that
//! in two ways: (1) before launching, we obtain foreground rights and hand them to the new
//! process; (2) afterwards we watch for the new top-level window to appear and raise it
//! ourselves, which also covers programs that hand the job to an already-running instance.

use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0
}

/// A normal, visible, titled, top-level application window.
unsafe fn is_app_window(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() {
        return false;
    }
    if GetWindow(hwnd, GW_OWNER).map(|h| !h.0.is_null()).unwrap_or(false) {
        return false;
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    ex & WS_EX_TOOLWINDOW.0 == 0 && GetWindowTextLengthW(hwnd) > 0 && !is_cloaked(hwnd)
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<isize>);
    if is_app_window(hwnd) {
        list.push(hwnd.0 as isize);
    }
    BOOL(1)
}

/// All current application windows, topmost first.
pub fn snapshot() -> Vec<isize> {
    let mut list: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<isize> as isize));
    }
    list
}

/// Application windows that are not in `known`, topmost first.
pub fn new_windows(known: &[isize]) -> Vec<isize> {
    snapshot().into_iter().filter(|h| !known.contains(h)).collect()
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Obtain the right to change the foreground window and let any process use it for a moment.
/// A harmless synthetic key press (F24, which nothing uses) makes Windows treat this process
/// as the one that received the latest input; `AllowSetForegroundWindow(ASFW_ANY)` then lets the
/// program we are about to start bring itself forward.
pub unsafe fn allow_foreground() {
    let inputs = [key(VK_F24, false), key(VK_F24, true)];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    let _ = AllowSetForegroundWindow(ASFW_ANY);
}

/// Raise `hwnd` above everything and give it keyboard focus.
pub unsafe fn bring_to_front(hwnd_raw: isize) {
    let hwnd = HWND(hwnd_raw as *mut c_void);
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    // Sharing the input queue of the current foreground thread is the documented-by-practice way
    // to be allowed to take over the foreground.
    let me = GetCurrentThreadId();
    let fg = GetForegroundWindow();
    let fg_thread = if fg.0.is_null() { 0 } else { GetWindowThreadProcessId(fg, None) };
    let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();

    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);
    if attached {
        let _ = AttachThreadInput(me, fg_thread, false);
    }
    if GetForegroundWindow() != hwnd {
        SwitchToThisWindow(hwnd, true);
    }
}
