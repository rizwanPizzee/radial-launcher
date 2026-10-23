//! "Start with Windows" via HKCU\...\Run (no admin rights needed).

use super::{pcwstr, wide};
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "RadialLauncher";

unsafe fn open_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let sub = wide(RUN_KEY);
    let mut key = HKEY::default();
    let rc = RegCreateKeyExW(
        HKEY_CURRENT_USER,
        pcwstr(&sub),
        0,
        PCWSTR::null(),
        REG_OPTION_NON_VOLATILE,
        access,
        None,
        &mut key,
        None,
    );
    (rc == ERROR_SUCCESS).then_some(key)
}

pub fn is_enabled() -> bool {
    unsafe {
        let Some(key) = open_key(KEY_QUERY_VALUE) else { return false };
        let name = wide(VALUE);
        let rc = RegQueryValueExW(key, pcwstr(&name), None, None, None, None);
        let _ = RegCloseKey(key);
        rc == ERROR_SUCCESS
    }
}

/// Point the Run entry at `exe` (the launcher), or remove it. Returns false on failure.
pub fn set_enabled(on: bool, exe: &std::path::Path) -> bool {
    unsafe {
        let Some(key) = open_key(KEY_SET_VALUE | KEY_QUERY_VALUE) else { return false };
        let name = wide(VALUE);
        let ok = if on {
            let cmd = wide(&format!("\"{}\"", exe.display()));
            let bytes = std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2);
            RegSetValueExW(key, pcwstr(&name), 0, REG_SZ, Some(bytes)) == ERROR_SUCCESS
        } else {
            let rc = RegDeleteValueW(key, pcwstr(&name));
            rc == ERROR_SUCCESS || RegQueryValueExW(key, pcwstr(&name), None, None, None, None) != ERROR_SUCCESS
        };
        let _ = RegCloseKey(key);
        ok
    }
}
