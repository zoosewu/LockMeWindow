# WindowWarden

A small Windows 10/11 x64 utility that applies per-application rules based on the foreground window: it can confine the cursor while an application is in front, and mute an application while it is in the background.

WindowWarden runs without administrator rights, including when the configured application runs as administrator. The interface is available in English and Traditional Chinese and follows the Windows display language by default.

<a href="https://slint.dev"><picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-dark.svg"><img alt="#MadeWithSlint" src="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-light.svg" height="60"></picture></a>

## Download

Download `window-warden.exe` from the [latest release](https://github.com/zoosewu/LockMeWindow/releases/latest). To verify it, compare the output below with `window-warden.exe.sha256` from the same release:

```powershell
(Get-FileHash .\window-warden.exe -Algorithm SHA256).Hash
```

## Use

1. Launch `window-warden.exe`.
2. On the **Applications** tab, select **Add…** and pick a running application or enter its path or executable name. Optionally enter a display name to show instead of the path. Under **Features**, choose what WindowWarden does for it — one, both or neither:
   - **Lock cursor** keeps the cursor inside the **Application window** or on the **Monitor showing the application** while it is in the foreground.
   - **Mute in background** mutes the application whenever another window is in the foreground, and restores its sound when it comes back. A mute you set yourself in the volume mixer is left as it is.

   Then select **Save**. Use **Refresh** to rescan running applications.
3. Select an entry to **Edit…** or **Remove** it, or double-click it to edit. Both ask for confirmation.
4. On the **Settings** tab:
   - Enable **Start with Windows** to launch WindowWarden in the tray when you sign in.
   - Choose **Language** to follow the system or always use English or Traditional Chinese.
   - Select **Change…** to keep settings in another folder, or **Reset to default** to return to `%APPDATA%\WindowWarden`. If the chosen folder already contains settings, you can load them or overwrite them with the current settings.
   - Select **Export…** to save all settings to a JSON file, or **Import…** to replace all settings with one.

Settings are saved immediately to `settings.json` in the config folder. The tray icon remains visible while WindowWarden is running. Minimize or close the window to hide it; left-click the tray icon to restore it, or right-click and choose **Exit** to unlock the cursor and quit.

Only one instance can run at a time. Launching it again restores the existing window from the tray.

If WindowWarden stops unexpectedly while it has muted an application, it restores that application's sound the next time it starts.

## Build

```powershell
cargo build --release
```

The executable is written to `target\release\window-warden.exe`.

Interface strings are marked with `@tr` in `ui/*.slint`. Traditional Chinese translations live in `lang/zh-TW/LC_MESSAGES/window-warden.po` and are bundled into the executable. To list the strings, run:

```powershell
slint-tr-extractor --no-default-translation-context -o window-warden.pot ui/app.slint ui/tray.slint
```

WindowWarden is a Rust reimplementation of [AutoCursorLock](https://github.com/James-LG/AutoCursorLock), used under its MIT license.
