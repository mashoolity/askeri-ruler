<p align="center">
  <img src="docs/icon.png" width="128" alt="Askeri Ruler icon">
</p>

# Askeri Ruler

A tiny Windows on-screen guide line for precise visual alignment, built for working with
dental images. Press a hotkey, a thin red line follows your mouse; rotate it around a pivot,
lock it in place, and keep working in any app underneath.

- **Always ready** – sits in the notification area; one global hotkey (default **Ctrl+Shift+X**).
- **Follows the mouse** – the line tracks the cursor vertically, the pivot dot tracks it horizontally.
  The cursor is hidden while placing so nothing covers the image.
- **Rotation** – hold the **right mouse button** and move the mouse up/down to rotate around the dot
  (**Shift** = fine, **Ctrl** = snap to 15° steps). Release to lock.
- **Dashed X guide** – a dashed cross at ±45° through the pivot while placing.
- **Click-through when locked** – locked lines never block clicks or typing in other apps.
- **Multiple lines** – press the hotkey again to add another line.
- **Esc removes all lines** from anywhere.
- **Multi-monitor and high-DPI aware**; lines survive resolution and monitor changes.
- **Native and lightweight** – a single ~500 KB Rust executable, no runtime, no network access.

## Usage

| Action | How |
|---|---|
| New line | **Ctrl+Shift+X** (or tray menu → *New line*) |
| Move line / pivot | Move the mouse |
| Lock without rotating | Left click (the dashed X stays on the line) |
| Rotate | Hold right button, move mouse up/down; release to lock (the X is removed) |
| Fine rotation / snap | Hold **Shift** / **Ctrl** while rotating |
| Remove all lines | **Esc** (or tray menu → *Clear all lines*) |
| Settings | Double-click the tray icon (or tray menu → *Settings…*) |

**Settings** let you change the hotkey, line color, line width, dot size, dashed-cross width and
dash pattern, rotation speed, snap step, and *Start with Windows*. They are stored in
`%APPDATA%\AskeriRuller\settings.ini`.

## Building from source

Requirements (Windows 10/11, x64):

- [Rust](https://rustup.rs) with the MSVC toolchain (`stable-x86_64-pc-windows-msvc`)
- Visual Studio 2022 Build Tools with the **Desktop development with C++** workload and the
  **Windows 11 SDK** (provides the linker, `rc.exe`, `makeappx`, `makepri`, `signtool`)

```powershell
cargo build --release          # target\release\askeri-ruller.exe
cargo test                     # unit tests (rasterizer, settings)
```

Or with winget:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.Windows11SDK.26100 --includeRecommended"
```

### MSIX package (Microsoft Store)

```powershell
powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1            # -> target\msix\*.msix
powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1 -Register  # local test install (Developer Mode)
```

Package identity lives in [`packaging/package.json`](packaging/package.json). See
[STORE.md](STORE.md) for the full Microsoft Store submission guide.

## Project layout

```
src/
  main.rs         entry point, window classes, message loop, raw mouse input
  app.rs          state machine: placing -> rotating -> locked, tray/settings actions
  overlay.rs      per-monitor transparent layered overlay (split into strips for fast redraws)
  raster.rs       anti-aliased line / dashed line / dot rasterizer (unit tested)
  hook.rs         low-level keyboard hook that swallows Esc while lines are visible
  settings.rs     settings model + INI persistence
  settings_ui.rs  native Win32 settings window
  tray.rs         notification-area icon and menu
  autostart.rs    Start with Windows (Run key, or StartupTask when packaged)
  icon_art.rs     procedural app icon (used by build.rs and the asset generator)
build.rs          generates app.ico, embeds icon + manifest + version info
res/app.manifest  PerMonitorV2 DPI awareness, common controls v6
examples/gen_assets.rs   Store / tile logo PNGs
packaging/        MSIX manifest template and build script
```

## Privacy

Askeri Ruler collects no data and makes no network connections. See [PRIVACY.md](PRIVACY.md).

## Disclaimer

Askeri Ruler is a visual guide tool. It is not a medical device and is not intended for
diagnosis or certified measurement.
