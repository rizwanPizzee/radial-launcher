# Radial Launcher

A tiny always-running Windows app. **Hold Shift + Middle mouse button** and a game-style
wheel of your favourite apps appears in the middle of the screen. Move toward a slice and
release to launch it.

![wheel](docs/preview.png)

## Use it

| Action | Result |
|---|---|
| Hold **Shift + Middle button** | Wheel opens in the centre of the monitor under the pointer; pointer moves to the centre |
| Move toward a slice, **release** | Launches that app. Releasing over the centre hub cancels |
| **Quick tap** (< 250 ms) instead of holding | Wheel stays open; **left-click** a slice to launch |
| **Right-click**, **Esc**, or middle-click again | Cancel |

The tray icon menu has: Enabled (pause the hook, e.g. before a game), Start with Windows,
Settings, Reload config, Quit. Settings opens a separate on-demand native window in its own
`radial-launcher-settings.exe`; that executable is not running alongside the launcher unless
you open it. Use it to choose a wheel design, colors, size, open position, pointer behavior,
foreground behavior, and up to 12 apps/files. Use **Browse...** to select an executable,
shortcut, document, or other file. Leave a row blank to remove it. Saving settings refreshes
an already-running launcher automatically.

## Build

You need Windows 10/11 and Rust (https://rustup.rs). The default MSVC toolchain also wants
the "Desktop development with C++" workload from Visual Studio Build Tools.

```
cargo build --release
```

The build produces `target\release\radial-launcher.exe` and
`target\release\radial-launcher-settings.exe`. Keep both files in the same folder; the tray
menu launches the Settings executable beside the main launcher. There is no installer or
runtime dependency.

No local toolchain? Push this folder to GitHub and run the **build** workflow
(`.github/workflows/main.yml`); it uploads both executables as a downloadable artifact.

## Configure

Config lives in `%APPDATA%\RadialLauncher\config.json` and is created on first run.
Most settings can be changed from the tray menu's **Settings...** window; manual edits can
be applied with **Reload config**.

```json
{
  "size": 520,
  "center": "screen",
  "restore_cursor": false,
  "accent": "#4C9AFF",
  "wheel_color": "#16181E",
  "hub_color": "#101115",
  "design": "segments",
  "focus_app": true,
  "items": [
    { "name": "Notepad",    "path": "%SystemRoot%\\System32\\notepad.exe" },
    { "name": "Calculator", "path": "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App" },
    { "name": "My project", "path": "C:\\Program Files\\Microsoft VS Code\\Code.exe", "args": "\"D:\\work\\project\"" }
  ]
}
```

* `items`: 1 to 12 entries, clockwise from the top. `path` can be an .exe, .lnk, document,
  folder, URL, or `shell:AppsFolder\<AppUserModelID>` for Store apps (run
  `Get-StartApps` in PowerShell to list the IDs). `%ENV_VARS%` are expanded.
* `center`: `"screen"` (centre of the monitor; pointer is moved there) or `"cursor"` (wheel
  opens under the pointer).
* `restore_cursor`: when `true`, restore the pointer to its pre-wheel position. Defaults to
  `false`, so it remains where you moved it in the wheel.
* `design`: one of `segments`, `band`, `orbs`, `halo`, `arc`, or `disc`.
* `accent`, `wheel_color`, and `hub_color`: editable `#RRGGBB` colors. `focus_app` asks
  Windows to foreground/restore the selected app after launch.
* Icons are loaded from the app itself. If one can't be loaded, a coloured letter is used.

Run `radial-launcher-settings.exe` to open settings directly. The always-on hook and overlay
remain lightweight; the settings executable is not running when its window is closed.

## Design notes (why it stays light)

* Idle = the UI thread blocks in `GetMessageW`; the hook thread blocks too. 0 % CPU.
* The mouse-hook callback only reads a few atomics and posts a message; all work happens on
  the UI thread, so the pointer never lags.
* The overlay is a layered, click-through, no-activate window drawn with software
  rendering (`tiny-skia`); no GPU, no browser engine, no framework.
* App icons, labels, and render buffers are allocated only while the wheel is open and
  released when it closes. The font is loaded only while icons/labels are being built, then
  dropped. The settings UI runs only on demand as a separate process, so neither its controls
  nor the wheel's image cache increase idle launcher memory.

## Known limitations

* Windows blocks hooks from seeing input over **elevated (admin) windows** unless this app is
  also run as administrator.
* The wheel can't draw over **exclusive-fullscreen** games (borderless-windowed is fine). Use the
  tray "Enabled" switch for games with strict anti-cheat.
* **Esc** is also delivered to the app under the wheel (keyboard input isn't intercepted);
  right-click cancels without side effects.
* Unsigned exes that install a global mouse hook can trigger antivirus heuristics.
* Only the platform-independent core (config, geometry, drawing) is covered by unit tests
  (`cargo test --lib`). The Win32 layer is type-checked but has not been run on a real
  Windows machine yet.

## Layout

```
src/lib.rs      platform-independent core (config, text, wheel geometry + drawing)
src/win/        Windows layer: hook.rs, mod.rs (overlay + lifecycle), tray.rs, icons.rs, autostart.rs
examples/       preview.rs renders the wheel to PNG on any OS
```
