# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What this is

Askeri Ruler: a Windows-only tray app (Rust, Win32) that draws an on-screen red guide line
following the mouse, rotatable around a pivot dot, lockable and click-through. Used for dental
image measurements. Distributed via Microsoft Store (MSIX) and, planned, a signed standalone
installer.

## Commands

```powershell
cargo build --release      # target\release\askeri-ruller.exe (release = no console window)
cargo build                # debug build keeps a console for eprintln! debugging
cargo test                 # unit tests: raster.rs, settings.rs
powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1            # MSIX
powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1 -Register  # dev install
powershell -ExecutionPolicy Bypass -File scripts\smoke-test.ps1              # end-to-end check
powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1       # dist\ setup.exe + zip
```

Releases: pushing tag `vX.Y.Z` (must equal `Cargo.toml` version) runs
`.github/workflows/release.yml`, which publishes the installer and zip to GitHub Releases.
See RELEASING.md. Signing is optional via a `SIGN_COMMAND` containing `{file}`.

- The shell is Windows PowerShell 5.1. If `cargo` is not on PATH in an existing session, add
  `$env:USERPROFILE\.cargo\bin`.
- The app is single-instance: stop a running copy before launching another
  (`Get-Process askeri-ruller | Stop-Process`), and before rebuilding (the exe is locked).
- A dev-registered MSIX runs from `target\msix\stage`; rebuilding that folder while the
  packaged app runs will fail.

## Architecture (read before changing behavior)

- **Event queue, no logic in window procedures.** Window procs and the keyboard hook only push
  `events::Event`s; `main::run_loop` drains Win32 messages, then feeds events to `App::handle`,
  then renders at most once (`render_if_dirty`). This avoids Win32 re-entrancy (many APIs call
  window procs synchronously) and coalesces 1000 Hz raw mouse input into one redraw per batch.
  Keep it this way: never borrow/modify `App` from a window proc.
- **State machine** (`app.rs`): Idle → Placing (hotkey) → Rotating (RMB down) → Locked (RMB up or
  LMB). Hotkey while locked adds another line. Esc clears everything.
- **Overlays** (`overlay.rs`): one per monitor, each a premultiplied-BGRA DIB shown through a
  stack of 96 px-high layered *strip* windows updated with `UpdateLayeredWindowIndirect` and
  per-strip dirty rects. Layered-window cost scales with dirty area; a full-screen diagonal
  redraw was ~10 ms vs ~2.7 ms with strips. Don't collapse back to one window.
  Interactive mode = background alpha 1 (invisible but hit-testable, hides cursor);
  locked mode = alpha 0 + `WS_EX_TRANSPARENT` (click-through).
- **Coordinates are physical pixels** (PerMonitorV2 via manifest + API). Lines store pivots in
  virtual-screen coords; on display change they're remapped by monitor device name.
- **Rotation** uses raw input deltas (`RIDEV_INPUTSINK`) with the hidden cursor clipped to the
  pivot. Absolute raw input (RDP/tablets) is converted to deltas in `main::read_raw_mouse`.
- **Esc hook** (`hook.rs`): `WH_KEYBOARD_LL`, installed only while lines exist (or Esc is held),
  swallows Esc down *and* the matching up.
- **Settings window** (`settings_ui.rs`): plain Win32 controls laid out in 96-DPI units and
  re-laid out on `WM_DPICHANGED`. The global hotkey is unregistered while it is open so the
  hotkey control can capture the combination.
- **Autostart** (`autostart.rs`): Run registry key when unpackaged; WinRT `StartupTask` when
  packaged (`GetCurrentPackageFullName`). WinRT calls run on a worker thread in the
  process-wide MTA kept alive by `CoIncrementMTAUsage` — do **not** `RoInitialize` per
  short-lived thread (it crashed in combase.dll because windows-rs caches activation factories).

- **Single instance & lifecycle** (`main.rs`): a named mutex. A second copy started with
  `--autostart` (Run key) or by the packaged StartupTask exits silently; one started by the
  user posts `WM_APP+2` so the running copy opens Settings. `WM_CLOSE` on the main window
  (what `taskkill` without /f and the installer send) quits cleanly; overlays ignore WM_CLOSE.
  The Run value name `AskeriRuller` and `--autostart` are shared with `installer\askeri-ruler.iss`.

## Conventions

- Win32 via `windows-sys` (raw, stable). The `windows` crate is used **only** for WinRT
  (`StartupTask`). Prefer extending `windows-sys` features in `Cargo.toml` over adding crates.
- No binary art in the repo: the icon is drawn in `src/icon_art.rs`; `build.rs` writes `app.ico`
  and `examples/gen_assets.rs` writes Store PNGs. `docs/icon.png` is just a README image —
  regenerate it from `target\msix\listing\logo-300x300.png` if the icon changes.
- Windows constants not exported by windows-sys (e.g. `SS_LEFT`, `DM_GETDEFID`) are written as
  literals with a comment.
- Keep `raster.rs` pure (no Windows deps) and covered by unit tests.

## Product decisions (don't revisit without the user)

- Line: anti-aliased, default 2 px red; round pivot dot (default 10 px) shown only while
  placing/rotating.
- Dashed X at ±45° shows while placing; removed once RMB rotation starts; kept if the line is
  locked by left click.
- Esc removes **all** lines and is swallowed (other apps never see it).
- Hotkey while a line is locked **adds** a new line.
- No "hide from screenshots" option (explicitly declined).
- Store listing must not make medical/diagnostic claims (see STORE.md).

## Verifying changes

Unit tests cover math only. For behavior, run `scripts\smoke-test.ps1`: it launches the
release build, sends the hotkey/clicks/Esc with `keybd_event`/`mouse_event`, and counts red
pixels in the cursor's row (physical coordinates via `GetSystemMetrics`), plus checks the
settings window and a simulated `WM_DISPLAYCHANGE`. It drives the real desktop, so tell the
user before running it. For UI layout, screenshot the window (`CopyFromScreen`) and look at it.
For performance, time `Overlay::draw` temporarily rather than guessing.

## Packaging notes

- `packaging/AppxManifest.xml` is a template; `build-msix.ps1` fills `$Tokens$` from
  `packaging/package.json` and the version from `Cargo.toml` (`x.y.z` → `x.y.z.0`).
- `build-msix.ps1` strips the `<packaging>` section from the generated `priconfig.xml` so all
  logo scales stay in one `resources.pri` (single package, not a bundle).
- The Store signs uploads; local sideload needs Developer Mode (`-Register`) or a trusted cert.
