//! Windows implementations: shell icons, GDI text, file dialogs, Start-menu list, autostart.

use super::{CommandExt, StartApp};
use radial_core::text::TextRenderer;
use radial_core::win::gdi_text::GdiText;
use radial_core::win::{autostart, icons, ipc, pcwstr, wide};
use std::ffi::c_void;
use tiny_skia::Pixmap;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Threading::{AttachThreadInput, CreateMutexW, GetCurrentThreadId};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_NODEREFERENCELINKS, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WINDOW_TITLE: &str = "Radial Launcher Settings";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keeps COM initialised on the UI thread for the lifetime of the app.
pub struct Session {
    _com: icons::ComGuard,
}

impl Session {
    pub fn new() -> Session {
        Session { _com: icons::ComGuard::new() }
    }
}

pub fn text_renderer() -> Box<dyn TextRenderer> {
    Box::new(GdiText::new())
}

pub fn load_icon(path: &str) -> Option<Pixmap> {
    icons::load_icon(path)
}

/// Fonts to add behind egui's built-in one: Segoe UI covers Arabic, Cyrillic, Greek and more,
/// Segoe UI Symbol and Yu Gothic / Microsoft YaHei cover symbols and CJK names when present.
pub fn system_fonts() -> Vec<(String, Vec<u8>)> {
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    ["segoeui.ttf", "seguisym.ttf", "msyh.ttc", "YuGothR.ttc"]
        .iter()
        .filter_map(|f| std::fs::read(format!("{dir}\\Fonts\\{f}")).ok().map(|d| (format!("sys-{f}"), d)))
        .collect()
}

fn pick(folder: bool) -> Option<String> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut opts = dlg.GetOptions().ok()? | FOS_FORCEFILESYSTEM | FOS_NODEREFERENCELINKS;
        if folder {
            opts |= FOS_PICKFOLDERS;
        }
        dlg.SetOptions(opts).ok()?;
        if !folder {
            let specs = [
                COMDLG_FILTERSPEC { pszName: w!("Programs and shortcuts"), pszSpec: w!("*.exe;*.lnk;*.bat;*.cmd;*.msc") },
                COMDLG_FILTERSPEC { pszName: w!("All files"), pszSpec: w!("*.*") },
            ];
            dlg.SetFileTypes(&specs).ok()?;
        }
        dlg.Show(GetActiveWindow()).ok()?;
        let item = dlg.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const c_void));
        path
    }
}

pub fn pick_file() -> Option<String> {
    pick(false)
}

pub fn pick_folder() -> Option<String> {
    pick(true)
}

/// Installed apps from the Start menu (classic and Store apps) via PowerShell's `Get-StartApps`.
/// Blocking, takes a second or two: call it from a worker thread.
pub fn list_start_apps() -> Result<Vec<StartApp>, String> {
    let script = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
                  Get-StartApps | Sort-Object Name | Select-Object Name,AppID | ConvertTo-Json -Compress";
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Could not run PowerShell: {e}"))?;
    if !out.status.success() {
        return Err("PowerShell could not list the apps.".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(text.trim().trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Unexpected answer from PowerShell: {e}"))?;
    let rows = match value {
        serde_json::Value::Array(a) => a,
        v @ serde_json::Value::Object(_) => vec![v],
        _ => Vec::new(),
    };
    let mut apps: Vec<StartApp> = rows
        .iter()
        .filter_map(|r| {
            let name = r.get("Name")?.as_str()?.trim().to_owned();
            let id = r.get("AppID")?.as_str()?.trim().to_owned();
            (!name.is_empty() && !id.is_empty()).then(|| StartApp { name, path: format!("shell:AppsFolder\\{id}") })
        })
        .collect();
    apps.dedup_by(|a, b| a.path == b.path);
    Ok(apps)
}

pub fn autostart_enabled() -> bool {
    autostart::is_enabled()
}

pub fn set_autostart(on: bool, exe: &std::path::Path) -> bool {
    autostart::set_enabled(on, exe)
}

pub fn notify_launcher() -> bool {
    ipc::notify_launcher_reload()
}

/// True if this is the only settings window; otherwise the existing one is raised.
pub fn first_instance() -> bool {
    unsafe {
        let _ = CreateMutexW(None, false, w!("Local\\RadialLauncher.Settings"));
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return true;
        }
        let title = wide(WINDOW_TITLE);
        if let Ok(hwnd) = FindWindowW(PCWSTR::null(), pcwstr(&title)) {
            bring_to_front(hwnd);
        }
        false
    }
}

unsafe fn bring_to_front(hwnd: HWND) {
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    let me = GetCurrentThreadId();
    let fg = GetForegroundWindow();
    let fg_thread = if fg.0.is_null() { 0 } else { GetWindowThreadProcessId(fg, None) };
    let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);
    if attached {
        let _ = AttachThreadInput(me, fg_thread, false);
    }
}

/// Make sure our own window is in front (it is started by the launcher in the background).
pub fn raise_self() {
    unsafe {
        let title = wide(WINDOW_TITLE);
        if let Ok(hwnd) = FindWindowW(PCWSTR::null(), pcwstr(&title)) {
            bring_to_front(hwnd);
        }
    }
}
