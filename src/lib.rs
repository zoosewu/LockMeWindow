use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::mem::zeroed;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE, RECT, SetLastError,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
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
}

impl Settings {
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(PathBuf::from(std::env::var("APPDATA")?)
        .join("LockMeWindow")
        .join("settings.json"))
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
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ClipCursor, GetClipCursor, GetCursorPos};

    struct RestoreClip(RECT);

    impl Drop for RestoreClip {
        fn drop(&mut self) {
            unsafe { ClipCursor(&self.0) };
        }
    }

    #[test]
    fn one_identity_matches_path_or_process_name() {
        let by_path = ManagedApplication {
            identity: r"C:\Games\ZZZ\zzz.exe".into(),
            target: LockTarget::Window,
        };
        let by_name = ManagedApplication {
            identity: "ZZZ.EXE".into(),
            target: LockTarget::Window,
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
            }],
            start_with_windows: true,
        };

        settings.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), settings);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn old_settings_default_startup_to_off() {
        let settings: Settings = serde_json::from_str(r#"{"apps":[]}"#).unwrap();
        assert!(!settings.start_with_windows);
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
