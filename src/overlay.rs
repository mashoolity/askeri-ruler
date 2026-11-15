//! One transparent, topmost overlay per monitor.
//!
//! Each overlay owns a 32-bit premultiplied-alpha DIB the size of its monitor.
//! On screen it is shown by a stack of full-width horizontal *strip* windows,
//! each a layered window that displays its slice of the DIB via
//! `UpdateLayeredWindowIndirect` (source offset + dirty rectangle).
//!
//! Why strips: the cost of a layered-window update grows with the dirty area,
//! and one window only gets one dirty rectangle. A diagonal line's bounding box
//! is the whole monitor, but within each strip it only touches a small region,
//! so splitting into strips cuts the pushed area by roughly H / STRIP_HEIGHT.
//!
//! Two modes:
//! * **interactive** (placing/rotating): every pixel has alpha 1/255, which is
//!   invisible but makes the windows hit-testable. The overlay receives all
//!   mouse input and hides the cursor.
//! * **click-through** (locked): background alpha is 0 and the windows have
//!   `WS_EX_TRANSPARENT`, so all input goes to the apps underneath.

use std::f64::consts::FRAC_PI_4;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::raster;
use crate::settings::Style;

/// Invisible but hit-testable background (alpha = 1).
const BG_INTERACTIVE: u32 = 0x01_00_00_00;
/// Fully transparent background.
const BG_CLEAR: u32 = 0x00_00_00_00;

/// Height of each strip window in physical pixels.
const STRIP_HEIGHT: i32 = 96;

/// `color` (0xRRGGBB) at the given coverage, as premultiplied BGRA in a
/// little-endian u32 (0xAARRGGBB). Alpha never drops below the background's
/// so interactive overlays stay hit-testable everywhere.
fn ink(coverage: u8, bg_alpha: u8, color: u32) -> u32 {
    let c = coverage as u32;
    let a = coverage.max(bg_alpha) as u32;
    let ch = |shift: u32| (((color >> shift) & 0xFF) * c + 127) / 255;
    (a << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// A line to draw, in this overlay's local pixel coordinates.
pub struct LineSpec {
    pub px: f64,
    pub py: f64,
    pub angle: f64,
    pub dot: bool,
    /// Dashed X through the pivot at Â±45Â° to the line.
    pub cross: bool,
}

pub struct Overlay {
    pub rect: RECT,
    /// Strip windows from top to bottom; strip `i` starts at row `i * STRIP_HEIGHT`.
    strips: Vec<HWND>,
    w: i32,
    h: i32,
    mem_dc: HDC,
    bmp: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut u32,
    /// Pixel indices currently painted with ink.
    drawn: Vec<u32>,
    scratch: Vec<raster::Coverage>,
    /// Per-strip dirty rectangles (strip-local), reused between frames.
    dirty: Vec<Option<RECT>>,
    bg: u32,
    interactive: bool,
    needs_full: bool,
}

impl Overlay {
    pub fn new(class: *const u16, hinstance: HINSTANCE, rect: RECT, interactive: bool) -> Option<Self> {
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        if w <= 0 || h <= 0 {
            return None;
        }
        unsafe {
            let mem_dc = CreateCompatibleDC(null_mut());
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // top-down rows
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = null_mut();
            let bmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(mem_dc);
                return None;
            }
            let old_bmp = SelectObject(mem_dc, bmp);

            let n = ((h + STRIP_HEIGHT - 1) / STRIP_HEIGHT) as usize;
            let mut ov = Overlay {
                rect,
                strips: Vec::with_capacity(n),
                w,
                h,
                mem_dc,
                bmp,
                old_bmp,
                bits: bits as *mut u32,
                drawn: Vec::new(),
                scratch: Vec::new(),
                dirty: vec![None; n],
                bg: if interactive { BG_INTERACTIVE } else { BG_CLEAR },
                interactive,
                needs_full: true,
            };
            for i in 0..n as i32 {
                let top = i * STRIP_HEIGHT;
                let sh = STRIP_HEIGHT.min(h - top);
                let hwnd = CreateWindowExW(
                    ex_style(interactive),
                    class,
                    null(),
                    WS_POPUP,
                    rect.left,
                    rect.top + top,
                    w,
                    sh,
                    null_mut(),
                    null_mut(),
                    hinstance,
                    null(),
                );
                if hwnd.is_null() {
                    return None; // Drop cleans up what was created
                }
                ov.strips.push(hwnd);
            }
            ov.draw(&[], &crate::settings::Settings::default().style());
            for &s in &ov.strips {
                ShowWindow(s, SW_SHOWNOACTIVATE);
            }
            ov.bring_to_top();
            Some(ov)
        }
    }

    /// A window of this overlay suitable for mouse capture.
    pub fn capture_hwnd(&self) -> HWND {
        self.strips[0]
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }

    pub fn bring_to_top(&self) {
        for &s in &self.strips {
            unsafe {
                SetWindowPos(s, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    pub fn set_interactive(&mut self, interactive: bool) {
        if self.interactive == interactive {
            return;
        }
        self.interactive = interactive;
        self.bg = if interactive { BG_INTERACTIVE } else { BG_CLEAR };
        self.needs_full = true;
        for &s in &self.strips {
            unsafe {
                SetWindowLongPtrW(s, GWL_EXSTYLE, ex_style(interactive) as isize);
            }
        }
        self.bring_to_top();
    }

    /// Redraws the overlay with the given lines and pushes the changed areas.
    pub fn draw(&mut self, lines: &[LineSpec], style: &Style) {
        let (w, h) = (self.w, self.h);

        let mut new = std::mem::take(&mut self.scratch);
        new.clear();
        for l in lines {
            let (cx, cy) = raster::shape_center(l.px, l.py, style.line_width);
            raster::line_pixels(w, h, cx, cy, l.angle, style.line_width, None, &mut new);
            if l.cross && style.show_cross {
                for d in [FRAC_PI_4, -FRAC_PI_4] {
                    raster::line_pixels(w, h, cx, cy, l.angle + d, style.cross_width, Some(&style.dash), &mut new);
                }
            }
            if l.dot {
                raster::dot_pixels(w, h, cx, cy, style.dot_diameter, &mut new);
            }
        }

        let bg = self.bg;
        let mut drawn = std::mem::take(&mut self.drawn);
        let full = self.needs_full;
        if full {
            self.pixels().fill(bg);
        } else {
            if drawn.is_empty() && new.is_empty() {
                self.scratch = new;
                return;
            }
            let px = self.pixels();
            for &i in &drawn {
                px[i as usize] = bg;
            }
            self.dirty.iter_mut().for_each(|d| *d = None);
            let (w, dirty) = (w as u32, &mut self.dirty);
            for i in drawn.iter().copied().chain(new.iter().map(|c| c.0)) {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                let s = (y / STRIP_HEIGHT) as usize;
                let ly = y - s as i32 * STRIP_HEIGHT;
                let r = dirty[s].get_or_insert(RECT { left: x, top: ly, right: x + 1, bottom: ly + 1 });
                r.left = r.left.min(x);
                r.top = r.top.min(ly);
                r.right = r.right.max(x + 1);
                r.bottom = r.bottom.max(ly + 1);
            }
        }

        // Overlapping shapes keep the strongest coverage per pixel.
        let bg_alpha = (bg >> 24) as u8;
        {
            let px = self.pixels();
            for &(i, a) in &new {
                let cur = px[i as usize];
                if a > (cur >> 24) as u8 {
                    px[i as usize] = ink(a, bg_alpha, style.color);
                }
            }
        }
        drawn.clear();
        drawn.extend(new.iter().map(|c| c.0));
        self.drawn = drawn;
        self.scratch = new;
        self.needs_full = false;

        for s in 0..self.strips.len() {
            let top = s as i32 * STRIP_HEIGHT;
            let sh = STRIP_HEIGHT.min(h - top);
            let dirty = if full {
                Some(RECT { left: 0, top: 0, right: w, bottom: sh })
            } else {
                self.dirty[s]
            };
            if let Some(d) = dirty {
                self.present(s, &d);
            }
        }
    }

    fn present(&self, strip: usize, dirty: &RECT) {
        let top = strip as i32 * STRIP_HEIGHT;
        let dst = POINT { x: self.rect.left, y: self.rect.top + top };
        let size = SIZE { cx: self.w, cy: STRIP_HEIGHT.min(self.h - top) };
        let src = POINT { x: 0, y: top };
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
            prcDirty: dirty,
        };
        unsafe {
            UpdateLayeredWindowIndirect(self.strips[strip], &info);
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            for &s in &self.strips {
                DestroyWindow(s);
            }
            SelectObject(self.mem_dc, self.old_bmp);
            DeleteObject(self.bmp);
            DeleteDC(self.mem_dc);
        }
    }
}

fn ex_style(interactive: bool) -> WINDOW_EX_STYLE {
    let base = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    if interactive {
        base
    } else {
        base | WS_EX_TRANSPARENT
    }
}

/// Window procedure for overlay windows. It only translates messages into
/// events; all logic lives in `App`.
pub unsafe extern "system" fn overlay_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use crate::events::{push, Event};
    match msg {
        WM_SETCURSOR => {
            // Hide the cursor while it is over an interactive overlay.
            SetCursor(null_mut());
            1
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_MOUSEMOVE => {
            push(Event::MouseMove);
            0
        }
        WM_LBUTTONDOWN => {
            push(Event::LeftDown);
            0
        }
        WM_RBUTTONDOWN => {
            push(Event::RightDown);
            0
        }
        WM_RBUTTONUP => {
            push(Event::RightUp);
            0
        }
        WM_CAPTURECHANGED => {
            push(Event::CaptureLost);
            0
        }
        // We work in physical pixels and size ourselves to the monitor, so
        // ignore the system's suggested DPI rectangle.
        WM_DPICHANGED => 0,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
