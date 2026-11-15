//! Notification-area (tray) icon with a small context menu.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::util::wide;

pub const WM_TRAY: u32 = WM_APP + 1;

pub const CMD_NEW_LINE: usize = 1;
pub const CMD_CLEAR: usize = 2;
pub const CMD_EXIT: usize = 3;

pub struct Tray {
    hwnd: HWND,
    added: bool,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let mut t = Tray { hwnd, added: false };
        t.add();
        t
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = 1;
        nid
    }

    /// Adds the icon. Also called again when Explorer restarts.
    pub fn add(&mut self) {
        let mut nid = self.data();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        let tip = wide("Askeri Ruler (Ctrl+Shift+X)");
        let n = tip.len().min(nid.szTip.len());
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        self.added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } != 0;
    }

    /// Shows the context menu at the cursor and returns the chosen command.
    pub fn show_menu(&self, has_lines: bool) -> Option<usize> {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            let new_line = wide("New line\tCtrl+Shift+X");
            let clear = wide("Clear all lines\tEsc");
            let exit = wide("Exit");
            AppendMenuW(menu, MF_STRING, CMD_NEW_LINE, new_line.as_ptr());
            AppendMenuW(
                menu,
                MF_STRING | if has_lines { MF_ENABLED } else { MF_GRAYED },
                CMD_CLEAR,
                clear.as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, CMD_EXIT, exit.as_ptr());

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
        if self.added {
            let nid = self.data();
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &nid);
            }
        }
    }
}
