//! Settings window built from plain Win32 controls.
//!
//! Layout is defined in 96-DPI units and scaled for the monitor's DPI, and
//! re-applied on `WM_DPICHANGED`. The window procedure only forwards button
//! clicks as events; `App` drives everything else through `SettingsWindow`.

use std::cell::Cell;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::events::{push, Event};
use crate::settings::{self, Settings};
use crate::util::wide;

pub const ID_OK: u16 = IDOK as u16;
pub const ID_CANCEL: u16 = IDCANCEL as u16;
pub const ID_HOTKEY: u16 = 101;
pub const ID_SWATCH: u16 = 102;
pub const ID_COLOR: u16 = 103;
pub const ID_LINE_W: u16 = 104;
pub const ID_DOT: u16 = 105;
pub const ID_CROSS: u16 = 106;
pub const ID_CROSS_W: u16 = 107;
pub const ID_DASH: u16 = 108;
pub const ID_GAP: u16 = 109;
pub const ID_ROT: u16 = 110;
pub const ID_SNAP: u16 = 111;
pub const ID_AUTOSTART: u16 = 112;
pub const ID_RESET: u16 = 113;

/// Numeric fields: id, label, valid range.
const NUMS: [(u16, &str, (u32, u32)); 7] = [
    (ID_LINE_W, "Line width (px):", settings::LINE_WIDTH_RANGE),
    (ID_DOT, "Dot diameter (px):", settings::DOT_RANGE),
    (ID_CROSS_W, "Cross line width (px):", settings::CROSS_WIDTH_RANGE),
    (ID_DASH, "Dash length (px):", settings::DASH_RANGE),
    (ID_GAP, "Gap length (px):", settings::DASH_RANGE),
    (ID_ROT, "Rotation (° per 100 px):", settings::ROTATION_RANGE),
    (ID_SNAP, "Ctrl snap step (°):", settings::SNAP_RANGE),
];

fn num_get(s: &Settings, id: u16) -> u32 {
    match id {
        ID_LINE_W => s.line_width,
        ID_DOT => s.dot_diameter,
        ID_CROSS_W => s.cross_width,
        ID_DASH => s.dash_length,
        ID_GAP => s.dash_gap,
        ID_ROT => s.rotation_speed,
        _ => s.snap_step,
    }
}

fn num_set(s: &mut Settings, id: u16, v: u32) {
    match id {
        ID_LINE_W => s.line_width = v,
        ID_DOT => s.dot_diameter = v,
        ID_CROSS_W => s.cross_width = v,
        ID_DASH => s.dash_length = v,
        ID_GAP => s.dash_gap = v,
        ID_ROT => s.rotation_speed = v,
        _ => s.snap_step = v,
    }
}

// Layout in 96-DPI units.
const MARGIN: i32 = 16;
const LABEL_W: i32 = 176;
const CTRL_X: i32 = MARGIN + LABEL_W + 8;
const CTRL_W: i32 = 160;
const CLIENT_W: i32 = CTRL_X + CTRL_W + MARGIN;
const ROW: i32 = 32;
const CTRL_H: i32 = 24;
const BTN_W: i32 = 88;
const BTN_H: i32 = 28;

thread_local! {
    static SWATCH_BRUSH: Cell<HBRUSH> = const { Cell::new(null_mut()) };
    static CURRENT: Cell<HWND> = const { Cell::new(null_mut()) };
    static CUSTOM_COLORS: Cell<[u32; 16]> = const { Cell::new([0x00FFFFFF; 16]) };
}

/// The open settings window, if any (for `IsDialogMessage` in the main loop).
pub fn current_hwnd() -> HWND {
    CURRENT.with(|c| c.get())
}

struct Ctl {
    hwnd: HWND,
    rect: (i32, i32, i32, i32),
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    hinstance: HINSTANCE,
    ctls: Vec<Ctl>,
    /// (up-down control, its buddy edit)
    updowns: Vec<(HWND, HWND)>,
    font: HFONT,
    dpi: u32,
    color: u32,
}

fn colorref(rgb: u32) -> COLORREF {
    ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
}

impl SettingsWindow {
    pub fn open(hinstance: HINSTANCE, class: &[u16], s: &Settings, autostart: bool) -> Option<Self> {
        unsafe {
            let mut cursor = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cursor);
            let title = wide("Askeri Ruler Settings");
            let hwnd = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class.as_ptr(),
                title.as_ptr(),
                WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
                cursor.x,
                cursor.y,
                100,
                100,
                null_mut(),
                null_mut(),
                hinstance,
                null(),
            );
            if hwnd.is_null() {
                return None;
            }
            let mut w = SettingsWindow {
                hwnd,
                hinstance,
                ctls: Vec::new(),
                updowns: Vec::new(),
                font: null_mut(),
                dpi: GetDpiForWindow(hwnd).max(96),
                color: s.color,
            };
            w.build();
            w.set_values(s, autostart);
            w.relayout(w.dpi);
            w.center_on_cursor_monitor(cursor);
            CURRENT.with(|c| c.set(hwnd));
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            if let Some(hk) = w.ctl(ID_HOTKEY) {
                SetFocus(hk);
            }
            Some(w)
        }
    }

    fn add(&mut self, class: &str, text: &str, style: u32, ex: u32, id: u16, rect: (i32, i32, i32, i32)) -> HWND {
        let class = wide(class);
        let text = wide(text);
        let hwnd = unsafe {
            CreateWindowExW(
                ex,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                0,
                0,
                self.hwnd,
                id as usize as HMENU,
                self.hinstance,
                null(),
            )
        };
        self.ctls.push(Ctl { hwnd, rect });
        hwnd
    }

    fn label(&mut self, row: i32, text: &str) {
        self.add("STATIC", text, 0 /* SS_LEFT */, 0, 0xFFFF, (MARGIN, row_y(row) + 4, LABEL_W, CTRL_H - 4));
    }

    fn build(&mut self) {
        let mut row = 0;
        self.label(row, "Hotkey:");
        self.add(
            "msctls_hotkey32",
            "",
            WS_TABSTOP,
            WS_EX_CLIENTEDGE,
            ID_HOTKEY,
            (CTRL_X, row_y(row), CTRL_W, CTRL_H),
        );
        row += 1;

        self.label(row, "Line color:");
        self.add("STATIC", "", WS_BORDER, 0, ID_SWATCH, (CTRL_X, row_y(row), 40, CTRL_H));
        self.add(
            "BUTTON",
            "Choose…",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_COLOR,
            (CTRL_X + 48, row_y(row) - 1, CTRL_W - 48, CTRL_H + 2),
        );
        row += 1;

        for &(id, label, (lo, hi)) in NUMS.iter() {
            if id == ID_CROSS_W {
                self.add(
                    "BUTTON",
                    "Show dashed diagonal cross",
                    WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                    0,
                    ID_CROSS,
                    (MARGIN, row_y(row), CLIENT_W - 2 * MARGIN, CTRL_H),
                );
                row += 1;
            }
            self.label(row, label);
            let edit = self.add(
                "EDIT",
                "",
                WS_TABSTOP | ES_NUMBER as u32 | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
                WS_EX_CLIENTEDGE,
                id,
                (CTRL_X, row_y(row), 80, CTRL_H),
            );
            let cls = wide("msctls_updown32");
            unsafe {
                let ud = CreateWindowExW(
                    0,
                    cls.as_ptr(),
                    null(),
                    WS_CHILD
                        | WS_VISIBLE
                        | UDS_SETBUDDYINT as u32
                        | UDS_ALIGNRIGHT as u32
                        | UDS_ARROWKEYS as u32
                        | UDS_NOTHOUSANDS as u32,
                    0,
                    0,
                    0,
                    0,
                    self.hwnd,
                    (id + 100) as usize as HMENU,
                    self.hinstance,
                    null(),
                );
                SendMessageW(ud, UDM_SETBUDDY, edit as WPARAM, 0);
                SendMessageW(ud, UDM_SETRANGE32, lo as WPARAM, hi as LPARAM);
                self.updowns.push((ud, edit));
            }
            row += 1;
        }

        self.add(
            "BUTTON",
            "Start with Windows",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ID_AUTOSTART,
            (MARGIN, row_y(row), CLIENT_W - 2 * MARGIN, CTRL_H),
        );
        row += 1;

        self.add(
            "STATIC",
            "While rotating: hold Shift for fine control, Ctrl to snap.",
            0 /* SS_LEFT */,
            0,
            0xFFFF,
            (MARGIN, row_y(row) + 4, CLIENT_W - 2 * MARGIN, CTRL_H),
        );
        row += 1;

        let by = row_y(row) + 8;
        self.add(
            "BUTTON",
            "Reset to defaults",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_RESET,
            (MARGIN, by, 128, BTN_H),
        );
        self.add(
            "BUTTON",
            "OK",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            0,
            ID_OK,
            (CLIENT_W - MARGIN - 2 * BTN_W - 8, by, BTN_W, BTN_H),
        );
        self.add(
            "BUTTON",
            "Cancel",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_CANCEL,
            (CLIENT_W - MARGIN - BTN_W, by, BTN_W, BTN_H),
        );
    }

    fn client_height() -> i32 {
        // Rows: hotkey, color, 7 numbers, cross checkbox, autostart, hint.
        row_y(12) + 8 + BTN_H + MARGIN
    }

    fn ctl(&self, id: u16) -> Option<HWND> {
        let h = unsafe { GetDlgItem(self.hwnd, id as i32) };
        (!h.is_null()).then_some(h)
    }

    fn scale(&self, v: i32) -> i32 {
        (v * self.dpi as i32 + 48) / 96
    }

    /// Positions all controls and sizes the window for `dpi`.
    pub fn relayout(&mut self, dpi: u32) {
        self.dpi = dpi.max(96);
        unsafe {
            let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
            ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                ncm.cbSize,
                &mut ncm as *mut _ as *mut core::ffi::c_void,
                0,
                self.dpi,
            );
            let font = CreateFontIndirectW(&ncm.lfMessageFont);
            for c in &self.ctls {
                let (x, y, w, h) = c.rect;
                SetWindowPos(
                    c.hwnd,
                    null_mut(),
                    self.scale(x),
                    self.scale(y),
                    self.scale(w),
                    self.scale(h),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(c.hwnd, WM_SETFONT, font as WPARAM, 1);
            }
            for &(ud, edit) in &self.updowns {
                SendMessageW(ud, UDM_SETBUDDY, edit as WPARAM, 0);
            }
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            self.font = font;

            let mut r = RECT { left: 0, top: 0, right: self.scale(CLIENT_W), bottom: self.scale(Self::client_height()) };
            AdjustWindowRectExForDpi(
                &mut r,
                WS_CAPTION | WS_SYSMENU,
                0,
                WS_EX_CONTROLPARENT,
                self.dpi,
            );
            SetWindowPos(
                self.hwnd,
                null_mut(),
                0,
                0,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn center_on_cursor_monitor(&self, pt: POINT) {
        unsafe {
            let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(mon, &mut mi);
            let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(self.hwnd, &mut wr);
            let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
            let wa = mi.rcWork;
            let x = wa.left + ((wa.right - wa.left) - w) / 2;
            let y = wa.top + ((wa.bottom - wa.top) - h) / 2;
            SetWindowPos(self.hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    pub fn set_values(&mut self, s: &Settings, autostart: bool) {
        unsafe {
            if let Some(hk) = self.ctl(ID_HOTKEY) {
                let mut f = 0u32;
                if s.hotkey_mods & MOD_SHIFT != 0 {
                    f |= HOTKEYF_SHIFT;
                }
                if s.hotkey_mods & MOD_CONTROL != 0 {
                    f |= HOTKEYF_CONTROL;
                }
                if s.hotkey_mods & MOD_ALT != 0 {
                    f |= HOTKEYF_ALT;
                }
                SendMessageW(hk, HKM_SETHOTKEY, ((f << 8) | (s.hotkey_vk & 0xFF)) as WPARAM, 0);
            }
            for &(ud, edit) in &self.updowns {
                let id = GetDlgCtrlID(edit) as u16;
                SendMessageW(ud, UDM_SETPOS32, 0, num_get(s, id) as LPARAM);
            }
            self.set_check(ID_CROSS, s.show_cross);
            self.set_check(ID_AUTOSTART, autostart);
        }
        self.set_color(s.color);
        self.update_enabled();
    }

    fn set_check(&self, id: u16, on: bool) {
        if let Some(h) = self.ctl(id) {
            unsafe {
                SendMessageW(h, BM_SETCHECK, if on { BST_CHECKED } else { BST_UNCHECKED } as WPARAM, 0);
            }
        }
    }

    fn checked(&self, id: u16) -> bool {
        self.ctl(id)
            .map(|h| unsafe { SendMessageW(h, BM_GETCHECK, 0, 0) } == BST_CHECKED as LRESULT)
            .unwrap_or(false)
    }

    /// Enables the cross fields only while the cross is turned on.
    pub fn update_enabled(&self) {
        let on = self.checked(ID_CROSS);
        for id in [ID_CROSS_W, ID_DASH, ID_GAP] {
            if let Some(h) = self.ctl(id) {
                unsafe {
                    EnableWindow(h, on as i32);
                    EnableWindow(GetDlgItem(self.hwnd, (id + 100) as i32), on as i32);
                }
            }
        }
    }

    fn set_color(&mut self, rgb: u32) {
        self.color = rgb & 0xFFFFFF;
        unsafe {
            let brush = CreateSolidBrush(colorref(self.color));
            let old = SWATCH_BRUSH.with(|b| b.replace(brush));
            if !old.is_null() {
                DeleteObject(old);
            }
            if let Some(h) = self.ctl(ID_SWATCH) {
                InvalidateRect(h, null(), 1);
            }
        }
    }

    pub fn choose_color(&mut self) {
        let mut custom = CUSTOM_COLORS.with(|c| c.get());
        let mut cc: CHOOSECOLORW = unsafe { std::mem::zeroed() };
        cc.lStructSize = std::mem::size_of::<CHOOSECOLORW>() as u32;
        cc.hwndOwner = self.hwnd;
        cc.rgbResult = colorref(self.color);
        cc.lpCustColors = custom.as_mut_ptr();
        cc.Flags = CC_RGBINIT | CC_FULLOPEN;
        if unsafe { ChooseColorW(&mut cc) } != 0 {
            self.set_color(colorref(cc.rgbResult)); // COLORREF <-> RGB swap is symmetric
        }
        CUSTOM_COLORS.with(|c| c.set(custom));
    }

    pub fn autostart_checked(&self) -> bool {
        self.checked(ID_AUTOSTART)
    }

    /// Reads and validates all fields. On error, focuses the offending field
    /// and returns a message for the user.
    pub fn read(&self) -> Result<(Settings, bool), String> {
        let mut s = Settings::default();
        unsafe {
            let hk = self.ctl(ID_HOTKEY).map(|h| SendMessageW(h, HKM_GETHOTKEY, 0, 0) as u32).unwrap_or(0);
            let vk = hk & 0xFF;
            let flags = (hk >> 8) & 0xFF;
            if vk == 0 {
                self.focus(ID_HOTKEY);
                return Err("Click the Hotkey field and press the key combination you want to use.".into());
            }
            let mut mods = 0;
            if flags & HOTKEYF_SHIFT != 0 {
                mods |= MOD_SHIFT;
            }
            if flags & HOTKEYF_CONTROL != 0 {
                mods |= MOD_CONTROL;
            }
            if flags & HOTKEYF_ALT != 0 {
                mods |= MOD_ALT;
            }
            let is_fkey = (0x70..=0x87).contains(&vk);
            if mods == 0 && !is_fkey {
                self.focus(ID_HOTKEY);
                return Err("The hotkey needs Ctrl, Alt or Shift (or use a function key such as F8).".into());
            }
            s.hotkey_mods = mods;
            s.hotkey_vk = vk;
        }
        for &(id, label, (lo, hi)) in &NUMS {
            let text = self.text(id);
            match text.trim().parse::<u32>() {
                Ok(v) if v >= lo && v <= hi => num_set(&mut s, id, v),
                _ => {
                    self.focus(id);
                    let name = label.trim_end_matches(':');
                    return Err(format!("{name} must be a number from {lo} to {hi}."));
                }
            }
        }
        s.color = self.color;
        s.show_cross = self.checked(ID_CROSS);
        Ok((s, self.checked(ID_AUTOSTART)))
    }

    fn text(&self, id: u16) -> String {
        let Some(h) = self.ctl(id) else { return String::new() };
        let mut buf = [0u16; 32];
        let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn focus(&self, id: u16) {
        if let Some(h) = self.ctl(id) {
            unsafe {
                SetFocus(h);
                SendMessageW(h, EM_SETSEL, 0, -1isize as LPARAM);
            }
        }
    }

    pub fn show_error(&self, text: &str) {
        let text = wide(text);
        let caption = wide("Askeri Ruler");
        unsafe {
            MessageBoxW(self.hwnd, text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONWARNING);
        }
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            CURRENT.with(|c| c.set(null_mut()));
            DestroyWindow(self.hwnd);
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            let b = SWATCH_BRUSH.with(|b| b.replace(null_mut()));
            if !b.is_null() {
                DeleteObject(b);
            }
        }
    }
}

fn row_y(row: i32) -> i32 {
    MARGIN + row * ROW
}

const DM_GETDEFID: u32 = WM_USER;
const DC_HASDEFID: u32 = 0x534B;

pub unsafe extern "system" fn settings_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            if code == BN_CLICKED {
                push(Event::SettingsCommand(id));
            }
            0
        }
        WM_CTLCOLORSTATIC => {
            if GetDlgCtrlID(lparam as HWND) == ID_SWATCH as i32 {
                let b = SWATCH_BRUSH.with(|b| b.get());
                if !b.is_null() {
                    return b as LRESULT;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            push(Event::SettingsCommand(ID_CANCEL));
            0
        }
        WM_DPICHANGED => {
            let r = &*(lparam as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            push(Event::SettingsDpiChanged((wparam & 0xFFFF) as u32));
            0
        }
        DM_GETDEFID => ((DC_HASDEFID << 16) | IDOK as u32) as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
