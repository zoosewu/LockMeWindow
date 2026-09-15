# LockMeWindow

A small Windows 10/11 x64 utility that confines the cursor while a configured application is in the foreground.

LockMeWindow runs without administrator rights, including when the configured application runs as administrator.

## Download

Download `lock-me-window.exe` from the [latest release](https://github.com/zoosewu/LockMeWindow/releases/latest). To verify it, compare the output below with `lock-me-window.exe.sha256` from the same release:

```powershell
(Get-FileHash .\lock-me-window.exe -Algorithm SHA256).Hash
```

## Use

1. Launch `lock-me-window.exe`.
2. Select a running application or enter an executable name/full path.
3. Choose `Window` or `Monitor`, then select `Add`.
4. Use `Refresh` to rescan running applications.
5. Enable `Start with Windows` to launch LockMeWindow in the tray when you sign in.

Settings are saved immediately to `%APPDATA%\LockMeWindow\settings.json`. The tray icon remains visible while LockMeWindow is running. Minimize or close the window to hide it; left-click the tray icon to restore it or right-click and choose **Exit** to unlock the cursor and quit.

Only one instance can run at a time. Launching it again restores the existing window from the tray.

## Build

```powershell
cargo build --release
```

The executable is written to `target\release\lock-me-window.exe`.

LockMeWindow is a Rust reimplementation of [AutoCursorLock](https://github.com/James-LG/AutoCursorLock), used under its MIT license.
