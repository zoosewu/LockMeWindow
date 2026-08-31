# LockMeWindow

A small Windows 10/11 x64 utility that confines the cursor while a configured application is in the foreground.

LockMeWindow requests administrator access at startup so it can inspect and control elevated games. Windows will show a UAC prompt on every launch.

## Use

1. Launch `lock-me-window.exe`.
2. Select a running application or enter an executable name/full path.
3. Choose `Window` or `Monitor`, then select `Add`.
4. Use `Refresh` to rescan running applications.

Settings are saved immediately to `%APPDATA%\LockMeWindow\settings.json`. Minimize the window to send it to the system tray; close it to unlock the cursor and exit.

## Build

```powershell
cargo build --release
```

The executable is written to `target\release\lock-me-window.exe`.

LockMeWindow is a Rust reimplementation of [AutoCursorLock](https://github.com/James-LG/AutoCursorLock), used under its MIT license.
