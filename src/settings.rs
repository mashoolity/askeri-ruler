//! User settings: defaults, validation ranges and INI-style persistence in
//! `%APPDATA%\AskeriRuller\settings.ini`.

use std::path::PathBuf;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

use crate::raster::Dash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// `MOD_*` flags for `RegisterHotKey` (without `MOD_NOREPEAT`).
    pub hotkey_mods: u32,
    /// Virtual-key code of the hotkey.
    pub hotkey_vk: u32,
    /// Line color as 0xRRGGBB.
    pub color: u32,
    pub line_width: u32,
    pub dot_diameter: u32,
    pub show_cross: bool,
    pub cross_width: u32,
    pub dash_length: u32,
    pub dash_gap: u32,
    /// Degrees of rotation per 100 px of vertical mouse movement.
    pub rotation_speed: u32,
    /// Ctrl-snap step in degrees.
    pub snap_step: u32,
}

/// Inclusive (min, max) ranges for numeric settings.
pub const LINE_WIDTH_RANGE: (u32, u32) = (1, 10);
pub const DOT_RANGE: (u32, u32) = (2, 40);
pub const CROSS_WIDTH_RANGE: (u32, u32) = (1, 10);
pub const DASH_RANGE: (u32, u32) = (1, 100);
pub const ROTATION_RANGE: (u32, u32) = (1, 90);
pub const SNAP_RANGE: (u32, u32) = (1, 90);

/// Shift divides the rotation speed by this for fine control.
pub const FINE_DIVISOR: f64 = 5.0;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey_mods: MOD_CONTROL | MOD_SHIFT,
            hotkey_vk: b'X' as u32,
            color: 0xFF0000,
            line_width: 2,
            dot_diameter: 10,
            show_cross: true,
            cross_width: 2,
            dash_length: 6,
            dash_gap: 6,
            rotation_speed: 10,
            snap_step: 15,
        }
    }
}

/// Drawing parameters derived from settings.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub color: u32,
    pub line_width: f64,
    pub dot_diameter: f64,
    pub show_cross: bool,
    pub cross_width: f64,
    pub dash: Dash,
}

impl Settings {
    pub fn style(&self) -> Style {
        let dot = self.dot_diameter as f64;
        Style {
            color: self.color,
            line_width: self.line_width as f64,
            dot_diameter: dot,
            show_cross: self.show_cross,
            cross_width: self.cross_width as f64,
            dash: Dash {
                on: self.dash_length as f64,
                off: self.dash_gap as f64,
                // Leave a little room around the dot.
                start: dot / 2.0 + 4.0,
            },
        }
    }

    pub fn hotkey_name(&self) -> String {
        hotkey_name(self.hotkey_mods, self.hotkey_vk)
    }

    fn clamp(mut self) -> Self {
        let c = |v: u32, (lo, hi): (u32, u32)| v.clamp(lo, hi);
        self.line_width = c(self.line_width, LINE_WIDTH_RANGE);
        self.dot_diameter = c(self.dot_diameter, DOT_RANGE);
        self.cross_width = c(self.cross_width, CROSS_WIDTH_RANGE);
        self.dash_length = c(self.dash_length, DASH_RANGE);
        self.dash_gap = c(self.dash_gap, DASH_RANGE);
        self.rotation_speed = c(self.rotation_speed, ROTATION_RANGE);
        self.snap_step = c(self.snap_step, SNAP_RANGE);
        self.color &= 0xFFFFFF;
        self.hotkey_mods &= MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN;
        if self.hotkey_vk == 0 || self.hotkey_vk > 0xFE {
            let d = Settings::default();
            self.hotkey_vk = d.hotkey_vk;
            self.hotkey_mods = d.hotkey_mods;
        }
        self
    }

    pub fn to_ini(&self) -> String {
        format!(
            "; Askeri Ruler settings\n\
             hotkey_mods = {}\n\
             hotkey_vk = {}\n\
             color = #{:06X}\n\
             line_width = {}\n\
             dot_diameter = {}\n\
             show_cross = {}\n\
             cross_width = {}\n\
             dash_length = {}\n\
             dash_gap = {}\n\
             rotation_speed = {}\n\
             snap_step = {}\n",
            self.hotkey_mods,
            self.hotkey_vk,
            self.color,
            self.line_width,
            self.dot_diameter,
            self.show_cross,
            self.cross_width,
            self.dash_length,
            self.dash_gap,
            self.rotation_speed,
            self.snap_step,
        )
    }

    /// Parses INI text. Unknown keys and invalid values fall back to defaults.
    pub fn from_ini(text: &str) -> Self {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let num = |cur: u32| v.parse::<u32>().unwrap_or(cur);
            match k {
                "hotkey_mods" => s.hotkey_mods = num(s.hotkey_mods),
                "hotkey_vk" => s.hotkey_vk = num(s.hotkey_vk),
                "color" => {
                    if let Ok(c) = u32::from_str_radix(v.trim_start_matches('#'), 16) {
                        s.color = c;
                    }
                }
                "line_width" => s.line_width = num(s.line_width),
                "dot_diameter" => s.dot_diameter = num(s.dot_diameter),
                "show_cross" => s.show_cross = v.eq_ignore_ascii_case("true") || v == "1",
                "cross_width" => s.cross_width = num(s.cross_width),
                "dash_length" => s.dash_length = num(s.dash_length),
                "dash_gap" => s.dash_gap = num(s.dash_gap),
                "rotation_speed" => s.rotation_speed = num(s.rotation_speed),
                "snap_step" => s.snap_step = num(s.snap_step),
                _ => {}
            }
        }
        s.clamp()
    }

    fn path() -> Option<PathBuf> {
        let appdata = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(appdata).join("AskeriRuller").join("settings.ini"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Self::from_ini(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = Self::path().ok_or_else(|| std::io::Error::other("APPDATA is not set"))?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, self.to_ini())
    }
}

/// Human-readable hotkey such as "Ctrl+Shift+X".
pub fn hotkey_name(mods: u32, vk: u32) -> String {
    let mut s = String::new();
    if mods & MOD_CONTROL != 0 {
        s.push_str("Ctrl+");
    }
    if mods & MOD_ALT != 0 {
        s.push_str("Alt+");
    }
    if mods & MOD_SHIFT != 0 {
        s.push_str("Shift+");
    }
    if mods & MOD_WIN != 0 {
        s.push_str("Win+");
    }
    s.push_str(&key_name(vk));
    s
}

fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => unsafe {
            let mut scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) as i32;
            // Keys on the navigation block need the "extended" bit for a correct name.
            if matches!(vk as u16, VK_PRIOR | VK_NEXT | VK_END | VK_HOME | VK_LEFT | VK_UP | VK_RIGHT | VK_DOWN
                | VK_INSERT | VK_DELETE | VK_DIVIDE | VK_NUMLOCK)
            {
                scan |= 0x100;
            }
            let mut buf = [0u16; 64];
            let n = GetKeyNameTextW(scan << 16, buf.as_mut_ptr(), buf.len() as i32);
            if n > 0 {
                String::from_utf16_lossy(&buf[..n as usize])
            } else {
                format!("Key {vk:#04X}")
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_roundtrip() {
        let s = Settings {
            color: 0x12AB34,
            line_width: 3,
            show_cross: false,
            hotkey_mods: MOD_ALT,
            hotkey_vk: 0x71,
            ..Settings::default()
        };
        assert_eq!(Settings::from_ini(&s.to_ini()), s);
    }

    #[test]
    fn ini_clamps_and_ignores_garbage() {
        let s = Settings::from_ini("line_width = 99\ndot_diameter = abc\nfoo = 1\ncolor = #00ff00\n");
        assert_eq!(s.line_width, 10);
        assert_eq!(s.dot_diameter, Settings::default().dot_diameter);
        assert_eq!(s.color, 0x00FF00);
    }

    #[test]
    fn hotkey_names() {
        assert_eq!(hotkey_name(MOD_CONTROL | MOD_SHIFT, b'X' as u32), "Ctrl+Shift+X");
        assert_eq!(hotkey_name(MOD_ALT, 0x71), "Alt+F2");
    }
}
