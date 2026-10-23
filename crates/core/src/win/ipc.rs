//! Talking to the running launcher from the settings app / refresh helper.

use super::{pcwstr, wide};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};

/// Window class of the launcher's (hidden) message/overlay window.
pub const WHEEL_CLASS: &str = "RadialLauncherWheel";
/// Posted to the launcher: "re-read the config file and the icon cache".
pub const WM_RELOAD: u32 = WM_APP + 30;

pub fn find_launcher() -> Option<HWND> {
    let class = wide(WHEEL_CLASS);
    unsafe { FindWindowW(pcwstr(&class), PCWSTR::null()).ok() }
}

/// Returns false if the launcher is not running.
pub fn notify_launcher_reload() -> bool {
    match find_launcher() {
        Some(hwnd) => unsafe { PostMessageW(hwnd, WM_RELOAD, WPARAM(1), LPARAM(0)).is_ok() },
        None => false,
    }
}
