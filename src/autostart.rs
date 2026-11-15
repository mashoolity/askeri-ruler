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
/// Also used by installer\askeri-ruler.iss — keep in sync.
const VALUE_NAME: &str = "AskeriRuller";
/// Command-line flag added to the Run key entry (and by the installer).
pub const AUTOSTART_ARG: &str = "--autostart";

/// True when Windows started us at login (Run key flag, or the packaged
/// startup task) rather than the user launching us.
pub fn launched_at_startup() -> bool {
    if std::env::args().skip(1).any(|a| a.eq_ignore_ascii_case(AUTOSTART_ARG)) {
        return true;
    }
    is_packaged() && packaged::activated_by_startup_task()
}

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
    use std::sync::OnceLock;

    use windows::core::HSTRING;
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::Win32::System::Com::CoIncrementMTAUsage;

    /// Must match `TaskId` in AppxManifest.xml.
    const TASK_ID: &str = "AskeriRullerStartup";

    /// Runs a WinRT call on a worker thread in the process-wide MTA, so
    /// blocking on the async operation never stalls the UI thread.
    ///
    /// The MTA is kept alive for the whole process with CoIncrementMTAUsage
    /// (worker threads join it implicitly). Initializing COM per short-lived
    /// thread instead would tear the MTA down when that thread exits, leaving
    /// the `windows` crate's cached activation factories dangling and
    /// crashing the next call.
    fn on_mta<T: Send + 'static>(f: impl FnOnce() -> windows::core::Result<T> + Send + 'static) -> windows::core::Result<T> {
        static MTA: OnceLock<bool> = OnceLock::new();
        MTA.get_or_init(|| unsafe { CoIncrementMTAUsage().is_ok() });
        std::thread::spawn(f).join().unwrap_or_else(|_| Err(windows::core::Error::empty()))
    }

    fn state() -> windows::core::Result<StartupTaskState> {
        on_mta(|| StartupTask::GetAsync(&HSTRING::from(TASK_ID))?.join()?.State())
    }

    pub fn is_enabled() -> bool {
        matches!(state(), Ok(StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy))
    }

    pub fn activated_by_startup_task() -> bool {
        use windows::ApplicationModel::Activation::ActivationKind;
        use windows::ApplicationModel::AppInstance;
        on_mta(|| AppInstance::GetActivatedEventArgs()?.Kind())
            .map(|k| k == ActivationKind::StartupTask)
            .unwrap_or(false)
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
            let cmd = wide(&format!("\"{exe}\" {AUTOSTART_ARG}"));
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
