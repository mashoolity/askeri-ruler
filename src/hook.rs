//! Low-level keyboard hook that catches and swallows ESC.
//!
//! The hook is installed only while at least one line is on screen (or while
//! ESC is still held after clearing), so the app does not touch keyboard input
//! the rest of the time.

use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::events::{push, Event};

/// True between a swallowed ESC key-down and its key-up.
static ESC_HELD: AtomicBool = AtomicBool::new(false);

pub fn esc_held() -> bool {
    ESC_HELD.load(Ordering::Relaxed)
}

pub struct KeyboardHook(HHOOK);

impl KeyboardHook {
    pub fn install(hinstance: HINSTANCE) -> Option<Self> {
        let h = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hinstance, 0) };
        if h.is_null() {
            None
        } else {
            Some(KeyboardHook(h))
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        unsafe {
            UnhookWindowsHookEx(self.0);
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        if kb.vkCode == VK_ESCAPE as u32 {
            match wparam as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    // Auto-repeat key-downs are swallowed too, but only the
                    // first one produces an event.
                    if !ESC_HELD.swap(true, Ordering::Relaxed) {
                        push(Event::EscapePressed);
                    }
                    return 1;
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    if ESC_HELD.swap(false, Ordering::Relaxed) {
                        push(Event::EscapeReleased);
                        return 1;
                    }
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}
