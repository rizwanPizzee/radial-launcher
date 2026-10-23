//! Windows front-end of the launcher: overlay window, wheel lifecycle, tray and launching.
//!
//! Threads: the UI thread (this file) owns the overlay window and all state; the hook thread
//! (`hook.rs`) only posts messages to it. While idle both threads sit blocked in
//! `GetMessageW` (0 % CPU).
//!
//! Memory: nothing that is only needed while the wheel is open stays alive afterwards.
//! Icons are read from the icon cache file when the wheel opens and dropped when it closes,
//! text is drawn with GDI (no font file loaded), the render buffers are freed on close, and the
//! working set is trimmed a moment after every close. Extracting icons (which loads a lot of
//! shell/imaging code) happens in a separate short-lived process.

mod focus;
mod hook;
mod tray;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::time::{Duration, Instant};

use radial_core::config::{self, Builtin, Config};
use radial_core::iconcache;
use radial_core::wheel::{pick_icons, Session};
use radial_core::win::gdi_text::GdiText;
use radial_core::win::{autostart, icons as shell_icons, ipc, pcwstr, wide};
use tiny_skia::Pixmap;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use hook::*;

const TIMER_FADE: usize = 1;
const TIMER_KEYS: usize = 2;
const TIMER_TRIM: usize = 3;
const TIMER_FOCUS: usize = 4;

/// Safety net: a wheel left open this long closes itself.
const MAX_OPEN: Duration = Duration::from_secs(60);
/// How long we keep watching for a launched program's window (ticks of 120 ms).
const FOCUS_TICKS: u32 = 35;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

pub fn message_box(title: &str, text: &str) {
    let (t, c) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(None, pcwstr(&c), pcwstr(&t), MB_OK | MB_ICONWARNING);
    }
}

struct Open {
    session: Session,
    text: GdiText,
    pm: Pixmap,
    mem_dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    /// Window top-left and wheel centre, in screen pixels.
    origin: POINT,
    centre: POINT,
    return_to: Option<POINT>,
    hovered: Option<usize>,
    opened: Instant,
    sticky: bool,
    /// Fade-in progress 0..=255 and the final opacity.
    fade: u8,
    max_alpha: u8,
}

struct FocusWatch {
    known: Vec<isize>,
    ticks: u32,
    found_at: Option<u32>,
}

struct App {
    cfg: Config,
    open: Option<Open>,
    focus: Option<FocusWatch>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` on the app state. Returns `None` if the state is currently borrowed (a re-entrant
/// call) instead of panicking: with `panic = "abort"` a panic would kill the whole process.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

// ---------------------------------------------------------------- entry points

/// `radial-launcher --refresh-icons`: rebuild the icon cache, tell the running launcher, exit.
pub fn refresh_icons_and_exit() {
    if let Ok(cfg) = config::load_or_create() {
        let _ = shell_icons::refresh_cache(&cfg);
        ipc::notify_launcher_reload();
    }
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
                message_box("Radial Launcher - config error", &format!("Using defaults.\n\n{e}"));
                Config::default()
            }
        };
        let accent = cfg.theme.hover;
        let stale = iconcache::is_stale(&cfg);
        APP.with(|a| *a.borrow_mut() = Some(App { cfg, open: None, focus: None }));

        let hinstance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class = wide(ipc::WHEEL_CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: pcwstr(&class),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            pcwstr(&class),
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
                message_box("Radial Launcher", &format!("Could not create the overlay window: {e}"));
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
        if stale {
            spawn_icon_refresh();
        }
        SetTimer(hwnd, TIMER_TRIM, 3000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        CoUninitialize();
    }
}

fn spawn_icon_refresh() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--refresh-icons").spawn();
    }
}

/// Give back every page of memory the OS can take: they are faulted in again on demand, so the
/// idle working set shrinks to what the process really needs to keep resident.
unsafe fn trim_memory() {
    let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
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
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(monitor, &mut mi);
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    let scale = dpi_x.max(96) as f32 / 96.0;

    let size = (app.cfg.size as f32 * scale).round() as u32;
    let half = (size / 2) as i32;
    let screen_mode = app.cfg.center != "cursor";

    let r = mi.rcMonitor;
    let centre = if screen_mode {
        POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }
    } else {
        POINT {
            x: cursor.x.clamp(r.left + half, (r.right - half).max(r.left + half)),
            y: cursor.y.clamp(r.top + half, (r.bottom - half).max(r.top + half)),
        }
    };
    let origin = POINT { x: centre.x - half, y: centre.y - half };

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

    // Icons come from the cache file and live only as long as this session.
    let mut text = GdiText::new();
    let cache = iconcache::read(&iconcache::cache_path()).unwrap_or_default();
    let icons = pick_icons(&app.cfg, &mut text, |it| cache.get(&iconcache::key_for(&it.path)).cloned());
    drop(cache);
    let session = Session::new(&app.cfg, size, icons, &mut text);

    let mut open = Open {
        session,
        text,
        pm,
        mem_dc,
        bitmap,
        previous,
        bits: bits as *mut u8,
        origin,
        centre,
        return_to: (screen_mode && app.cfg.return_cursor).then_some(cursor),
        hovered: None,
        opened: Instant::now(),
        sticky: false,
        fade: if app.cfg.animation { 0 } else { 255 },
        max_alpha: (app.cfg.opacity as u32 * 255 / 100) as u8,
    };

    if screen_mode {
        let _ = SetCursorPos(centre.x, centre.y);
    }
    redraw(&mut open);
    commit(hwnd, &open);
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, origin.x, origin.y, size as i32, size as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    if open.fade < 255 {
        SetTimer(hwnd, TIMER_FADE, 15, None);
    }
    SetTimer(hwnd, TIMER_KEYS, 40, None);
    app.open = Some(open);
}

/// Render the wheel and copy it into the DIB.
unsafe fn redraw(o: &mut Open) {
    o.session.render(&mut o.pm, o.hovered, &mut o.text);
    let n = o.pm.data().len();
    let dst = std::slice::from_raw_parts_mut(o.bits, n);
    for (s, d) in o.pm.data().chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}

/// Push the DIB to the screen with the current fade/opacity.
unsafe fn commit(hwnd: HWND, o: &Open) {
    let side = o.session.layout().size as i32;
    let size = SIZE { cx: side, cy: side };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: (o.fade as u32 * o.max_alpha as u32 / 255) as u8,
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

/// Hide and free the wheel. Returns the index to launch, if `launch` and something was hovered.
/// The pointer is left exactly where it is (unless the user enabled "return pointer").
unsafe fn close_wheel(app: &mut App, hwnd: HWND, launch: bool) -> Option<usize> {
    let Some(o) = app.open.take() else {
        ACTIVE.store(false, Relaxed);
        return None;
    };
    let _ = KillTimer(hwnd, TIMER_FADE);
    let _ = KillTimer(hwnd, TIMER_KEYS);
    let _ = ShowWindow(hwnd, SW_HIDE);
    SelectObject(o.mem_dc, o.previous);
    let _ = DeleteObject(HGDIOBJ(o.bitmap.0));
    let _ = DeleteDC(o.mem_dc);
    if let Some(p) = o.return_to {
        let _ = SetCursorPos(p.x, p.y);
    }
    ACTIVE.store(false, Relaxed);
    SetTimer(hwnd, TIMER_TRIM, 1500, None);

    let chosen = if launch { o.hovered } else { None };
    // `o` (session, icons, text renderer, render buffer) is dropped here.
    chosen
}

unsafe fn on_move(app: &mut App, hwnd: HWND, x: i32, y: i32) {
    let Some(o) = app.open.as_mut() else { return };
    let hit = o.session.hit_test((x - o.centre.x) as f32, (y - o.centre.y) as f32);
    if hit != o.hovered {
        o.hovered = hit;
        redraw(o);
        commit(hwnd, o);
    }
}

unsafe fn on_middle_up(app: &mut App, hwnd: HWND) -> Option<usize> {
    let tap_ms = app.cfg.tap_ms as u128;
    let o = app.open.as_mut()?;
    if o.opened.elapsed().as_millis() < tap_ms {
        o.sticky = true; // quick tap: stay open, wait for a click
        None
    } else {
        close_wheel(app, hwnd, true)
    }
}

unsafe fn on_timer(app: &mut App, hwnd: HWND, id: usize) {
    match id {
        TIMER_FADE => {
            let Some(o) = app.open.as_mut() else {
                let _ = KillTimer(hwnd, TIMER_FADE);
                return;
            };
            o.fade = o.fade.saturating_add(51);
            commit(hwnd, o);
            if o.fade == 255 {
                let _ = KillTimer(hwnd, TIMER_FADE);
            }
        }
        TIMER_KEYS => {
            let Some(o) = app.open.as_ref() else {
                let _ = KillTimer(hwnd, TIMER_KEYS);
                return;
            };
            let esc = (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16) & 0x8000 != 0;
            if esc || o.opened.elapsed() > MAX_OPEN {
                let _ = close_wheel(app, hwnd, false);
            }
        }
        TIMER_TRIM => {
            let _ = KillTimer(hwnd, TIMER_TRIM);
            if app.open.is_none() {
                trim_memory();
            }
        }
        TIMER_FOCUS => on_focus_tick(app, hwnd),
        _ => {}
    }
}

// ---------------------------------------------------------------- launching

/// Launch item `index`. Called outside of any state borrow, because it may show dialogs.
unsafe fn launch_index(hwnd: HWND, index: usize) {
    let Some((item, focus_it)) = with_app(|a| (a.cfg.items.get(index).cloned(), a.cfg.focus_launched)) else { return };
    let Some(item) = item else { return };

    if item.builtin() == Some(Builtin::Settings) {
        open_settings();
        return;
    }

    if focus_it {
        // Snapshot first, so the window that appears afterwards can be recognised as new.
        let known = focus::snapshot();
        focus::allow_foreground();
        with_app(|a| a.focus = Some(FocusWatch { known, ticks: 0, found_at: None }));
        SetTimer(hwnd, TIMER_FOCUS, 120, None);
    }

    let file = wide(&config::expand_env(item.path.trim()));
    let args = wide(&config::expand_env(&item.args));
    let result = ShellExecuteW(
        None,
        w!("open"),
        pcwstr(&file),
        if item.args.trim().is_empty() { PCWSTR::null() } else { pcwstr(&args) },
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
    // ShellExecute reports failure with a value <= 32.
    if (result.0 as isize) <= 32 {
        let _ = KillTimer(hwnd, TIMER_FOCUS);
        with_app(|a| a.focus = None);
        message_box("Radial Launcher", &format!("Could not open \"{}\".\n\n{}\n\nCheck the path in Settings.", item.name, item.path));
    }
}

/// Raise the window of a program we launched as soon as it appears.
unsafe fn on_focus_tick(app: &mut App, hwnd: HWND) {
    let Some(watch) = app.focus.as_mut() else {
        let _ = KillTimer(hwnd, TIMER_FOCUS);
        return;
    };
    watch.ticks += 1;
    let fresh = focus::new_windows(&watch.known);
    if let Some(&first) = fresh.first() {
        focus::bring_to_front(first);
        watch.known.extend(fresh);
        watch.found_at = Some(watch.ticks);
    }
    // Stop shortly after the last new window (splash screens are followed by the real one), or give up.
    let done = watch.found_at.is_some_and(|t| watch.ticks >= t + 8) || watch.ticks >= FOCUS_TICKS;
    if done {
        let _ = KillTimer(hwnd, TIMER_FOCUS);
        app.focus = None;
    }
}

/// Start the settings app (next to this exe) and let it come to the front.
unsafe fn open_settings() {
    let path = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("radial-settings.exe")));
    match path {
        Some(p) if p.exists() => {
            focus::allow_foreground();
            if let Err(e) = std::process::Command::new(&p).spawn() {
                message_box("Radial Launcher", &format!("Could not start the settings app:\n{e}"));
            }
        }
        _ => message_box(
            "Radial Launcher",
            "radial-settings.exe was not found next to radial-launcher.exe.\n\nPut both files in the same folder.",
        ),
    }
}

// ---------------------------------------------------------------- tray / reload

/// Re-read the config. `quiet` suppresses the error dialog (used when the settings app asks).
unsafe fn reload_config(hwnd: HWND, quiet: bool) {
    match config::load_or_create() {
        Ok(cfg) => {
            let accent = cfg.theme.hover;
            let stale = iconcache::is_stale(&cfg);
            with_app(|app| {
                if app.open.is_some() {
                    let _ = close_wheel(app, hwnd, false);
                }
                app.cfg = cfg;
            });
            tray::refresh(hwnd, accent);
            if stale {
                spawn_icon_refresh();
            }
            SetTimer(hwnd, TIMER_TRIM, 1500, None);
        }
        Err(e) if !quiet => message_box("Radial Launcher - config error", &format!("Keeping the previous settings.\n\n{e}")),
        Err(_) => {}
    }
}

unsafe fn edit_config() {
    let path = config::config_path();
    let args = wide(&format!("\"{}\"", path.display()));
    ShellExecuteW(None, w!("open"), w!("notepad.exe"), pcwstr(&args), PCWSTR::null(), SW_SHOWNORMAL);
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
        tray::CMD_SETTINGS => open_settings(),
        tray::CMD_AUTOSTART => {
            let exe = std::env::current_exe().unwrap_or_default();
            if !autostart::set_enabled(!autostart::is_enabled(), &exe) {
                message_box("Radial Launcher", "Could not change the startup setting.");
            }
        }
        tray::CMD_EDIT => edit_config(),
        tray::CMD_RELOAD => reload_config(hwnd, false),
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
            if let Some(i) = with_app(|a| on_middle_up(a, hwnd)).flatten() {
                launch_index(hwnd, i);
            }
            LRESULT(0)
        }
        WM_WHEEL_LCLICK => {
            if let Some(i) = with_app(|a| close_wheel(a, hwnd, true)).flatten() {
                launch_index(hwnd, i);
            }
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
        ipc::WM_RELOAD => {
            reload_config(hwnd, wparam.0 != 0);
            LRESULT(0)
        }
        tray::WM_TRAY => {
            on_tray(hwnd, lparam);
            LRESULT(0)
        }
        WM_DESTROY => {
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Relaxed) => {
            // Explorer restarted: put the tray icon back.
            let accent = with_app(|a| a.cfg.theme.hover).unwrap_or_default_color();
            tray::add(hwnd, accent);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Tiny helper so the "Explorer restarted" path has a colour even if the state is busy.
trait OrDefaultColor {
    fn unwrap_or_default_color(self) -> radial_core::color::Color;
}

impl OrDefaultColor for Option<radial_core::color::Color> {
    fn unwrap_or_default_color(self) -> radial_core::color::Color {
        self.unwrap_or(radial_core::config::Theme::midnight().hover)
    }
}
