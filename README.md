# LockMeWindow

A small Windows 10/11 x64 utility that confines the cursor while a configured application is in the foreground.

LockMeWindow runs without administrator rights, including when the configured application runs as administrator. The interface follows the Windows display language (English or Traditional Chinese).

<a href="https://slint.dev"><picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-dark.svg"><img alt="#MadeWithSlint" src="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-light.svg" height="60"></picture></a>

## Download

Download `lock-me-window.exe` from the [latest release](https://github.com/zoosewu/LockMeWindow/releases/latest). To verify it, compare the output below with `lock-me-window.exe.sha256` from the same release:

```powershell
(Get-FileHash .\lock-me-window.exe -Algorithm SHA256).Hash
```

## Use

1. Launch `lock-me-window.exe`.
2. On the **Applications** tab, select **Add…**, pick a running application or enter an executable name/full path, choose `Window` or `Monitor`, then select **Save**. Use **Refresh** to rescan running applications.
3. Select an entry to **Edit…** or **Remove** it. Both ask for confirmation.
4. On the **Settings** tab:
   - Enable **Start with Windows** to launch LockMeWindow in the tray when you sign in.
   - Select **Change…** to keep settings in another folder, or **Reset to default** to return to `%APPDATA%\LockMeWindow`. If the chosen folder already contains settings, you can load them or overwrite them with the current settings.
   - Select **Export…** to save all settings to a JSON file, or **Import…** to replace all settings with one.

Settings are saved immediately to `settings.json` in the config folder. The tray icon remains visible while LockMeWindow is running. Minimize or close the window to hide it; left-click the tray icon to restore it, or right-click and choose **Exit** to unlock the cursor and quit.

Only one instance can run at a time. Launching it again restores the existing window from the tray.

## Build

```powershell
cargo build --release
```

The executable is written to `target\release\lock-me-window.exe`.

Interface strings are marked with `@tr` in `ui/*.slint`. Traditional Chinese translations live in `lang/zh-TW/LC_MESSAGES/lock-me-window.po` and are bundled into the executable. To list the strings, run:

```powershell
slint-tr-extractor --no-default-translation-context -o lock-me-window.pot ui/app.slint ui/tray.slint
```

LockMeWindow is a Rust reimplementation of [AutoCursorLock](https://github.com/James-LG/AutoCursorLock), used under its MIT license.
