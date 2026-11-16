//! A tiny thread-local event queue.
//!
//! Window procedures and the keyboard hook only *translate* OS messages into
//! `Event`s and push them here. The main loop drains the queue and feeds the
//! events to `App`, which owns all state. This keeps window procedures free of
//! re-entrancy problems (Win32 often calls a window procedure synchronously from
//! inside another API call).

use std::cell::RefCell;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// Global hotkey (Ctrl+Shift+X) pressed.
    Hotkey,
    /// Mouse moved over an overlay while placing.
    MouseMove,
    /// Left button pressed on an overlay (locks the line without rotating).
    LeftDown,
    /// Right button pressed on an overlay (start rotating).
    RightDown,
    /// Right button released (lock the line).
    RightUp,
    /// Left button pressed on a locked line's dot (line id).
    HandleDown(u32),
    /// Mouse moved over (or, while dragging, captured by) a dot handle.
    HandleDrag,
    /// Left button released / capture lost while dragging a dot.
    HandleUp,
    /// Relative mouse movement from raw input (used while rotating).
    RawMouseDelta { dx: i32, dy: i32 },
    /// Our window lost mouse capture unexpectedly.
    CaptureLost,
    /// ESC pressed anywhere (swallowed by the keyboard hook).
    EscapePressed,
    /// ESC released (the hook can be removed if nothing is on screen).
    EscapeReleased,
    /// Tray icon right-clicked.
    TrayMenu,
    /// Open the settings window (tray double-click, or a second launch).
    ShowSettings,
    /// Quit the app (WM_CLOSE from taskkill / the installer).
    Exit,
    /// A button in the settings window was clicked (control id).
    SettingsCommand(u16),
    /// The settings window moved to a monitor with a different DPI.
    SettingsDpiChanged(u32),
    /// Explorer restarted, the tray icon must be re-added.
    TaskbarCreated,
    /// Monitor layout or resolution changed (debounced).
    DisplayChanged,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
}

pub fn push(e: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back(e));
}

pub fn pop() -> Option<Event> {
    QUEUE.with(|q| q.borrow_mut().pop_front())
}

pub fn is_empty() -> bool {
    QUEUE.with(|q| q.borrow().is_empty())
}
