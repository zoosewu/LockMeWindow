# LockMeWindow

A small Windows 10/11 x64 utility that confines the cursor while a configured application is in the foreground.

LockMeWindow requests administrator access at startup so it can inspect and control elevated games. Windows will show a UAC prompt on every launch.

## Use

1. Launch `lock-me-window.exe`.
2. Select a running application or enter an executable name/full path.
3. Choose `Window` or `Monitor`, then select `Add`.
4. Use `Refresh` to rescan running applications.
5. Enable `Start with Windows` to launch LockMeWindow in the tray when you sign in.

Settings are saved immediately to `%APPDATA%\LockMeWindow\settings.json`. Minimize or close the window to send it to the system tray; left-click the tray icon to restore it or right-click and choose **Exit** to unlock the cursor and quit.

## Build

```powershell
cargo build --release
```

The executable is written to `target\release\lock-me-window.exe`.

LockMeWindow is a Rust reimplementation of [AutoCursorLock](https://github.com/James-LG/AutoCursorLock), used under its MIT license.
