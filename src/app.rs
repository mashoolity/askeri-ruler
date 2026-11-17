//! Application state machine.
//!
//! Idle ──hotkey──▶ Placing ◀──RMB up── Rotating
//!                    │  └────RMB down────▶ ┘
//!                    ├──LMB down──▶ Locked
//!                    └──ESC──▶ the line being placed is removed
//! Locked ──hotkey──▶ Placing a new line (existing lines stay)
//! Locked ──ESC──▶ the most recent line is removed (Idle when none are left)
//!
//! While placing, the line follows the mouse at its current angle, so it can
//! be rotated (RMB held, mouse moved like a knob), moved again, rotated again,
//! and finally locked with a left click.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Input::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::autostart;
use crate::events::Event;
use crate::handle::Handle;
use crate::hook::{self, KeyboardHook};
use crate::overlay::{BisectorSpec, LineSpec, Overlay};
use crate::raster::{self, normalize_angle};
use crate::settings::{Settings, FINE_DIVISOR};
use crate::settings_ui::{self, SettingsWindow};
use crate::tray::{self, Tray};

pub const HOTKEY_ID: i32 = 1;

#[derive(Clone, Copy)]
struct Line {
    /// Stable id, used to match a locked line with its drag handle.
    id: u32,
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

/// A locked line being dragged by its dot.
struct Drag {
    id: u32,
    /// Pivot minus cursor at the moment the drag started (no jump on grab).
    offset: (f64, f64),
}

#[derive(Clone)]
struct Monitor {
    name: String,
    rect: RECT,
}

pub struct App {
    hinstance: HINSTANCE,
    main_hwnd: HWND,
    overlay_class: Vec<u16>,
    settings_class: Vec<u16>,
    handle_class: Vec<u16>,
    settings: Settings,
    hotkey_registered: bool,
    /// Monitors the current overlays were created for (same order).
    monitors: Vec<Monitor>,
    overlays: Vec<Overlay>,
    lines: Vec<Line>,
    next_id: u32,
    active: Option<Active>,
    /// Dot handles of the locked lines (one per line, matched by id).
    handles: Vec<Handle>,
    dragging: Option<Drag>,
    /// Handles must be re-raised above the overlays after these were raised.
    raise_handles: bool,
    hook: Option<KeyboardHook>,
    tray: Tray,
    settings_ui: Option<SettingsWindow>,
    dirty: bool,
    pub quit: bool,
}

impl App {
    pub fn new(
        hinstance: HINSTANCE,
        main_hwnd: HWND,
        overlay_class: Vec<u16>,
        settings_class: Vec<u16>,
        handle_class: Vec<u16>,
    ) -> Self {
        let settings = Settings::load();
        let tray = Tray::new(main_hwnd, hinstance, &tip(&settings));
        let mut app = App {
            hinstance,
            main_hwnd,
            overlay_class,
            settings_class,
            handle_class,
            settings,
            hotkey_registered: false,
            monitors: Vec::new(),
            overlays: Vec::new(),
            lines: Vec::new(),
            next_id: 1,
            active: None,
            handles: Vec::new(),
            dragging: None,
            raise_handles: false,
            hook: None,
            tray,
            settings_ui: None,
            dirty: false,
            quit: false,
        };
        if !app.register_hotkey(&settings) {
            crate::message_box(&format!(
                "The hotkey {} is already used by another app.\n\
                 Choose a different one in Settings (double-click the tray icon).",
                settings.hotkey_name()
            ));
        }
        app
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
                // Stop rotating; the line keeps its angle and follows the
                // mouse again until a left click locks it.
                if matches!(&self.active, Some(a) if a.rotating) {
                    self.end_rotate();
                    self.dirty = true;
                }
            }
            Event::RawMouseDelta { dx, dy } => self.rotate_by(dx, dy),
            Event::HandleDown(id) => self.begin_drag(id),
            Event::HandleDrag => self.drag(),
            Event::HandleUp => self.end_drag(),
            Event::EscapePressed => self.remove_last(),
            Event::EscapeReleased => self.update_hook(),
            Event::TrayMenu => self.tray_menu(),
            Event::ShowSettings => self.open_settings(),
            Event::Exit => self.exit(),
            Event::TaskbarCreated => self.tray.add(&tip(&self.settings)),
            Event::DisplayChanged => self.display_changed(),
            Event::SettingsCommand(id) => self.settings_command(id),
            Event::SettingsDpiChanged(dpi) => {
                if let Some(w) = &mut self.settings_ui {
                    w.relayout(dpi);
                }
            }
        }
    }

    // ---- hotkey -------------------------------------------------------

    fn register_hotkey(&mut self, s: &Settings) -> bool {
        self.unregister_hotkey();
        let ok = unsafe { RegisterHotKey(self.main_hwnd, HOTKEY_ID, s.hotkey_mods | MOD_NOREPEAT, s.hotkey_vk) } != 0;
        self.hotkey_registered = ok;
        ok
    }

    fn unregister_hotkey(&mut self) {
        if self.hotkey_registered {
            unsafe {
                UnregisterHotKey(self.main_hwnd, HOTKEY_ID);
            }
            self.hotkey_registered = false;
        }
    }

    // ---- lines --------------------------------------------------------

    fn ensure_overlays(&mut self, interactive: bool) {
        if self.overlays.is_empty() {
            self.monitors = monitor_list();
            for m in &self.monitors {
                match Overlay::new(self.overlay_class.as_ptr(), self.hinstance, m.rect, interactive) {
                    Some(ov) => self.overlays.push(ov),
                    None => {
                        // Keep monitors and overlays index-aligned.
                        self.overlays.clear();
                        self.monitors.clear();
                        return;
                    }
                }
            }
        } else {
            for ov in &mut self.overlays {
                ov.set_interactive(interactive);
                ov.bring_to_top();
            }
        }
        self.raise_handles = true;
    }

    fn start_line(&mut self) {
        if self.active.is_some() {
            return;
        }
        self.end_drag();
        self.ensure_overlays(true);
        let p = cursor_pos();
        let id = self.next_id;
        self.next_id += 1;
        self.active = Some(Active {
            line: Line { id, px: p.x as f64, py: p.y as f64, angle: 0.0, cross: true },
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

    /// Knob-style rotation: moving the mouse right or up turns the line
    /// clockwise, left or down turns it counter-clockwise.
    fn rotate_by(&mut self, dx: i32, dy: i32) {
        let Some(a) = &mut self.active else { return };
        let turn = knob_delta(dx, dy);
        if !a.rotating || turn == 0 {
            return;
        }
        let mut deg_per_px = self.settings.rotation_speed as f64 / 100.0;
        if key_down(VK_SHIFT) {
            deg_per_px /= FINE_DIVISOR;
        }
        a.raw_angle = normalize_angle(a.raw_angle + (turn as f64 * deg_per_px).to_radians());
        a.line.angle = if key_down(VK_CONTROL) {
            let s = (self.settings.snap_step as f64).to_radians();
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
        self.raise_handles = true;
        self.dirty = true;
        self.update_hook();
    }

    // ---- dragging a locked line by its dot ----------------------------

    fn begin_drag(&mut self, id: u32) {
        if self.active.is_some() || self.dragging.is_some() {
            return;
        }
        let Some(l) = self.lines.iter().find(|l| l.id == id) else { return };
        let p = cursor_pos();
        self.dragging = Some(Drag { id, offset: (l.px - p.x as f64, l.py - p.y as f64) });
        if let Some(h) = self.handles.iter().find(|h| h.id == id) {
            unsafe {
                SetCapture(h.hwnd);
            }
        }
    }

    fn drag(&mut self) {
        let Some(d) = &self.dragging else { return };
        let p = cursor_pos();
        if let Some(l) = self.lines.iter_mut().find(|l| l.id == d.id) {
            l.px = p.x as f64 + d.offset.0;
            l.py = p.y as f64 + d.offset.1;
            self.dirty = true;
        }
    }

    fn end_drag(&mut self) {
        if self.dragging.take().is_some() {
            unsafe {
                // Triggers WM_CAPTURECHANGED -> HandleUp, ignored now.
                ReleaseCapture();
            }
        }
    }

    /// Esc: removes the most recent line — the one being placed, otherwise
    /// the last locked one. One line per press.
    fn remove_last(&mut self) {
        self.end_drag();
        if self.active.is_some() {
            self.end_rotate();
            self.active = None;
            // The remaining locked lines go back to being click-through.
            for ov in &mut self.overlays {
                ov.set_interactive(false);
            }
            self.raise_handles = true;
        } else {
            self.lines.pop();
        }
        if self.lines.is_empty() && self.active.is_none() {
            self.clear_all();
            return;
        }
        self.dirty = true;
        self.update_hook();
    }

    fn clear_all(&mut self) {
        self.end_drag();
        self.end_rotate();
        self.active = None;
        self.lines.clear();
        self.handles.clear();
        self.overlays.clear(); // destroys the windows and frees the bitmaps
        self.monitors.clear();
        self.dirty = false;
        self.update_hook();
    }

    /// Monitors were added, removed, moved or changed resolution. Keep each
    /// locked line on its monitor (scaled to the new size) and rebuild the
    /// overlays; lines whose monitor is gone are removed.
    fn display_changed(&mut self) {
        if self.overlays.is_empty() {
            return;
        }
        self.end_drag();
        self.end_rotate();
        let old = std::mem::take(&mut self.monitors);
        let new = monitor_list();
        self.lines.retain_mut(|l| {
            let Some(om) = old.iter().find(|m| contains(&m.rect, l.px as i32, l.py as i32)) else {
                return false;
            };
            let Some(nm) = new.iter().find(|m| m.name == om.name) else { return false };
            let (or, nr) = (om.rect, nm.rect);
            let sx = (nr.right - nr.left) as f64 / (or.right - or.left).max(1) as f64;
            let sy = (nr.bottom - nr.top) as f64 / (or.bottom - or.top).max(1) as f64;
            l.px = (nr.left as f64 + (l.px - or.left as f64) * sx).floor();
            l.py = (nr.top as f64 + (l.py - or.top as f64) * sy).floor();
            true
        });
        self.overlays.clear();
        if self.lines.is_empty() && self.active.is_none() {
            self.handles.clear();
            self.update_hook();
            return;
        }
        let placing = self.active.is_some();
        self.ensure_overlays(placing);
        if let Some(a) = &mut self.active {
            let p = cursor_pos();
            a.line.px = p.x as f64;
            a.line.py = p.y as f64;
        }
        self.dirty = true;
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

    // ---- tray & settings ---------------------------------------------

    fn tray_menu(&mut self) {
        let has_lines = !self.lines.is_empty() || self.active.is_some();
        let auto = autostart::is_enabled();
        match self.tray.show_menu(has_lines, &self.settings.hotkey_name(), auto) {
            Some(tray::CMD_NEW_LINE) => self.start_line(),
            Some(tray::CMD_CLEAR) => self.clear_all(),
            Some(tray::CMD_SETTINGS) => self.open_settings(),
            Some(tray::CMD_AUTOSTART) => {
                if let Err(msg) = autostart::set_enabled(!auto) {
                    crate::message_box(&msg);
                }
            }
            Some(tray::CMD_EXIT) => self.exit(),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.clear_all();
        self.settings_ui = None;
        self.quit = true;
    }

    fn open_settings(&mut self) {
        if let Some(w) = &self.settings_ui {
            unsafe {
                ShowWindow(w.hwnd, SW_RESTORE);
                SetForegroundWindow(w.hwnd);
            }
            return;
        }
        // Free the hotkey so the hotkey field can capture the combination.
        self.unregister_hotkey();
        self.settings_ui =
            SettingsWindow::open(self.hinstance, &self.settings_class, &self.settings, autostart::is_enabled());
        if self.settings_ui.is_none() {
            let s = self.settings;
            self.register_hotkey(&s);
        }
    }

    fn settings_command(&mut self, id: u16) {
        let Some(w) = &mut self.settings_ui else { return };
        match id {
            settings_ui::ID_COLOR => w.choose_color(),
            settings_ui::ID_CROSS => w.update_enabled(),
            settings_ui::ID_RESET => {
                let auto = w.autostart_checked();
                w.set_values(&Settings::default(), auto);
            }
            settings_ui::ID_OK => self.apply_settings(),
            settings_ui::ID_CANCEL => {
                self.settings_ui = None;
                let s = self.settings;
                if !self.register_hotkey(&s) {
                    crate::message_box(&format!("The hotkey {} is already used by another app.", s.hotkey_name()));
                }
            }
            _ => {}
        }
    }

    fn settings_error(&self, msg: &str) {
        if let Some(w) = &self.settings_ui {
            w.show_error(msg);
        }
    }

    fn apply_settings(&mut self) {
        let Some(w) = &self.settings_ui else { return };
        let (s, auto) = match w.read() {
            Ok(v) => v,
            Err(msg) => return w.show_error(&msg),
        };
        if !self.register_hotkey(&s) {
            self.settings_error(&format!(
                "The hotkey {} is already used by another app. Please choose a different one.",
                s.hotkey_name()
            ));
            return;
        }
        if auto != autostart::is_enabled() {
            if let Err(msg) = autostart::set_enabled(auto) {
                self.settings_error(&msg);
            }
        }
        if let Err(e) = s.save() {
            self.settings_error(&format!("Settings were applied but could not be saved:\n{e}"));
        }
        self.settings = s;
        self.tray.set_tip(&tip(&s));
        self.settings_ui = None;
        self.dirty = true;
    }

    // ---- rendering ----------------------------------------------------

    pub fn render_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let style = self.settings.style();
        // All lines in creation order; the active one (with its dot) is last.
        let locked = self.lines.iter().map(|l| (l, false));
        let active = self.active.as_ref().map(|a| (&a.line, true));
        let all: Vec<(&Line, bool)> = locked.chain(active).collect();

        let mut specs = Vec::new();
        let mut bisectors = Vec::new();
        for ov in &mut self.overlays {
            specs.clear();
            bisectors.clear();
            let r = ov.rect;
            let on_this = |l: &Line| contains(&r, l.px as i32, l.py as i32);
            let local = |l: &Line| (l.px - r.left as f64, l.py - r.top as f64);
            for &(l, dot) in &all {
                if on_this(l) {
                    let (px, py) = local(l);
                    specs.push(LineSpec { px, py, angle: l.angle, dot, cross: l.cross });
                }
            }
            // Each new line gets a dotted bisector of its acute angle with the
            // previous line (when both are on this monitor).
            if style.show_bisector {
                for pair in all.windows(2) {
                    let (a, b) = (pair[0].0, pair[1].0);
                    if !(on_this(a) && on_this(b)) {
                        continue;
                    }
                    let (ax, ay) = local(a);
                    let (bx, by) = local(b);
                    let ca = raster::shape_center(ax, ay, style.line_width);
                    let cb = raster::shape_center(bx, by, style.line_width);
                    if let Some((cx, cy, angle)) = raster::bisector(ca, a.angle, cb, b.angle) {
                        bisectors.push(BisectorSpec { cx, cy, angle });
                    }
                }
            }
            ov.draw(&specs, &bisectors, &style);
        }
        self.sync_handles(&style);
    }

    /// One dot handle per locked line, centered on the line's pivot.
    /// Handles are click-through while a new line is being placed.
    fn sync_handles(&mut self, style: &crate::settings::Style) {
        let ids: Vec<u32> = self.lines.iter().map(|l| l.id).collect();
        self.handles.retain(|h| ids.contains(&h.id));
        let placing = self.active.is_some();
        let raise = std::mem::take(&mut self.raise_handles);
        for l in &self.lines {
            let idx = match self.handles.iter().position(|h| h.id == l.id) {
                Some(i) => i,
                None => match Handle::new(self.handle_class.as_ptr(), self.hinstance, l.id) {
                    Some(h) => {
                        self.handles.push(h);
                        self.handles.len() - 1
                    }
                    None => continue,
                },
            };
            let (cx, cy) = raster::shape_center(l.px, l.py, style.line_width);
            self.handles[idx].update(cx, cy, style, placing, raise);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.unregister_hotkey();
    }
}

/// Knob mapping of a mouse movement to a rotation amount in pixels.
/// Positive = clockwise on screen (y points down): right and up turn
/// clockwise, like dragging a knob in music software.
fn knob_delta(dx: i32, dy: i32) -> i32 {
    dx - dy
}

fn tip(s: &Settings) -> String {
    format!("Askeri Ruler ({})", s.hotkey_name())
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

fn monitor_list() -> Vec<Monitor> {
    unsafe extern "system" fn cb(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        let v = &mut *(data as *mut Vec<Monitor>);
        let mut mi: MONITORINFOEXW = std::mem::zeroed();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmon, &mut mi as *mut _ as *mut MONITORINFO) != 0 {
            let n = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len());
            v.push(Monitor {
                name: String::from_utf16_lossy(&mi.szDevice[..n]),
                rect: mi.monitorInfo.rcMonitor,
            });
        }
        1
    }
    let mut v: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), std::ptr::null(), Some(cb), &mut v as *mut _ as LPARAM);
    }
    v
}
