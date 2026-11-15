// Release builds are GUI apps with no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod events;
mod hook;
mod overlay;
mod raster;
mod settings;
mod settings_ui;
mod tray;
mod util;

use std::cell::Cell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use events::{push, Event};
use util::wide;

/// Posted by a second instance to ask this one to open its settings window.
const WM_SHOW_SETTINGS: u32 = WM_APP + 2;

/// Timer used to debounce bursts of WM_DISPLAYCHANGE.
const TIMER_DISPLAY: usize = 1;
const DISPLAY_SETTLE_MS: u32 = 600;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// Last absolute raw-mouse Y (remote desktop / tablets report absolute).
    static RAW_ABS_Y: Cell<Option<i32>> = const { Cell::new(None) };
}

pub fn reset_raw_abs() {
    RAW_ABS_Y.with(|c| c.set(None));
}

fn main() {
    unsafe {
        // Also declared in the manifest; this is a fallback.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Single instance. The handle is intentionally kept open until exit.
        let mutex_name = wide("Local\\AskeriRuller.SingleInstance");
        let _mutex = CreateMutexW(null(), 1, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Started at login while another copy (e.g. the Store and the
            // installer version both set to autostart) is already running:
            // exit quietly. Started by the user: bring up the running copy's
            // settings window so the launch visibly does something.
            if !autostart::launched_at_startup() {
                show_running_instance();
            }
            return;
        }

        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES | ICC_UPDOWN_CLASS | ICC_HOTKEY_CLASS,
        };
        InitCommonControlsEx(&icc);

        let hinstance = GetModuleHandleW(null());
        let icon = LoadIconW(hinstance, tray::ICON_ID as usize as *const u16);

        let main_class = wide("AskeriRullerMain");
        let overlay_class = wide("AskeriRullerOverlay");
        let settings_class = wide("AskeriRullerSettings");
        register_class(hinstance, &main_class, main_wndproc, null_mut(), null_mut());
        register_class(hinstance, &overlay_class, overlay::overlay_wndproc, null_mut(), null_mut());
        register_class(
            hinstance,
            &settings_class,
            settings_ui::settings_wndproc,
            icon,
            (COLOR_BTNFACE + 1) as usize as HBRUSH,
        );

        // Hidden top-level window: receives the hotkey, raw input, tray
        // callbacks and display-change broadcasts.
        let title = wide("Askeri Ruler");
        let main_hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            main_class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        );
        if main_hwnd.is_null() {
            message_box("Failed to create the main window.");
            return;
        }

        TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Ordering::Relaxed);

        let mut app = app::App::new(hinstance, main_hwnd, overlay_class, settings_class);
        run_loop(&mut app);

        drop(app);
        DestroyWindow(main_hwnd);
    }
}

/// Asks the already-running instance to open its settings window.
unsafe fn show_running_instance() {
    let class = wide("AskeriRullerMain");
    let hwnd = FindWindowW(class.as_ptr(), null());
    if hwnd.is_null() {
        return;
    }
    // We are the foreground process (the user just launched us), so we may
    // let the running instance take the foreground for its settings window.
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    AllowSetForegroundWindow(pid);
    PostMessageW(hwnd, WM_SHOW_SETTINGS, 0, 0);
}

/// Message loop that drains all pending messages, applies the resulting
/// events, then renders at most once. High-rate raw mouse input is thereby
/// coalesced into one redraw per batch.
unsafe fn run_loop(app: &mut app::App) {
    let mut msg: MSG = std::mem::zeroed();
    loop {
        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
            if msg.message == WM_QUIT {
                return;
            }
            // Tab / Enter / Esc navigation in the settings window.
            let dlg = settings_ui::current_hwnd();
            if !dlg.is_null() && IsDialogMessageW(dlg, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        while let Some(e) = events::pop() {
            app.handle(e);
        }
        if app.quit {
            return;
        }
        app.render_if_dirty();
        if events::is_empty() {
            WaitMessage();
        }
    }
}

unsafe fn register_class(
    hinstance: HINSTANCE,
    name: &[u16],
    proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    icon: HICON,
    background: HBRUSH,
) {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: icon,
        hCursor: if background.is_null() { null_mut() } else { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: background,
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: icon,
    };
    RegisterClassExW(&wc);
}

unsafe extern "system" fn main_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY if wparam as i32 == app::HOTKEY_ID => {
            push(Event::Hotkey);
            0
        }
        WM_INPUT => {
            read_raw_mouse(lparam as HRAWINPUT);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        tray::WM_TRAY => {
            match (lparam as u32) & 0xFFFF {
                WM_RBUTTONUP | WM_CONTEXTMENU => push(Event::TrayMenu),
                WM_LBUTTONDBLCLK => push(Event::ShowSettings),
                _ => {}
            }
            0
        }
        WM_SHOW_SETTINGS => {
            push(Event::ShowSettings);
            0
        }
        // Sent by `taskkill` (without /f), e.g. from the installer and
        // uninstaller: exit cleanly so the tray icon is removed.
        WM_CLOSE => {
            push(Event::Exit);
            0
        }
        WM_DISPLAYCHANGE => {
            // Windows often sends several of these while a change settles.
            SetTimer(hwnd, TIMER_DISPLAY, DISPLAY_SETTLE_MS, None);
            0
        }
        WM_TIMER if wparam == TIMER_DISPLAY => {
            KillTimer(hwnd, TIMER_DISPLAY);
            push(Event::DisplayChanged);
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            push(Event::TaskbarCreated);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn read_raw_mouse(h: HRAWINPUT) {
    let mut raw: RAWINPUT = std::mem::zeroed();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let got = GetRawInputData(
        h,
        RID_INPUT,
        &mut raw as *mut _ as *mut core::ffi::c_void,
        &mut size,
        std::mem::size_of::<RAWINPUTHEADER>() as u32,
    );
    if got == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE {
        return;
    }
    let m = raw.data.mouse;
    if (m.usFlags as u32 & MOUSE_MOVE_ABSOLUTE as u32) == 0 {
        push(Event::RawMouseDelta { dx: m.lLastX, dy: m.lLastY });
    } else {
        // Absolute coordinates are normalized to 0..65535 across the screen
        // (or the virtual desktop). Convert to pixels and diff.
        let virtual_desk = (m.usFlags as u32 & MOUSE_VIRTUAL_DESKTOP as u32) != 0;
        let h_px = GetSystemMetrics(if virtual_desk { SM_CYVIRTUALSCREEN } else { SM_CYSCREEN });
        let y = (m.lLastY as i64 * h_px as i64 / 65535) as i32;
        let prev = RAW_ABS_Y.with(|c| c.replace(Some(y)));
        if let Some(prev) = prev {
            push(Event::RawMouseDelta { dx: 0, dy: y - prev });
        }
    }
}

pub fn message_box(text: &str) {
    let text = wide(text);
    let caption = wide("Askeri Ruler");
    unsafe {
        MessageBoxW(null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}
