//! Global low-level mouse hook on its own thread.
//!
//! The hook callback does as little as possible (it runs for *every* mouse
//! event on the system and Windows removes hooks that answer too slowly): a few
//! atomic loads, and a `PostMessage` to the UI thread when something relevant
//! happens. All drawing and launching happens on the UI thread.
//!
//! Gesture: hold the trigger (by default **Shift + Middle button**) to open the wheel, move
//! towards a slice and release to launch. A quick tap leaves the wheel open so you can click a
//! slice instead; right-click or Esc cancels; the mouse wheel flips pages.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering::Relaxed};

use radial_core::config::{Button, Modifier};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_WHEEL_OPEN: u32 = WM_APP + 1;
pub const WM_WHEEL_MOVE: u32 = WM_APP + 2; // wparam = x, lparam = y (screen px)
pub const WM_WHEEL_RELEASE: u32 = WM_APP + 3; // the trigger button was released
pub const WM_WHEEL_LCLICK: u32 = WM_APP + 4;
pub const WM_WHEEL_RCLICK: u32 = WM_APP + 5;
pub const WM_WHEEL_SCROLL: u32 = WM_APP + 6; // wparam = wheel delta (i16, sign-extended)

static TARGET: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Tray "Enabled" switch. When false the hook passes everything straight through.
pub static ENABLED: AtomicBool = AtomicBool::new(true);
/// True from the moment the gesture starts until the UI thread has closed the wheel.
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Set while a full-screen app is in front and the user asked to stay out of its way.
pub static PAUSED: AtomicBool = AtomicBool::new(false);

static TRIGGER_BUTTON: AtomicU8 = AtomicU8::new(0); // 0 middle, 1 X1, 2 X2
static TRIGGER_KEY: AtomicU8 = AtomicU8::new(1); // 0 none, 1 shift, 2 ctrl, 3 alt

/// Change the gesture that opens the wheel (takes effect immediately).
pub fn set_trigger(button: Button, key: Modifier) {
    TRIGGER_BUTTON.store(
        match button {
            Button::Middle => 0,
            Button::X1 => 1,
            Button::X2 => 2,
        },
        Relaxed,
    );
    TRIGGER_KEY.store(
        match key {
            Modifier::None => 0,
            Modifier::Shift => 1,
            Modifier::Ctrl => 2,
            Modifier::Alt => 3,
        },
        Relaxed,
    );
}

// Button-up events whose matching button-down we swallowed must be swallowed too,
// otherwise the application underneath would see an "up" without a "down".
static EAT_TRIGGER_UP: AtomicBool = AtomicBool::new(false);
static EAT_L_UP: AtomicBool = AtomicBool::new(false);
static EAT_R_UP: AtomicBool = AtomicBool::new(false);

fn post(msg: u32, wparam: usize, lparam: isize) {
    let hwnd = HWND(TARGET.load(Relaxed));
    unsafe {
        let _ = PostMessageW(hwnd, msg, WPARAM(wparam), LPARAM(lparam));
    }
}

fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 }
}

/// Is the key that belongs to the trigger held?
fn modifier_down() -> bool {
    match TRIGGER_KEY.load(Relaxed) {
        0 => true,
        1 => key_down(VK_SHIFT.0 as i32),
        2 => key_down(VK_CONTROL.0 as i32),
        _ => key_down(VK_MENU.0 as i32),
    }
}

/// Which of our three buttons does this mouse message belong to (0 middle, 1 X1, 2 X2)?
fn button_of(msg: u32, info: &MSLLHOOKSTRUCT) -> Option<u8> {
    match msg {
        WM_MBUTTONDOWN | WM_MBUTTONUP => Some(0),
        WM_XBUTTONDOWN | WM_XBUTTONUP => match info.mouseData >> 16 {
            1 => Some(1),
            2 => Some(2),
            _ => None,
        },
        _ => None,
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ENABLED.load(Relaxed) {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let active = ACTIVE.load(Relaxed);
        // A wheel that is already open keeps working even if a full-screen app has just appeared.
        if !active && PAUSED.load(Relaxed) {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let msg = wparam.0 as u32;
        match msg {
            WM_MBUTTONDOWN | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                if button_of(msg, info) == Some(TRIGGER_BUTTON.load(Relaxed)) {
                    if matches!(msg, WM_MBUTTONDOWN | WM_XBUTTONDOWN) {
                        if active {
                            // Pressing the trigger again while the wheel is open = cancel.
                            EAT_TRIGGER_UP.store(true, Relaxed);
                            post(WM_WHEEL_RCLICK, 0, 0);
                            return LRESULT(1);
                        }
                        if modifier_down() {
                            ACTIVE.store(true, Relaxed);
                            EAT_TRIGGER_UP.store(true, Relaxed);
                            post(WM_WHEEL_OPEN, 0, 0);
                            return LRESULT(1);
                        }
                    } else if EAT_TRIGGER_UP.swap(false, Relaxed) {
                        post(WM_WHEEL_RELEASE, 0, 0);
                        return LRESULT(1);
                    }
                }
            }
            WM_MOUSEMOVE => {
                if active {
                    post(WM_WHEEL_MOVE, info.pt.x as isize as usize, info.pt.y as isize);
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
            // While the wheel is open the scroll wheel flips pages instead of scrolling the app below.
            WM_MOUSEWHEEL if active => {
                let delta = (info.mouseData >> 16) as u16 as i16;
                post(WM_WHEEL_SCROLL, delta as isize as usize, 0);
                return LRESULT(1);
            }
            WM_MOUSEHWHEEL if active => return LRESULT(1),
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
