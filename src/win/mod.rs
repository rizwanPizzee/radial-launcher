//! Windows front-end: overlay window, wheel lifecycle, tray and launching.
//!
//! Threads: the UI thread (this file) owns the overlay window and all state; the
//! hook thread (`hook.rs`) only posts messages to it. While idle the UI thread
//! sits in `GetMessageW` (0 % CPU) and holds only configuration; icons and render
//! buffers are loaded only while the wheel is open.

mod autostart;
mod hook;
mod icons;
mod tray;
mod util;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering::Relaxed};
use std::time::{Duration, Instant};

use radial_launcher::config::{self, Config};
use radial_launcher::text;
use radial_launcher::wheel::{self, Layout, WheelItem};
use tiny_skia::Pixmap;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CloseHandle, CreateMutexW, GetProcessId, WaitForInputIdle};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};
use windows::Win32::UI::WindowsAndMessaging::*;

use hook::*;
use util::{message_box, wide};

const TIMER_FADE: usize = 1;
const TIMER_KEYS: usize = 2;
const WM_RELOAD_CONFIG: u32 = WM_APP + 21;
/// Releasing the middle button sooner than this keeps the wheel open ("click mode").
const TAP_MS: u128 = 250;
/// Safety net: a wheel left open this long closes itself.
const MAX_OPEN: Duration = Duration::from_secs(60);

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static LAUNCHED_WINDOW: AtomicIsize = AtomicIsize::new(0);

struct Open {
    lay: Layout,
    pm: Pixmap,
    mem_dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    /// Window top-left and wheel centre, in screen pixels.
    origin: POINT,
    centre: POINT,
    restore_cursor: Option<POINT>,
    hovered: Option<usize>,
    opened: Instant,
    sticky: bool,
    alpha: u8,
}

struct App {
    cfg: Config,
    items: Vec<WheelItem>,
    open: Option<Open>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` on the app state. Returns `None` if the state is currently borrowed
/// (a re-entrant call) instead of panicking: with `panic = "abort"` a panic here
/// would kill the whole process.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

pub fn run() {
    unsafe {
        // Single instance.
        let _mutex = CreateMutexW(None, false, w!("Local\\RadialLauncher.SingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let cfg = match config::load_or_create() {
            Ok(c) => c,
            Err(e) => {
                message_box(
                    "Radial Launcher - config error",
                    &format!("Using defaults.\n\n{e}"),
                );
                Config::default()
            }
        };
        let accent = cfg.accent_rgb();
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                cfg,
                items: Vec::new(),
                open: None,
            })
        });

        let hinstance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class = w!("RadialLauncherWheel");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class,
            w!("Radial Launcher"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            hinstance,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                message_box(
                    "Radial Launcher",
                    &format!("Could not create the overlay window: {e}"),
                );
                return;
            }
        };

        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Relaxed);
        tray::add(hwnd, accent);
        if hook::start(hwnd).is_err() {
            message_box("Radial Launcher", "Could not start the mouse hook thread.");
            tray::remove(hwnd);
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        CoUninitialize();
    }
}

// ---------------------------------------------------------------- items

/// Load icons and pre-render labels. The font is read from disk, used, and dropped again.
fn build_items(cfg: &Config) -> Vec<WheelItem> {
    let accent = cfg.accent_rgb();
    let windir = icons::expand("%WINDIR%");
    let font = text::load_font(&format!("{windir}\\Fonts\\segoeui.ttf"));
    // Labels are rendered once at the primary monitor's scale.
    let sys_scale = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;

    cfg.items
        .iter()
        .map(|it| {
            let icon = icons::load_icon(&icons::expand(&it.path), wheel::ICON_PX as i32)
                .map(|p| wheel::fit_icon(&p))
                .unwrap_or_else(|| {
                    text::fallback_icon(font.as_ref(), &it.name, accent, wheel::ICON_PX)
                });
            let label = font
                .as_ref()
                .and_then(|f| text::render_text(f, &text::shorten(&it.name, 22), 26.0 * sys_scale));
            WheelItem { icon, label }
        })
        .collect()
}

// ---------------------------------------------------------------- wheel lifecycle

unsafe fn open_wheel(app: &mut App, hwnd: HWND) {
    if app.cfg.items.is_empty() || app.open.is_some() {
        if app.open.is_none() {
            ACTIVE.store(false, Relaxed);
        }
        return;
    }

    let mut cursor = POINT::default();
    let _ = GetCursorPos(&mut cursor);
    let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = GetMonitorInfoW(monitor, &mut mi);
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    let scale = dpi_x.max(96) as f32 / 96.0;

    let size = (app.cfg.size as f32 * scale).round() as u32;
    let lay = Layout::new(size, scale);
    let half = (size / 2) as i32;
    let screen_mode = app.cfg.center != "cursor";

    let r = mi.rcMonitor;
    let centre = if screen_mode {
        POINT {
            x: (r.left + r.right) / 2,
            y: (r.top + r.bottom) / 2,
        }
    } else {
        POINT {
            x: cursor
                .x
                .clamp(r.left + half, (r.right - half).max(r.left + half)),
            y: cursor
                .y
                .clamp(r.top + half, (r.bottom - half).max(r.top + half)),
        }
    };
    let origin = POINT {
        x: centre.x - half,
        y: centre.y - half,
    };

    // Back buffer: a 32-bit top-down DIB shared with GDI, freed again when the wheel closes.
    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = size as i32;
    bi.bmiHeader.biHeight = -(size as i32);
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB.0;
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = match CreateDIBSection(HDC::default(), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(b) if !bits.is_null() => b,
        _ => {
            ACTIVE.store(false, Relaxed);
            return;
        }
    };
    let Some(pm) = Pixmap::new(size, size) else {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        ACTIVE.store(false, Relaxed);
        return;
    };
    let mem_dc = CreateCompatibleDC(HDC::default());
    let previous = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

    let mut open = Open {
        lay,
        pm,
        mem_dc,
        bitmap,
        previous,
        bits: bits as *mut u8,
        origin,
        centre,
        restore_cursor: (screen_mode && app.cfg.restore_cursor).then_some(cursor),
        hovered: None,
        opened: Instant::now(),
        sticky: false,
        alpha: 0,
    };

    app.items = build_items(&app.cfg);
    if screen_mode {
        let _ = SetCursorPos(centre.x, centre.y);
    }
    redraw(&mut open, &app.items, &app.cfg);
    commit(hwnd, &open);
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        origin.x,
        origin.y,
        size as i32,
        size as i32,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    SetTimer(hwnd, TIMER_FADE, 15, None);
    SetTimer(hwnd, TIMER_KEYS, 40, None);
    app.open = Some(open);
}

/// Render the wheel into the DIB.
unsafe fn redraw(o: &mut Open, items: &[WheelItem], cfg: &Config) {
    wheel::render(
        &mut o.pm,
        &o.lay,
        items,
        o.hovered,
        cfg.accent_rgb(),
        cfg.wheel_rgb(),
        cfg.hub_rgb(),
        &cfg.design,
    );
    let n = (o.lay.size * o.lay.size * 4) as usize;
    let dst = std::slice::from_raw_parts_mut(o.bits, n);
    for (s, d) in o.pm.data().chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}

/// Push the DIB to the screen with the current fade alpha.
unsafe fn commit(hwnd: HWND, o: &Open) {
    let size = SIZE {
        cx: o.lay.size as i32,
        cy: o.lay.size as i32,
    };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: o.alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        HDC::default(),
        Some(&o.origin),
        Some(&size),
        o.mem_dc,
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
}

unsafe fn close_wheel(app: &mut App, hwnd: HWND, launch: bool) {
    let Some(o) = app.open.take() else {
        ACTIVE.store(false, Relaxed);
        return;
    };
    let _ = KillTimer(hwnd, TIMER_FADE);
    let _ = KillTimer(hwnd, TIMER_KEYS);
    let _ = ShowWindow(hwnd, SW_HIDE);
    SelectObject(o.mem_dc, o.previous);
    let _ = DeleteObject(HGDIOBJ(o.bitmap.0));
    let _ = DeleteDC(o.mem_dc);
    if let Some(p) = o.restore_cursor {
        let _ = SetCursorPos(p.x, p.y);
    }
    ACTIVE.store(false, Relaxed);
    app.items.clear();

    if launch {
        if let Some(item) = o.hovered.and_then(|i| app.cfg.items.get(i)) {
            launch_item(&item.path, &item.args, app.cfg.focus_app);
        }
    }
    // `o.pm` (the render buffer) is dropped here.
}

unsafe extern "system" fn find_process_window(hwnd: HWND, process: LPARAM) -> BOOL {
    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    if process_id as isize == process.0 && IsWindowVisible(hwnd).as_bool() {
        let owner = GetWindow(hwnd, GW_OWNER);
        if owner.0.is_null() {
            LAUNCHED_WINDOW.store(hwnd.0 as isize, Relaxed);
            return false.into();
        }
    }
    true.into()
}

fn launch_item(path: &str, args: &str, focus_app: bool) {
    unsafe {
        // Let the program we are about to start take the foreground.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let file = wide(&icons::expand(path));
        let params = wide(&icons::expand(args));
        let mut execute = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("open"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: if args.trim().is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params.as_ptr())
            },
            nShow: SW_SHOWNORMAL,
            ..Default::default()
        };
        if !ShellExecuteExW(&mut execute).as_bool() {
            message_box(
                "Radial Launcher",
                &format!("Could not launch:\n{path}\n\nWindows error: {}", GetLastError().0),
            );
            return;
        }
        if !execute.hProcess.is_invalid() {
            if focus_app {
                let _ = WaitForInputIdle(execute.hProcess, 3000);
                let process_id = GetProcessId(execute.hProcess);
                if process_id != 0 {
                    let _ = AllowSetForegroundWindow(process_id);
                    LAUNCHED_WINDOW.store(0, Relaxed);
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < deadline {
                        let _ = EnumWindows(
                            Some(find_process_window),
                            LPARAM(process_id as isize),
                        );
                        let window = LAUNCHED_WINDOW.load(Relaxed);
                        if window != 0 {
                            let window = HWND(window as *mut c_void);
                            let _ = ShowWindow(window, SW_RESTORE);
                            let _ = SetForegroundWindow(window);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            }
            let _ = CloseHandle(execute.hProcess);
        }
    }
}

unsafe fn on_move(app: &mut App, hwnd: HWND, x: i32, y: i32) {
    let n = app.items.len();
    let Some(o) = app.open.as_mut() else { return };
    let hit = o.lay.hit_test_design(
        (x - o.centre.x) as f32,
        (y - o.centre.y) as f32,
        n,
        &app.cfg.design,
    );
    if hit != o.hovered {
        o.hovered = hit;
        redraw(o, &app.items, &app.cfg);
        commit(hwnd, o);
    }
}

unsafe fn on_middle_up(app: &mut App, hwnd: HWND) {
    let Some(o) = app.open.as_mut() else { return };
    if o.opened.elapsed().as_millis() < TAP_MS {
        o.sticky = true; // quick tap: stay open, wait for a click
    } else {
        close_wheel(app, hwnd, true);
    }
}

unsafe fn on_timer(app: &mut App, hwnd: HWND, id: usize) {
    let Some(o) = app.open.as_mut() else { return };
    match id {
        TIMER_FADE => {
            o.alpha = o.alpha.saturating_add(51);
            commit(hwnd, o);
            if o.alpha == 255 {
                let _ = KillTimer(hwnd, TIMER_FADE);
            }
        }
        TIMER_KEYS => {
            let esc = (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16) & 0x8000 != 0;
            if esc || o.opened.elapsed() > MAX_OPEN {
                close_wheel(app, hwnd, false);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- tray commands

unsafe fn reload_config(hwnd: HWND) {
    match config::load_or_create() {
        Ok(cfg) => {
            let accent = cfg.accent_rgb();
            with_app(|app| {
                if app.open.is_some() {
                    close_wheel(app, hwnd, false);
                }
                app.cfg = cfg;
                app.items.clear();
            });
            tray::remove(hwnd);
            tray::add(hwnd, accent);
        }
        Err(e) => message_box(
            "Radial Launcher - config error",
            &format!("Keeping the previous settings.\n\n{e}"),
        ),
    }
}

unsafe fn open_settings() {
    let settings_exe = std::env::current_exe()
        .map(|exe| exe.with_file_name("radial-launcher-settings.exe"));
    match settings_exe.and_then(|exe| std::process::Command::new(exe).spawn()) {
        Ok(_) => {}
        Err(e) => message_box(
            "Radial Launcher",
            &format!(
                "Could not open Radial Launcher Settings.\nMake sure radial-launcher-settings.exe is next to radial-launcher.exe.\n\n{e}"
            ),
        ),
    }
}

unsafe fn on_tray(hwnd: HWND, lparam: LPARAM) {
    let event = lparam.0 as u32;
    if event != WM_RBUTTONUP && event != WM_CONTEXTMENU && event != WM_LBUTTONUP {
        return;
    }
    // No borrow of APP is held while the menu runs its own message loop.
    let enabled = ENABLED.load(Relaxed);
    match tray::show_menu(hwnd, enabled, autostart::is_enabled()) {
        tray::CMD_ENABLED => {
            ENABLED.store(!enabled, Relaxed);
            if enabled {
                // Disabling while the wheel is open must not leave it stuck on screen.
                with_app(|app| close_wheel(app, hwnd, false));
            }
        }
        tray::CMD_AUTOSTART => {
            if !autostart::set_enabled(!autostart::is_enabled()) {
                message_box("Radial Launcher", "Could not change the startup setting.");
            }
        }
        tray::CMD_SETTINGS => open_settings(),
        tray::CMD_RELOAD => reload_config(hwnd),
        tray::CMD_QUIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- window procedure

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_WHEEL_OPEN => {
            if with_app(|a| open_wheel(a, hwnd)).is_none() {
                ACTIVE.store(false, Relaxed);
            }
            LRESULT(0)
        }
        WM_WHEEL_MOVE => {
            let (x, y) = (wparam.0 as isize as i32, lparam.0 as i32);
            with_app(|a| on_move(a, hwnd, x, y));
            LRESULT(0)
        }
        WM_WHEEL_MIDDLE_UP => {
            with_app(|a| on_middle_up(a, hwnd));
            LRESULT(0)
        }
        WM_WHEEL_LCLICK => {
            with_app(|a| close_wheel(a, hwnd, true));
            LRESULT(0)
        }
        WM_WHEEL_RCLICK => {
            with_app(|a| close_wheel(a, hwnd, false));
            LRESULT(0)
        }
        WM_TIMER => {
            with_app(|a| on_timer(a, hwnd, wparam.0));
            LRESULT(0)
        }
        tray::WM_TRAY => {
            on_tray(hwnd, lparam);
            LRESULT(0)
        }
        WM_RELOAD_CONFIG => {
            reload_config(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Relaxed) => {
            // Explorer restarted: put the tray icon back.
            let accent = with_app(|a| a.cfg.accent_rgb()).unwrap_or([76, 154, 255]);
            tray::add(hwnd, accent);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
