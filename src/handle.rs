//! Drag handle for a locked line: its pivot dot, shown in a tiny layered
//! window of its own.
//!
//! Locked overlays are click-through, so the dot needs a separate window to
//! receive the mouse. Only the dot's own pixels have alpha > 0, which makes
//! exactly the visible dot hit-testable; everything around it stays
//! click-through. While a new line is being placed, handles are made fully
//! click-through so they don't steal the overlay's clicks.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::events::{push, Event};
use crate::overlay::paint_dot;
use crate::settings::Style;

pub struct Handle {
    pub hwnd: HWND,
    /// Id of the line this handle belongs to.
    pub id: u32,
    size: i32,
    mem_dc: HDC,
    bmp: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut u32,
    /// Top-left of the window and the (dot diameter, color) it was painted with.
    pos: Option<(i32, i32)>,
    painted: Option<(u64, u32)>,
    click_through: bool,
}

impl Handle {
    pub fn new(class: *const u16, hinstance: HINSTANCE, id: u32) -> Option<Self> {
        // Large enough for the biggest dot setting (40 px) plus its rings.
        let size = 64;
        unsafe {
            let hwnd = CreateWindowExW(
                ex_style(false),
                class,
                null(),
                WS_POPUP,
                0,
                0,
                size,
                size,
                null_mut(),
                null_mut(),
                hinstance,
                null(),
            );
            if hwnd.is_null() {
                return None;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, id as isize);
            let mem_dc = CreateCompatibleDC(null_mut());
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = size;
            bmi.bmiHeader.biHeight = -size;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = null_mut();
            let bmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(mem_dc);
                DestroyWindow(hwnd);
                return None;
            }
            let old_bmp = SelectObject(mem_dc, bmp);
            Some(Handle {
                hwnd,
                id,
                size,
                mem_dc,
                bmp,
                old_bmp,
                bits: bits as *mut u32,
                pos: None,
                painted: None,
                click_through: false,
            })
        }
    }

    /// Shows the dot centered on (`cx`, `cy`) in screen pixels. Repaints only
    /// when the look changed, moves only when the position changed.
    pub fn update(&mut self, cx: f64, cy: f64, style: &Style, click_through: bool, raise: bool) {
        let s = self.size;
        let origin = ((cx.floor() as i32) - s / 2, (cy.floor() as i32) - s / 2);
        let look = (style.dot_diameter.to_bits(), style.color);
        let repaint = self.painted != Some(look);
        if repaint {
            let px = unsafe { std::slice::from_raw_parts_mut(self.bits, (s * s) as usize) };
            px.fill(0);
            let (lx, ly) = (cx - origin.0 as f64, cy - origin.1 as f64);
            let mut scratch = Vec::new();
            let mut drawn = Vec::new();
            paint_dot(px, s, s, lx, ly, style, &mut scratch, &mut drawn);
            self.painted = Some(look);
        }
        if click_through != self.click_through {
            self.click_through = click_through;
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex_style(click_through) as isize);
            }
        }
        if repaint || self.pos != Some(origin) {
            self.present(origin);
            self.pos = Some(origin);
        }
        unsafe {
            if raise || IsWindowVisible(self.hwnd) == 0 {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    fn present(&self, origin: (i32, i32)) {
        let dst = POINT { x: origin.0, y: origin.1 };
        let size = SIZE { cx: self.size, cy: self.size };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: std::mem::size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: null_mut(),
            pptDst: &dst,
            psize: &size,
            hdcSrc: self.mem_dc,
            pptSrc: &src,
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: null(),
        };
        unsafe {
            UpdateLayeredWindowIndirect(self.hwnd, &info);
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            SelectObject(self.mem_dc, self.old_bmp);
            DeleteObject(self.bmp);
            DeleteDC(self.mem_dc);
        }
    }
}

fn ex_style(click_through: bool) -> WINDOW_EX_STYLE {
    let base = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    if click_through {
        base | WS_EX_TRANSPARENT
    } else {
        base
    }
}

/// Window procedure for handle windows: translates mouse input into events.
pub unsafe extern "system" fn handle_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let id = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as u32;
    match msg {
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
            1
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_LBUTTONDOWN => {
            push(Event::HandleDown(id));
            0
        }
        WM_MOUSEMOVE => {
            push(Event::HandleDrag);
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            push(Event::HandleUp);
            0
        }
        WM_DPICHANGED | WM_CLOSE => 0,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
