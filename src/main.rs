// Release builds are GUI apps with no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod events;
mod hook;
mod overlay;
mod raster;
mod tray;
mod util;

use std::cell::Cell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Input::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use events::{push, Event};
use util::wide;

const HOTKEY_ID: i32 = 1;
const HOTKEY_MODS: HOT_KEY_MODIFIERS = MOD_CONTROL | MOD_SHIFT;
const HOTKEY_VK: u32 = b'X' as u32;
const HOTKEY_NAME: &str = "Ctrl+Shift+X";

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
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Single instance. The handle is intentionally kept open until exit.
        let mutex_name = wide("Local\\AskeriRuller.SingleInstance");
        let _mutex = CreateMutexW(null(), 1, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            message_box("Askeri Ruler is already running.\nLook for its icon in the notification area.");
            return;
        }

        let hinstance = GetModuleHandleW(null());

        let main_class = wide("AskeriRullerMain");
        let overlay_class = wide("AskeriRullerOverlay");
        register_class(hinstance, &main_class, main_wndproc);
        register_class(hinstance, &overlay_class, overlay::overlay_wndproc);

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

        if RegisterHotKey(main_hwnd, HOTKEY_ID, HOTKEY_MODS | MOD_NOREPEAT, HOTKEY_VK) == 0 {
            message_box(&format!(
                "The hotkey {HOTKEY_NAME} is already used by another app.\n\
                 You can still create lines from the tray icon menu."
            ));
        }

        let mut app = app::App::new(hinstance, main_hwnd, overlay_class);
        run_loop(&mut app);

        UnregisterHotKey(main_hwnd, HOTKEY_ID);
        drop(app);
        DestroyWindow(main_hwnd);
    }
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

unsafe fn register_class(hinstance: HINSTANCE, name: &[u16], proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: null_mut(),
    };
    RegisterClassExW(&wc);
}

unsafe extern "system" fn main_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY if wparam as i32 == HOTKEY_ID => {
            push(Event::Hotkey);
            0
        }
        WM_INPUT => {
            read_raw_mouse(lparam as HRAWINPUT);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        tray::WM_TRAY => {
            let m = (lparam as u32) & 0xFFFF;
            if m == WM_RBUTTONUP || m == WM_CONTEXTMENU {
                push(Event::TrayMenu);
            }
            0
        }
        WM_DISPLAYCHANGE => {
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

fn message_box(text: &str) {
    let text = wide(text);
    let caption = wide("Askeri Ruler");
    unsafe {
        MessageBoxW(null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}
