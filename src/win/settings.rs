//! On-demand native settings window. It runs in a separate process mode so the
//! editor and its controls consume no memory while the launcher is idle.

use super::util::{message_box, wide};
use super::WM_RELOAD_CONFIG;
use radial_launcher::config::{self, Config, ItemConfig, MAX_ITEMS};
use std::cell::RefCell;
use std::ffi::c_void;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{GetSysColorBrush, COLOR_WINDOW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const ID_DESIGN: i32 = 1;
const ID_SIZE: i32 = 2;
const ID_CENTER: i32 = 3;
const ID_ACCENT: i32 = 4;
const ID_WHEEL: i32 = 5;
const ID_HUB: i32 = 6;
const ID_RESTORE: i32 = 7;
const ID_FOCUS: i32 = 8;
const ID_SAVE: i32 = 500;
const ID_CANCEL: i32 = 501;
const ROW_BASE: i32 = 100;
const ROW_STRIDE: i32 = 4;

thread_local! {
    static SETTINGS_CONFIG: RefCell<Config> = RefCell::new(Config::default());
}

fn control(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: i32,
) -> HWND {
    let text = wide(text);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | style,
            x,
            y,
            width,
            height,
            parent,
            HMENU(id as *mut c_void),
            GetModuleHandleW(None).map(Into::into).unwrap_or_default(),
            None,
        )
        .unwrap_or_default()
    }
}

fn label(parent: HWND, text: &str, x: i32, y: i32, width: i32) {
    let _ = control(
        parent,
        w!("STATIC"),
        text,
        WINDOW_STYLE(0),
        x,
        y,
        width,
        22,
        0,
    );
}

fn set_text(parent: HWND, id: i32, value: &str) {
    let value = wide(value);
    unsafe {
        let _ = SetWindowTextW(GetDlgItem(parent, id), PCWSTR(value.as_ptr()));
    }
}

fn text(parent: HWND, id: i32) -> String {
    let mut value = vec![0u16; 4096];
    unsafe {
        let len = GetWindowTextW(GetDlgItem(parent, id), &mut value);
        String::from_utf16_lossy(&value[..len.max(0) as usize])
    }
}

fn combo(parent: HWND, id: i32, x: i32, y: i32, width: i32, entries: &[&str], selected: usize) {
    let hwnd = control(
        parent,
        w!("COMBOBOX"),
        "",
        WS_VSCROLL | CBS_DROPDOWNLIST,
        x,
        y,
        width,
        150,
        id,
    );
    for entry in entries {
        let entry = wide(entry);
        unsafe {
            let _ = SendMessageW(
                hwnd,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(entry.as_ptr() as isize),
            );
        }
    }
    unsafe {
        let _ = SendMessageW(hwnd, CB_SETCURSEL, WPARAM(selected), LPARAM(0));
    }
}

unsafe fn create_form(hwnd: HWND) {
    let cfg = SETTINGS_CONFIG.with(|stored| stored.borrow().clone());
    label(hwnd, "Wheel design", 20, 18, 90);
    combo(
        hwnd,
        ID_DESIGN,
        110,
        14,
        145,
        &["Segments", "Band", "Orbs", "Halo", "Arc", "Disc"],
        match cfg.design.as_str() {
            "band" => 1,
            "orbs" => 2,
            "halo" => 3,
            "arc" => 4,
            "disc" => 5,
            _ => 0,
        },
    );
    label(hwnd, "Size", 275, 18, 45);
    let _ = control(
        hwnd,
        w!("EDIT"),
        "",
        WS_BORDER | ES_AUTOHSCROLL,
        320,
        14,
        70,
        25,
        ID_SIZE,
    );
    set_text(hwnd, ID_SIZE, &cfg.size.to_string());
    label(hwnd, "Open at", 415, 18, 60);
    combo(
        hwnd,
        ID_CENTER,
        480,
        14,
        145,
        &["Screen center", "Pointer"],
        if cfg.center == "cursor" { 1 } else { 0 },
    );

    label(hwnd, "Highlight color", 20, 58, 110);
    let _ = control(hwnd, w!("EDIT"), "", WS_BORDER | ES_AUTOHSCROLL, 130, 54, 100, 25, ID_ACCENT);
    set_text(hwnd, ID_ACCENT, &cfg.accent);
    label(hwnd, "Slice color", 260, 58, 90);
    let _ = control(hwnd, w!("EDIT"), "", WS_BORDER | ES_AUTOHSCROLL, 350, 54, 100, 25, ID_WHEEL);
    set_text(hwnd, ID_WHEEL, &cfg.wheel_color);
    label(hwnd, "Center color", 480, 58, 90);
    let _ = control(hwnd, w!("EDIT"), "", WS_BORDER | ES_AUTOHSCROLL, 570, 54, 100, 25, ID_HUB);
    set_text(hwnd, ID_HUB, &cfg.hub_color);

    let restore = control(
        hwnd,
        w!("BUTTON"),
        "Return pointer to its original position",
        BS_AUTOCHECKBOX,
        20,
        94,
        290,
        24,
        ID_RESTORE,
    );
    let focus = control(
        hwnd,
        w!("BUTTON"),
        "Bring launched app to the foreground",
        BS_AUTOCHECKBOX,
        340,
        94,
        300,
        24,
        ID_FOCUS,
    );
    let _ = SendMessageW(
        restore,
        BM_SETCHECK,
        WPARAM(if cfg.restore_cursor { 1 } else { 0 }),
        LPARAM(0),
    );
    let _ = SendMessageW(
        focus,
        BM_SETCHECK,
        WPARAM(if cfg.focus_app { 1 } else { 0 }),
        LPARAM(0),
    );

    label(hwnd, "Wheel apps and files (blank rows are ignored; use Browse to choose a file)", 20, 130, 700);
    label(hwnd, "Name", 60, 154, 150);
    label(hwnd, "Path", 225, 154, 480);
    label(hwnd, "Arguments (optional)", 735, 154, 170);
    for row in 0..MAX_ITEMS {
        let y = 178 + row as i32 * 31;
        let base = ROW_BASE + row as i32 * ROW_STRIDE;
        label(hwnd, &format!("{:02}", row + 1), 20, y + 3, 30);
        let _ = control(
            hwnd,
            w!("EDIT"),
            "",
            WS_BORDER | ES_AUTOHSCROLL,
            55,
            y,
            155,
            25,
            base,
        );
        let _ = control(
            hwnd,
            w!("EDIT"),
            "",
            WS_BORDER | ES_AUTOHSCROLL,
            220,
            y,
            500,
            25,
            base + 1,
        );
        let _ = control(
            hwnd,
            w!("EDIT"),
            "",
            WS_BORDER | ES_AUTOHSCROLL,
            730,
            y,
            175,
            25,
            base + 2,
        );
        let _ = control(
            hwnd,
            w!("BUTTON"),
            "Browse...",
            WINDOW_STYLE(0),
            915,
            y,
            85,
            25,
            base + 3,
        );
        if let Some(item) = cfg.items.get(row) {
            set_text(hwnd, base, &item.name);
            set_text(hwnd, base + 1, &item.path);
            set_text(hwnd, base + 2, &item.args);
        }
    }
    let _ = control(hwnd, w!("BUTTON"), "Save settings", BS_DEFPUSHBUTTON, 790, 570, 105, 32, ID_SAVE);
    let _ = control(hwnd, w!("BUTTON"), "Cancel", WINDOW_STYLE(0), 905, 570, 95, 32, ID_CANCEL);
    label(hwnd, "Changes are saved to your user config and used the next time the wheel opens.", 20, 578, 650);
}

unsafe fn browse(hwnd: HWND, row: usize) {
    let mut file = vec![0u16; 32768];
    let filter = wide("Applications and files\0*.exe;*.lnk;*.*\0All files\0*.*\0");
    let mut dialog = OPENFILENAMEW::default();
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = hwnd;
    dialog.lpstrFilter = PCWSTR(filter.as_ptr());
    dialog.lpstrFile = PWSTR(file.as_mut_ptr());
    dialog.nMaxFile = file.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if GetOpenFileNameW(&mut dialog).as_bool() {
        let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
        let path = String::from_utf16_lossy(&file[..end]);
        let id = ROW_BASE + row as i32 * ROW_STRIDE + 1;
        set_text(hwnd, id, &path);
        if text(hwnd, ROW_BASE + row as i32 * ROW_STRIDE).trim().is_empty() {
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("New item");
            set_text(hwnd, ROW_BASE + row as i32 * ROW_STRIDE, name);
        }
    }
}

fn valid_color(value: &str) -> bool {
    let hex = value.trim().trim_start_matches('#');
    hex.len() == 6 && u32::from_str_radix(hex, 16).is_ok()
}

unsafe fn save_settings(hwnd: HWND) {
    let mut cfg = SETTINGS_CONFIG.with(|stored| stored.borrow().clone());
    let designs = ["segments", "band", "orbs", "halo", "arc", "disc"];
    let design_index = SendMessageW(GetDlgItem(hwnd, ID_DESIGN), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let center_index = SendMessageW(GetDlgItem(hwnd, ID_CENTER), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let size = text(hwnd, ID_SIZE);
    let colors = [text(hwnd, ID_ACCENT), text(hwnd, ID_WHEEL), text(hwnd, ID_HUB)];
    if size.parse::<u32>().is_err() || colors.iter().any(|color| !valid_color(color)) {
        message_box(
            "Radial Launcher settings",
            "Enter a wheel size from 300 to 900 and colors in #RRGGBB format.",
        );
        return;
    }
    let size = size.parse::<u32>().unwrap_or(520);
    if !(300..=900).contains(&size) {
        message_box(
            "Radial Launcher settings",
            "Wheel size must be between 300 and 900.",
        );
        return;
    }
    cfg.size = size;
    cfg.design = designs
        .get(design_index.max(0) as usize)
        .unwrap_or(&"segments")
        .to_string();
    cfg.center = if center_index == 1 { "cursor" } else { "screen" }.into();
    cfg.accent = colors[0].clone();
    cfg.wheel_color = colors[1].clone();
    cfg.hub_color = colors[2].clone();
    cfg.restore_cursor =
        SendMessageW(GetDlgItem(hwnd, ID_RESTORE), BM_GETCHECK, WPARAM(0), LPARAM(0)).0
            == 1;
    cfg.focus_app =
        SendMessageW(GetDlgItem(hwnd, ID_FOCUS), BM_GETCHECK, WPARAM(0), LPARAM(0)).0
            == 1;
    cfg.items = (0..MAX_ITEMS)
        .filter_map(|row| {
            let base = ROW_BASE + row as i32 * ROW_STRIDE;
            let name = text(hwnd, base).trim().to_string();
            let path = text(hwnd, base + 1).trim().to_string();
            if name.is_empty() || path.is_empty() {
                return None;
            }
            Some(ItemConfig {
                name,
                path,
                args: text(hwnd, base + 2).trim().to_string(),
            })
        })
        .collect();
    if cfg.items.is_empty() {
        message_box("Radial Launcher settings", "Add at least one app or file before saving.");
        return;
    }
    match config::save(&cfg) {
        Ok(()) => {
            let launcher = FindWindowW(w!("RadialLauncherWheel"), PCWSTR::null());
            if !launcher.0.is_null() {
                let _ = PostMessageW(launcher, WM_RELOAD_CONFIG, WPARAM(0), LPARAM(0));
            }
            let _ = DestroyWindow(hwnd);
        }
        Err(error) => message_box(
            "Radial Launcher - settings error",
            &format!("Could not save settings:\n\n{error}"),
        ),
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            create_form(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            match id {
                ID_SAVE => save_settings(hwnd),
                ID_CANCEL => {
                    let _ = DestroyWindow(hwnd);
                }
                id if (ROW_BASE..ROW_BASE + MAX_ITEMS as i32 * ROW_STRIDE).contains(&id)
                    && (id - ROW_BASE) % ROW_STRIDE == 3 =>
                {
                    browse(hwnd, ((id - ROW_BASE) / ROW_STRIDE) as usize)
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn run() {
    let cfg = match config::load_or_create() {
        Ok(cfg) => cfg,
        Err(error) => {
            message_box(
                "Radial Launcher - config error",
                &format!("Could not load settings:\n\n{error}"),
            );
            Config::default()
        }
    };
    SETTINGS_CONFIG.with(|stored| *stored.borrow_mut() = cfg);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class = w!("RadialLauncherSettings");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let window = CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            w!("Radial Launcher Settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1040,
            660,
            None,
            None,
            instance,
            None,
        );
        let Ok(window) = window else {
            message_box("Radial Launcher", "Could not open the settings window.");
            return;
        };
        let _ = ShowWindow(window, SW_SHOW);
        let _ = UpdateWindow(window);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
