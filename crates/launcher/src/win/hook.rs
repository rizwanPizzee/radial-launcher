//! Global low-level mouse hook on its own thread.
//!
//! The hook callback does as little as possible (it runs for *every* mouse
//! event on the system and Windows removes hooks that answer too slowly): a few
//! atomic loads, and a `PostMessage` to the UI thread when something relevant
//! happens. All drawing and launching happens on the UI thread.
//!
//! Gesture: hold **Shift + Middle button** to open the wheel, move towards a slice
//! and release to launch. A quick tap (< 250 ms) leaves the wheel open so you can
//! click a slice instead; right-click or Esc cancels.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_WHEEL_OPEN: u32 = WM_APP + 1;
pub const WM_WHEEL_MOVE: u32 = WM_APP + 2; // wparam = x, lparam = y (screen px)
pub const WM_WHEEL_MIDDLE_UP: u32 = WM_APP + 3;
pub const WM_WHEEL_LCLICK: u32 = WM_APP + 4;
pub const WM_WHEEL_RCLICK: u32 = WM_APP + 5;

static TARGET: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Tray "Enabled" switch. When false the hook passes everything straight through.
pub static ENABLED: AtomicBool = AtomicBool::new(true);
/// True from the moment the gesture starts until the UI thread has closed the wheel.
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

// Button-up events whose matching button-down we swallowed must be swallowed too,
// otherwise the application underneath would see an "up" without a "down".
static EAT_M_UP: AtomicBool = AtomicBool::new(false);
static EAT_L_UP: AtomicBool = AtomicBool::new(false);
static EAT_R_UP: AtomicBool = AtomicBool::new(false);

fn post(msg: u32, wparam: usize, lparam: isize) {
    let hwnd = HWND(TARGET.load(Relaxed));
    unsafe {
        let _ = PostMessageW(hwnd, msg, WPARAM(wparam), LPARAM(lparam));
    }
}

fn shift_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16) & 0x8000 != 0 }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ENABLED.load(Relaxed) {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let active = ACTIVE.load(Relaxed);
        match wparam.0 as u32 {
            WM_MBUTTONDOWN => {
                if active {
                    // Pressing middle again while the wheel is open = cancel.
                    EAT_M_UP.store(true, Relaxed);
                    post(WM_WHEEL_RCLICK, 0, 0);
                    return LRESULT(1);
                }
                if shift_down() {
                    ACTIVE.store(true, Relaxed);
                    EAT_M_UP.store(true, Relaxed);
                    post(WM_WHEEL_OPEN, 0, 0);
                    return LRESULT(1);
                }
            }
            WM_MBUTTONUP => {
                if EAT_M_UP.swap(false, Relaxed) {
                    post(WM_WHEEL_MIDDLE_UP, 0, 0);
                    return LRESULT(1);
                }
            }
            WM_MOUSEMOVE => {
                if active {
                    post(
                        WM_WHEEL_MOVE,
                        info.pt.x as isize as usize,
                        info.pt.y as isize,
                    );
                }
            }
            WM_LBUTTONDOWN if active => {
                EAT_L_UP.store(true, Relaxed);
                post(WM_WHEEL_LCLICK, 0, 0);
                return LRESULT(1);
            }
            WM_RBUTTONDOWN if active => {
                EAT_R_UP.store(true, Relaxed);
                post(WM_WHEEL_RCLICK, 0, 0);
                return LRESULT(1);
            }
            WM_LBUTTONUP => {
                if EAT_L_UP.swap(false, Relaxed) {
                    return LRESULT(1);
                }
            }
            WM_RBUTTONUP => {
                if EAT_R_UP.swap(false, Relaxed) {
                    return LRESULT(1);
                }
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL if active => return LRESULT(1),
            _ => {}
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Start the hook thread. `target` is the window that receives the `WM_WHEEL_*` messages.
pub fn start(target: HWND) -> std::io::Result<()> {
    TARGET.store(target.0, Relaxed);
    std::thread::Builder::new()
        .name("mouse-hook".into())
        .stack_size(64 * 1024)
        .spawn(|| unsafe {
            let Ok(module) = GetModuleHandleW(None) else {
                return;
            };
            let Ok(_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), module, 0) else {
                super::message_box("Radial Launcher", "Could not install the mouse hook.");
                return;
            };
            // A low-level hook needs a message loop on the thread that installed it.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })?;
    Ok(())
}
