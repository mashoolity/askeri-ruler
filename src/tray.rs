//! Notification-area (tray) icon with a small context menu.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::util::wide;

pub const WM_TRAY: u32 = WM_APP + 1;

pub const CMD_NEW_LINE: usize = 1;
pub const CMD_CLEAR: usize = 2;
pub const CMD_EXIT: usize = 3;
pub const CMD_SETTINGS: usize = 4;
pub const CMD_AUTOSTART: usize = 5;

/// Resource id of the app icon (see build.rs).
pub const ICON_ID: u16 = 1;

pub struct Tray {
    hwnd: HWND,
    hinstance: HINSTANCE,
    icon: HICON,
    added: bool,
}

impl Tray {
    pub fn new(hwnd: HWND, hinstance: HINSTANCE, tip: &str) -> Self {
        let mut icon: HICON = null_mut();
        unsafe {
            LoadIconMetric(hinstance, ICON_ID as usize as *const u16, LIM_SMALL, &mut icon);
            if icon.is_null() {
                icon = LoadIconW(null_mut(), IDI_APPLICATION);
            }
        }
        let mut t = Tray { hwnd, hinstance, icon, added: false };
        t.add(tip);
        t
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = 1;
        nid
    }

    fn fill_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
        let tip = wide(tip);
        let n = tip.len().min(nid.szTip.len() - 1);
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        nid.szTip[n] = 0;
    }

    /// Adds the icon. Also called again when Explorer restarts.
    pub fn add(&mut self, tip: &str) {
        let _ = self.hinstance;
        let mut nid = self.data();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = self.icon;
        Self::fill_tip(&mut nid, tip);
        self.added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } != 0;
    }

    pub fn set_tip(&self, tip: &str) {
        let mut nid = self.data();
        nid.uFlags = NIF_TIP;
        Self::fill_tip(&mut nid, tip);
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Shows the context menu at the cursor and returns the chosen command.
    pub fn show_menu(&self, has_lines: bool, hotkey: &str, autostart: bool) -> Option<usize> {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            let new_line = wide(&format!("New line\t{hotkey}"));
            let clear = wide("Clear all lines\tEsc");
            let settings = wide("Settings…");
            let auto = wide("Start with Windows");
            let exit = wide("Exit");
            AppendMenuW(menu, MF_STRING, CMD_NEW_LINE, new_line.as_ptr());
            AppendMenuW(
                menu,
                MF_STRING | if has_lines { MF_ENABLED } else { MF_GRAYED },
                CMD_CLEAR,
                clear.as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, CMD_SETTINGS, settings.as_ptr());
            AppendMenuW(
                menu,
                MF_STRING | if autostart { MF_CHECKED } else { MF_UNCHECKED },
                CMD_AUTOSTART,
                auto.as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, CMD_EXIT, exit.as_ptr());
            SetMenuDefaultItem(menu, CMD_SETTINGS as u32, 0);

            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            // Required so the menu closes when the user clicks elsewhere.
            SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                0,
                self.hwnd,
                null(),
            );
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            if cmd > 0 {
                Some(cmd as usize)
            } else {
                None
            }
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            if self.added {
                let nid = self.data();
                Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            DestroyIcon(self.icon);
        }
    }
}
