//! Application state machine.
//!
//! Idle ──hotkey──▶ Placing ──RMB down──▶ Rotating ──RMB up──▶ Locked
//!                    │  └──LMB down (lock without rotating)───▶ Locked
//!                    └──ESC──▶ Idle (all lines removed)
//! Locked ──hotkey──▶ Placing a new line (existing lines stay)
//! Locked ──ESC──▶ Idle (all lines removed)

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Input::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::events::Event;
use crate::hook::{self, KeyboardHook};
use crate::overlay::{LineSpec, Overlay};
use crate::raster::normalize_angle;
use crate::tray::{self, Tray};

/// Rotation speed in degrees per pixel of vertical mouse movement.
const DEG_PER_PX: f64 = 0.1;
/// Rotation speed while Shift is held.
const DEG_PER_PX_FINE: f64 = 0.02;
/// Snap step while Ctrl is held.
const SNAP_DEG: f64 = 15.0;

#[derive(Clone, Copy)]
struct Line {
    /// Pivot in virtual-screen physical pixels.
    px: f64,
    py: f64,
    /// Radians; positive is clockwise on screen (y points down).
    angle: f64,
    /// Show the dashed diagonal cross. Removed for good once the line
    /// enters rotation mode (right button pressed).
    cross: bool,
}

struct Active {
    line: Line,
    rotating: bool,
    /// Accumulated angle before Ctrl-snapping.
    raw_angle: f64,
}

pub struct App {
    hinstance: HINSTANCE,
    main_hwnd: HWND,
    overlay_class: Vec<u16>,
    overlays: Vec<Overlay>,
    lines: Vec<Line>,
    active: Option<Active>,
    hook: Option<KeyboardHook>,
    tray: Tray,
    dirty: bool,
    pub quit: bool,
}

impl App {
    pub fn new(hinstance: HINSTANCE, main_hwnd: HWND, overlay_class: Vec<u16>) -> Self {
        App {
            hinstance,
            main_hwnd,
            overlay_class,
            overlays: Vec::new(),
            lines: Vec::new(),
            active: None,
            hook: None,
            tray: Tray::new(main_hwnd),
            dirty: false,
            quit: false,
        }
    }

    pub fn handle(&mut self, e: Event) {
        match e {
            Event::Hotkey => self.start_line(),
            Event::MouseMove => {
                if let Some(a) = &mut self.active {
                    if !a.rotating {
                        let p = cursor_pos();
                        a.line.px = p.x as f64;
                        a.line.py = p.y as f64;
                        self.dirty = true;
                    }
                }
            }
            Event::LeftDown => {
                if matches!(&self.active, Some(a) if !a.rotating) {
                    self.lock();
                }
            }
            Event::RightDown => self.begin_rotate(),
            Event::RightUp | Event::CaptureLost => {
                if matches!(&self.active, Some(a) if a.rotating) {
                    self.lock();
                }
            }
            Event::RawMouseDelta { dy, .. } => self.rotate_by(dy),
            Event::EscapePressed => self.clear_all(),
            Event::EscapeReleased => self.update_hook(),
            Event::TrayMenu => self.tray_menu(),
            Event::TaskbarCreated => self.tray.add(),
            Event::DisplayChanged => {
                // Monitor geometry changed; overlays no longer match it.
                self.clear_all();
            }
        }
    }

    fn start_line(&mut self) {
        if self.active.is_some() {
            return;
        }
        if self.overlays.is_empty() {
            for rect in monitor_rects() {
                if let Some(ov) = Overlay::new(self.overlay_class.as_ptr(), self.hinstance, rect, true) {
                    self.overlays.push(ov);
                }
            }
        } else {
            for ov in &mut self.overlays {
                ov.set_interactive(true);
            }
        }
        let p = cursor_pos();
        self.active = Some(Active {
            line: Line { px: p.x as f64, py: p.y as f64, angle: 0.0, cross: true },
            rotating: false,
            raw_angle: 0.0,
        });
        self.update_hook();
        self.dirty = true;
        // Nudge the system into sending WM_SETCURSOR so the cursor hides
        // immediately instead of on the next mouse move.
        unsafe {
            SetCursorPos(p.x, p.y);
        }
    }

    fn begin_rotate(&mut self) {
        let Some(a) = &mut self.active else { return };
        if a.rotating {
            return;
        }
        a.rotating = true;
        a.raw_angle = a.line.angle;
        a.line.cross = false;
        self.dirty = true;
        let (x, y) = (a.line.px as i32, a.line.py as i32);
        let capture_hwnd = self
            .overlays
            .iter()
            .find(|o| contains(&o.rect, x, y))
            .or(self.overlays.first())
            .map(|o| o.capture_hwnd());
        unsafe {
            if let Some(h) = capture_hwnd {
                SetCapture(h);
            }
            // Pin the hidden cursor to the pivot; rotation reads raw deltas.
            let clip = RECT { left: x, top: y, right: x + 1, bottom: y + 1 };
            ClipCursor(&clip);
        }
        crate::reset_raw_abs();
        register_raw_mouse(self.main_hwnd, true);
    }

    fn rotate_by(&mut self, dy: i32) {
        let Some(a) = &mut self.active else { return };
        if !a.rotating || dy == 0 {
            return;
        }
        let fine = key_down(VK_SHIFT);
        let snap = key_down(VK_CONTROL);
        let step = if fine { DEG_PER_PX_FINE } else { DEG_PER_PX };
        a.raw_angle = normalize_angle(a.raw_angle + (dy as f64 * step).to_radians());
        a.line.angle = if snap {
            let s = SNAP_DEG.to_radians();
            normalize_angle((a.raw_angle / s).round() * s)
        } else {
            a.raw_angle
        };
        self.dirty = true;
    }

    fn end_rotate(&mut self) {
        if let Some(a) = &mut self.active {
            if a.rotating {
                a.rotating = false;
                register_raw_mouse(self.main_hwnd, false);
                unsafe {
                    ClipCursor(std::ptr::null());
                    // Triggers WM_CAPTURECHANGED -> CaptureLost, which is
                    // ignored because nothing is rotating anymore.
                    ReleaseCapture();
                }
            }
        }
    }

    /// Freezes the active line and lets input through to other apps.
    fn lock(&mut self) {
        self.end_rotate();
        if let Some(a) = self.active.take() {
            self.lines.push(a.line);
        }
        for ov in &mut self.overlays {
            ov.set_interactive(false);
        }
        self.dirty = true;
        self.update_hook();
    }

    fn clear_all(&mut self) {
        self.end_rotate();
        self.active = None;
        self.lines.clear();
        self.overlays.clear(); // destroys the windows and frees the bitmaps
        self.dirty = false;
        self.update_hook();
    }

    /// Keeps the ESC hook installed only while something is on screen.
    fn update_hook(&mut self) {
        let want = !self.lines.is_empty() || self.active.is_some() || hook::esc_held();
        if want && self.hook.is_none() {
            self.hook = KeyboardHook::install(self.hinstance);
        } else if !want {
            self.hook = None;
        }
    }

    fn tray_menu(&mut self) {
        let has_lines = !self.lines.is_empty() || self.active.is_some();
        match self.tray.show_menu(has_lines) {
            Some(tray::CMD_NEW_LINE) => self.start_line(),
            Some(tray::CMD_CLEAR) => self.clear_all(),
            Some(tray::CMD_EXIT) => {
                self.clear_all();
                self.quit = true;
            }
            _ => {}
        }
    }

    pub fn render_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let mut specs = Vec::new();
        for ov in &mut self.overlays {
            specs.clear();
            let r = ov.rect;
            let locked = self.lines.iter().map(|l| (l, false));
            let active = self.active.as_ref().map(|a| (&a.line, true));
            for (l, dot) in locked.chain(active) {
                if contains(&r, l.px as i32, l.py as i32) {
                    specs.push(LineSpec {
                        px: l.px - r.left as f64,
                        py: l.py - r.top as f64,
                        angle: l.angle,
                        dot,
                        cross: l.cross,
                    });
                }
            }
            ov.draw(&specs);
        }
    }
}

fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn cursor_pos() -> POINT {
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut p);
    }
    p
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

fn register_raw_mouse(hwnd: HWND, on: bool) {
    let dev = RAWINPUTDEVICE {
        usUsagePage: 0x01, // HID_USAGE_PAGE_GENERIC
        usUsage: 0x02,     // HID_USAGE_GENERIC_MOUSE
        dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
        hwndTarget: if on { hwnd } else { null_mut() },
    };
    unsafe {
        RegisterRawInputDevices(&dev, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32);
    }
}

fn monitor_rects() -> Vec<RECT> {
    unsafe extern "system" fn cb(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        let v = &mut *(data as *mut Vec<RECT>);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut mi) != 0 {
            v.push(mi.rcMonitor);
        }
        1
    }
    let mut v: Vec<RECT> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), std::ptr::null(), Some(cb), &mut v as *mut _ as LPARAM);
    }
    v
}
