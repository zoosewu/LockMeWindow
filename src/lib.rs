use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::mem::zeroed;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HANDLE,
    RECT, SetLastError, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ClipCursor, GetClipCursor};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum LockTarget {
    #[default]
    Window,
    Monitor,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ManagedApplication {
    pub identity: String,
    pub target: LockTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ManagedApplication {
    pub fn matches(&self, process_name: &str, executable_path: Option<&Path>) -> bool {
        if is_path(&self.identity) {
            return executable_path
                .map(|path| {
                    normalize_path(&self.identity) == normalize_path(&path.to_string_lossy())
                })
                .unwrap_or_else(|| normalize_name(&self.identity) == normalize_name(process_name));
        }

        normalize_name(&self.identity) == normalize_name(process_name)
    }

    pub fn same_identity(&self, other: &str) -> bool {
        match (is_path(&self.identity), is_path(other)) {
            (true, true) => normalize_path(&self.identity) == normalize_path(other),
            (false, false) => normalize_name(&self.identity) == normalize_name(other),
            _ => false,
        }
    }

    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.identity)
    }
}

fn is_path(value: &str) -> bool {
    value.contains(['\\', '/']) || value.contains(':')
}

fn normalize_path(value: &str) -> String {
    value.trim().replace('/', "\\").to_lowercase()
}

fn normalize_name(value: &str) -> String {
    let file_name = value.trim().rsplit(['\\', '/']).next().unwrap_or_default();
    let lower = file_name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

#[derive(Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Settings {
    pub apps: Vec<ManagedApplication>,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub language: Language,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum Language {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
}

// Folder names under `lang/`; English is the untranslated default.
const TRANSLATIONS: [&str; 1] = ["zh-TW"];

impl Language {
    // Returns the bundled translation to select, where "" means English.
    pub fn translation(self, system_locale: Option<&str>) -> &'static str {
        match self {
            Language::System => system_locale.map_or("", bundled_language),
            Language::English => "",
            Language::TraditionalChinese => "zh-TW",
        }
    }
}

// Matches a locale the same way Slint does: exact name first, then the language part.
fn bundled_language(locale: &str) -> &'static str {
    fn base(locale: &str) -> &str {
        locale
            .find(['-', '_', '@'])
            .map_or(locale, |index| &locale[..index])
    }
    TRANSLATIONS
        .iter()
        .find(|translation| **translation == locale)
        .or_else(|| {
            TRANSLATIONS
                .iter()
                .find(|translation| base(translation) == base(locale))
        })
        .copied()
        .unwrap_or("")
}

impl Settings {
    pub fn load_from(path: &Path) -> Result<Self> {
        load_json(path)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        save_json(path, self)
    }
}

pub const SETTINGS_FILE_NAME: &str = "settings.json";
pub const LOCATION_FILE_NAME: &str = "location.json";

pub fn default_config_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(std::env::var("APPDATA")?).join("LockMeWindow"))
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ConfigLocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<PathBuf>,
}

impl ConfigLocation {
    pub fn load_from(path: &Path) -> Result<Self> {
        load_json(path)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        save_json(path, self)
    }

    pub fn settings_path(&self, default_dir: &Path) -> PathBuf {
        self.directory
            .as_deref()
            .unwrap_or(default_dir)
            .join(SETTINGS_FILE_NAME)
    }
}

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub fn ensure_cursor_clip(expected: RECT) -> bool {
    unsafe {
        let mut actual: RECT = zeroed();
        if GetClipCursor(&mut actual) != 0 && same_rect(actual, expected) {
            return true;
        }
        ClipCursor(&expected) != 0 && GetClipCursor(&mut actual) != 0 && same_rect(actual, expected)
    }
}

pub fn startup_command(executable: &Path) -> String {
    format!("\"{}\" --minimized", executable.display())
}

pub fn set_user_registry_string(subkey: &str, name: &str, value: Option<&str>) -> Result<()> {
    let subkey = wide(subkey);
    let name = wide(name);
    let status = unsafe {
        match value {
            Some(value) => {
                let data = wide(value);
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    size_of_val(data.as_slice()) as u32,
                )
            }
            None => match RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name.as_ptr()) {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                status => status,
            },
        }
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32).into())
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

pub struct SingleInstance(HANDLE);

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn claim_single_instance(name: &str) -> Result<Option<SingleInstance>> {
    let name: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let handle = CreateMutexW(null(), 0, name.as_ptr());
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            Ok(None)
        } else {
            Ok(Some(SingleInstance(handle)))
        }
    }
}

pub struct NamedEvent(HANDLE);

// Event handles can be used from any thread.
unsafe impl Send for NamedEvent {}

impl Drop for NamedEvent {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

impl NamedEvent {
    pub fn open_or_create(name: &str) -> Result<Self> {
        let name = wide(name);
        let handle = unsafe { CreateEventW(null(), 0, 0, name.as_ptr()) };
        if handle.is_null() {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(Self(handle))
        }
    }

    pub fn signal(&self) -> Result<()> {
        if unsafe { SetEvent(self.0) } == 0 {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(())
        }
    }

    pub fn wait(&self, timeout_ms: u32) -> bool {
        unsafe { WaitForSingleObject(self.0, timeout_ms) == WAIT_OBJECT_0 }
    }
}

fn same_rect(left: RECT, right: RECT) -> bool {
    left.left == right.left
        && left.top == right.top
        && left.right == right.right
        && left.bottom == right.bottom
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ClipCursor, GetClipCursor, GetCursorPos};

    struct RestoreClip(RECT);

    impl Drop for RestoreClip {
        fn drop(&mut self) {
            unsafe { ClipCursor(&self.0) };
        }
    }

    struct DeleteTestKey(String);

    impl Drop for DeleteTestKey {
        fn drop(&mut self) {
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr()) };
        }
    }

    fn user_registry_string(subkey: &str, name: &str) -> Option<String> {
        let subkey = wide(subkey);
        let name = wide(name);
        let mut buffer = [0u16; 512];
        let mut size = size_of_val(&buffer) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS)
            .then(|| String::from_utf16(&buffer[..size as usize / 2 - 1]).unwrap())
    }

    #[test]
    fn one_identity_matches_path_or_process_name() {
        let by_path = ManagedApplication {
            identity: r"C:\Games\ZZZ\zzz.exe".into(),
            target: LockTarget::Window,
            name: None,
        };
        let by_name = ManagedApplication {
            identity: "ZZZ.EXE".into(),
            target: LockTarget::Window,
            name: None,
        };

        assert!(by_path.matches("zzz.exe", Some(Path::new(r"c:\games\zzz\ZZZ.EXE"))));
        assert!(by_path.matches("zzz.exe", None));
        assert!(by_name.matches("zzz", Some(Path::new(r"D:\Other\zzz.exe"))));
        assert!(!by_path.matches("zzz.exe", Some(Path::new(r"D:\Other\zzz.exe"))));
        assert!(by_name.same_identity("zzz"));
        assert!(
            ManagedApplication {
                identity: "ŻÓŁĆ.EXE".into(),
                target: LockTarget::Window,
                name: None,
            }
            .matches("żółć", None)
        );
    }

    #[test]
    fn settings_round_trip_as_json() {
        let path = std::env::temp_dir().join(format!(
            "lock-me-window-settings-{}.json",
            std::process::id()
        ));
        let settings = Settings {
            apps: vec![ManagedApplication {
                identity: r"C:\Games\zzz.exe".into(),
                target: LockTarget::Monitor,
                name: Some("ZZZ".into()),
            }],
            start_with_windows: true,
            language: Language::TraditionalChinese,
        };

        settings.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), settings);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn display_name_falls_back_to_identity() {
        let mut app = ManagedApplication {
            identity: r"C:\Games\zzz.exe".into(),
            target: LockTarget::default(),
            name: None,
        };
        assert_eq!(app.display_name(), r"C:\Games\zzz.exe");

        app.name = Some("  ".into());
        assert_eq!(app.display_name(), r"C:\Games\zzz.exe");

        app.name = Some(" ZZZ ".into());
        assert_eq!(app.display_name(), "ZZZ");
    }

    #[test]
    fn language_selects_bundled_translation() {
        assert_eq!(Language::English.translation(Some("zh-TW")), "");
        assert_eq!(
            Language::TraditionalChinese.translation(Some("en-US")),
            "zh-TW"
        );
        assert_eq!(Language::System.translation(Some("zh-TW")), "zh-TW");
        assert_eq!(Language::System.translation(Some("zh-HK")), "zh-TW");
        assert_eq!(Language::System.translation(Some("en-US")), "");
        assert_eq!(Language::System.translation(None), "");
    }

    #[test]
    fn old_settings_default_name_and_language() {
        let settings: Settings = serde_json::from_str(
            r#"{"apps":[{"identity":"zzz.exe","target":"Window"}],"start_with_windows":true}"#,
        )
        .unwrap();
        assert_eq!(settings.apps[0].name, None);
        assert_eq!(settings.language, Language::System);
    }

    #[test]
    fn old_settings_default_startup_to_off() {
        let settings: Settings = serde_json::from_str(r#"{"apps":[]}"#).unwrap();
        assert!(!settings.start_with_windows);
    }

    #[test]
    fn config_location_uses_default_directory_until_customized() {
        let default_dir = Path::new(r"C:\Users\me\AppData\Roaming\LockMeWindow");
        let mut location = ConfigLocation::default();
        assert_eq!(
            location.settings_path(default_dir),
            default_dir.join(SETTINGS_FILE_NAME)
        );

        location.directory = Some(PathBuf::from(r"D:\Sync\LockMeWindow"));
        assert_eq!(
            location.settings_path(default_dir),
            Path::new(r"D:\Sync\LockMeWindow\settings.json")
        );
    }

    #[test]
    fn config_location_round_trips_and_defaults_when_missing() {
        let dir =
            std::env::temp_dir().join(format!("lock-me-window-location-{}", std::process::id()));
        let path = dir.join(LOCATION_FILE_NAME);
        assert_eq!(
            ConfigLocation::load_from(&path).unwrap(),
            ConfigLocation::default()
        );

        let location = ConfigLocation {
            directory: Some(PathBuf::from(r"D:\Sync\LockMeWindow")),
        };
        location.save_to(&path).unwrap();
        assert_eq!(ConfigLocation::load_from(&path).unwrap(), location);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_command_quotes_executable_and_starts_minimized() {
        assert_eq!(
            startup_command(Path::new(
                r"C:\Program Files\LockMeWindow\lock-me-window.exe"
            )),
            r#""C:\Program Files\LockMeWindow\lock-me-window.exe" --minimized"#
        );
    }

    #[test]
    fn user_registry_string_is_set_and_deleted() {
        let subkey = format!(r"Software\LockMeWindow.Test.{}", std::process::id());
        let _cleanup = DeleteTestKey(subkey.clone());
        let command = r#""C:\Tools\lock-me-window.exe" --minimized"#;

        set_user_registry_string(&subkey, "Startup", Some(command)).unwrap();
        assert_eq!(
            user_registry_string(&subkey, "Startup").as_deref(),
            Some(command)
        );

        set_user_registry_string(&subkey, "Startup", None).unwrap();
        assert_eq!(user_registry_string(&subkey, "Startup"), None);
        set_user_registry_string(&subkey, "Startup", None).unwrap();
    }

    #[test]
    fn named_event_wakes_another_handle_once() {
        let name = format!(r"Local\LockMeWindow.TestEvent.{}", std::process::id());
        let waiter = NamedEvent::open_or_create(&name).unwrap();
        let sender = NamedEvent::open_or_create(&name).unwrap();
        assert!(!waiter.wait(0));

        sender.signal().unwrap();
        assert!(waiter.wait(1000));
        assert!(!waiter.wait(0));
    }

    #[test]
    fn named_instance_is_exclusive_until_released() {
        let name = format!(r"Local\LockMeWindow.Test.{}", std::process::id());
        let first = claim_single_instance(&name).unwrap().unwrap();
        assert!(claim_single_instance(&name).unwrap().is_none());
        drop(first);
        assert!(claim_single_instance(&name).unwrap().is_some());
    }

    #[test]
    fn active_cursor_lock_recovers_after_external_clear() {
        unsafe {
            let mut original: RECT = zeroed();
            assert_ne!(GetClipCursor(&mut original), 0);
            let _restore = RestoreClip(original);

            let mut cursor: POINT = zeroed();
            assert_ne!(GetCursorPos(&mut cursor), 0);
            let expected = RECT {
                left: cursor.x - 100,
                top: cursor.y - 100,
                right: cursor.x + 100,
                bottom: cursor.y + 100,
            };
            assert_ne!(ClipCursor(&expected), 0);
            assert_ne!(ClipCursor(null()), 0);

            assert!(ensure_cursor_clip(expected));
            let mut actual: RECT = zeroed();
            assert_ne!(GetClipCursor(&mut actual), 0);
            assert!(same_rect(actual, expected));
        }
    }
}
