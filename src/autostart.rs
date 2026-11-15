//! "Start with Windows" via the per-user Run registry key.
//!
//! Note: inside an MSIX package this key is virtualized and has no effect;
//! the packaged build will use the MSIX `StartupTask` extension instead.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows_sys::Win32::System::Registry::*;

use crate::util::wide;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "AskeriRuller";

pub fn is_enabled() -> bool {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut size = 0u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size)
    };
    r == ERROR_SUCCESS
}

pub fn set_enabled(enabled: bool) -> bool {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    unsafe {
        if enabled {
            let Some(exe) = exe_path() else { return false };
            let cmd = wide(&format!("\"{exe}\""));
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                cmd.as_ptr() as *const core::ffi::c_void,
                (cmd.len() * 2) as u32,
            ) == ERROR_SUCCESS
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
            r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
        }
    }
}

fn exe_path() -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n]))
}
