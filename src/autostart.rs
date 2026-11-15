//! "Start with Windows".
//!
//! * Unpackaged (plain .exe): the per-user `Run` registry key.
//! * Packaged (MSIX / Microsoft Store): the `StartupTask` declared in
//!   `packaging\AppxManifest.xml`. The Run key is virtualized inside a
//!   package and would have no effect there.

use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows_sys::Win32::System::Registry::*;

use crate::util::wide;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "AskeriRuller";

/// True when running from an MSIX package (e.g. installed from the Store).
pub fn is_packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let mut len = 0u32;
        let r = unsafe { GetCurrentPackageFullName(&mut len, null_mut()) };
        r != APPMODEL_ERROR_NO_PACKAGE
    })
}

pub fn is_enabled() -> bool {
    if is_packaged() {
        packaged::is_enabled()
    } else {
        registry_is_enabled()
    }
}

/// Turns autostart on or off. The error is a message for the user.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if is_packaged() {
        packaged::set_enabled(enabled)
    } else if registry_set_enabled(enabled) {
        Ok(())
    } else {
        Err("Could not change the Start with Windows setting.".into())
    }
}

mod packaged {
    use windows::core::HSTRING;
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    /// Must match `TaskId` in AppxManifest.xml.
    const TASK_ID: &str = "AskeriRullerStartup";

    /// Runs a WinRT call on a short-lived MTA thread, so blocking on the
    /// async operation can never stall or deadlock the UI thread's apartment.
    fn on_mta<T: Send + 'static>(f: impl FnOnce() -> windows::core::Result<T> + Send + 'static) -> windows::core::Result<T> {
        std::thread::spawn(move || {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            f()
        })
        .join()
        .unwrap_or_else(|_| Err(windows::core::Error::empty()))
    }

    fn state() -> windows::core::Result<StartupTaskState> {
        on_mta(|| StartupTask::GetAsync(&HSTRING::from(TASK_ID))?.join()?.State())
    }

    pub fn is_enabled() -> bool {
        matches!(state(), Ok(StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy))
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let result = on_mta(move || {
            let task = StartupTask::GetAsync(&HSTRING::from(TASK_ID))?.join()?;
            if enabled {
                task.RequestEnableAsync()?.join()
            } else {
                task.Disable()?;
                task.State()
            }
        });
        match result {
            Ok(StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy) if enabled => Ok(()),
            Ok(StartupTaskState::Disabled | StartupTaskState::DisabledByUser) if !enabled => Ok(()),
            Ok(StartupTaskState::DisabledByUser) => Err(
                "Start with Windows was turned off in Windows Settings.\n\
                 Turn it on in Settings > Apps > Startup."
                    .into(),
            ),
            Ok(StartupTaskState::DisabledByPolicy | StartupTaskState::EnabledByPolicy) => {
                Err("Start with Windows is controlled by your organization's policy.".into())
            }
            Ok(_) => Err("Could not change the Start with Windows setting.".into()),
            Err(e) => Err(format!("Could not change the Start with Windows setting.\n{}", e.message())),
        }
    }
}

fn registry_is_enabled() -> bool {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut size = 0u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size)
    };
    r == ERROR_SUCCESS
}

fn registry_set_enabled(enabled: bool) -> bool {
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
