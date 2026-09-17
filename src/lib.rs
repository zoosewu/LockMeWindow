use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::mem::zeroed;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HANDLE,
    RECT, SetLastError, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
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
    #[serde(default = "enabled")]
    pub lock_cursor: bool,
    #[serde(default)]
    pub mute_in_background: bool,
}

// Entries saved before features were selectable only locked the cursor.
fn enabled() -> bool {
    true
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
    Ok(PathBuf::from(std::env::var("APPDATA")?).join("WindowWarden"))
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

// The app shipped as LockMeWindow before; carry that installation's files over
// the first time it runs under the new name.
pub fn migrate_config(legacy_dir: &Path, config_dir: &Path) -> Result<bool> {
    let source = legacy_dir.join(SETTINGS_FILE_NAME);
    let target = config_dir.join(SETTINGS_FILE_NAME);
    if target.exists() || !source.exists() {
        return Ok(false);
    }

    std::fs::create_dir_all(config_dir)?;
    std::fs::copy(&source, &target)?;
    let location = legacy_dir.join(LOCATION_FILE_NAME);
    if location.exists() {
        std::fs::copy(location, config_dir.join(LOCATION_FILE_NAME))?;
    }
    Ok(true)
}

pub const MUTED_FILE_NAME: &str = "muted.json";

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSession {
    pub key: String,
    pub process_name: String,
    pub process_path: Option<PathBuf>,
    pub muted: bool,
}

impl AudioSession {
    // Sessions come and go; the program that owns them is what stays the same.
    pub fn owner(&self) -> String {
        self.process_path
            .as_deref()
            .map(|path| normalize_path(&path.to_string_lossy()))
            .unwrap_or_else(|| normalize_name(&self.process_name))
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct MuteActions {
    pub mute: Vec<String>,
    pub unmute: Vec<String>,
}

// Tracks the mutes WindowWarden added, so it only ever restores its own.
#[derive(Debug, Default)]
pub struct BackgroundMute {
    // Session key to owning program, for sessions muted here and not yet restored.
    muted: HashMap<String, String>,
    // Sessions the user unmuted while in the background; left alone until they come to the front.
    overridden: HashSet<String>,
    // Programs muted here whose sessions ended. Windows hands that mute to the program's next
    // session, so it is restored or adopted when one appears.
    pending: BTreeSet<String>,
}

impl BackgroundMute {
    pub fn with_pending(owners: BTreeSet<String>) -> Self {
        Self {
            pending: owners,
            ..Self::default()
        }
    }

    pub fn owners(&self) -> BTreeSet<String> {
        self.muted.values().chain(&self.pending).cloned().collect()
    }

    pub fn is_idle(&self) -> bool {
        self.muted.is_empty() && self.pending.is_empty()
    }

    pub fn plan(
        &mut self,
        apps: &[ManagedApplication],
        foreground: Option<(&str, Option<&Path>)>,
        sessions: &[AudioSession],
    ) -> MuteActions {
        self.forget_ended(sessions);
        let mut actions = MuteActions::default();
        let mut seen = BTreeSet::new();

        for session in sessions {
            let owner = session.owner();
            let pending = self.pending.contains(&owner);
            let wants_mute = apps.iter().any(|app| {
                app.mute_in_background
                    && app.matches(&session.process_name, session.process_path.as_deref())
                    && !foreground.is_some_and(|(name, path)| app.matches(name, path))
            });

            if wants_mute {
                if self.overridden.contains(&session.key) {
                    // The user unmuted it in the background; wait for the next switch.
                } else if self.muted.contains_key(&session.key) {
                    if !session.muted {
                        self.muted.remove(&session.key);
                        self.overridden.insert(session.key.clone());
                    }
                } else if !session.muted {
                    actions.mute.push(session.key.clone());
                    self.muted.insert(session.key.clone(), owner.clone());
                } else if pending {
                    self.muted.insert(session.key.clone(), owner.clone());
                }
            } else {
                self.overridden.remove(&session.key);
                let ours = self.muted.remove(&session.key).is_some();
                if (ours || pending) && session.muted {
                    actions.unmute.push(session.key.clone());
                }
            }
            seen.insert(owner);
        }

        self.pending.retain(|owner| !seen.contains(owner));
        actions
    }

    pub fn release_all(&mut self, sessions: &[AudioSession]) -> MuteActions {
        self.forget_ended(sessions);
        let mut actions = MuteActions::default();
        let mut released = BTreeSet::new();

        for session in sessions {
            let owner = session.owner();
            let ours = self.muted.remove(&session.key).is_some() || self.pending.contains(&owner);
            if ours {
                if session.muted {
                    actions.unmute.push(session.key.clone());
                }
                released.insert(owner);
            }
        }

        self.pending.retain(|owner| !released.contains(owner));
        self.overridden.clear();
        actions
    }

    fn forget_ended(&mut self, sessions: &[AudioSession]) {
        let live: HashSet<&str> = sessions
            .iter()
            .map(|session| session.key.as_str())
            .collect();
        let ended: Vec<String> = self
            .muted
            .keys()
            .filter(|key| !live.contains(key.as_str()))
            .cloned()
            .collect();
        for key in ended {
            if let Some(owner) = self.muted.remove(&key) {
                self.pending.insert(owner);
            }
        }
        self.overridden.retain(|key| live.contains(key.as_str()));
    }
}

pub fn load_muted_owners(path: &Path) -> Result<BTreeSet<String>> {
    load_json(path)
}

pub fn save_muted_owners(path: &Path, owners: &BTreeSet<String>) -> Result<()> {
    if !owners.is_empty() {
        return save_json(path, owners);
    }
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
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

pub fn user_registry_dword(subkey: &str, name: &str) -> Option<u32> {
    let subkey = wide(subkey);
    let name = wide(name);
    let mut value = 0u32;
    let mut size = size_of_val(&value) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
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
            lock_cursor: true,
            mute_in_background: false,
        };
        let by_name = ManagedApplication {
            identity: "ZZZ.EXE".into(),
            target: LockTarget::Window,
            name: None,
            lock_cursor: true,
            mute_in_background: false,
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
                lock_cursor: true,
                mute_in_background: false,
            }
            .matches("żółć", None)
        );
    }

    #[test]
    fn settings_round_trip_as_json() {
        let path = std::env::temp_dir().join(format!(
            "window-warden-settings-{}.json",
            std::process::id()
        ));
        let settings = Settings {
            apps: vec![ManagedApplication {
                identity: r"C:\Games\zzz.exe".into(),
                target: LockTarget::Monitor,
                name: Some("ZZZ".into()),
                lock_cursor: true,
                mute_in_background: false,
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
            lock_cursor: true,
            mute_in_background: false,
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
        assert!(settings.apps[0].lock_cursor);
        assert!(!settings.apps[0].mute_in_background);
        assert_eq!(settings.language, Language::System);
    }

    #[test]
    fn old_settings_default_startup_to_off() {
        let settings: Settings = serde_json::from_str(r#"{"apps":[]}"#).unwrap();
        assert!(!settings.start_with_windows);
    }

    fn managed(identity: &str, mute: bool) -> ManagedApplication {
        ManagedApplication {
            identity: identity.into(),
            target: LockTarget::Window,
            name: None,
            lock_cursor: true,
            mute_in_background: mute,
        }
    }

    fn session(key: &str, exe: &str, muted: bool) -> AudioSession {
        AudioSession {
            key: key.into(),
            process_name: exe.into(),
            process_path: None,
            muted,
        }
    }

    fn keys(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    const CHROME: Option<(&str, Option<&Path>)> = Some(("chrome.exe", None));
    const GAME: Option<(&str, Option<&Path>)> = Some(("game.exe", None));

    #[test]
    fn background_mute_follows_the_foreground() {
        let apps = [managed("game.exe", true)];
        let mut mute = BackgroundMute::default();

        let actions = mute.plan(&apps, CHROME, &[session("s1", "game.exe", false)]);
        assert_eq!(actions.mute, keys(&["s1"]));

        let actions = mute.plan(&apps, GAME, &[session("s1", "game.exe", true)]);
        assert_eq!(actions.unmute, keys(&["s1"]));
        assert!(mute.owners().is_empty());
    }

    #[test]
    fn background_mute_leaves_a_user_mute_alone() {
        let apps = [managed("game.exe", true)];
        let mut mute = BackgroundMute::default();

        let muted = [session("s1", "game.exe", true)];
        assert_eq!(mute.plan(&apps, CHROME, &muted), MuteActions::default());
        assert_eq!(mute.plan(&apps, GAME, &muted), MuteActions::default());
    }

    #[test]
    fn background_mute_respects_a_manual_unmute_until_the_next_switch() {
        let apps = [managed("game.exe", true)];
        let mut mute = BackgroundMute::default();
        let unmuted = [session("s1", "game.exe", false)];
        mute.plan(&apps, CHROME, &unmuted);

        // The user unmutes the game in the volume mixer while it is in the background.
        assert_eq!(mute.plan(&apps, CHROME, &unmuted), MuteActions::default());
        assert_eq!(mute.plan(&apps, GAME, &unmuted), MuteActions::default());

        let actions = mute.plan(&apps, CHROME, &unmuted);
        assert_eq!(actions.mute, keys(&["s1"]));
    }

    #[test]
    fn background_mute_restores_a_program_closed_while_muted() {
        let apps = [managed("game.exe", true)];
        let mut mute = BackgroundMute::default();
        mute.plan(&apps, CHROME, &[session("s1", "game.exe", false)]);

        mute.plan(&apps, CHROME, &[]);
        assert_eq!(mute.owners(), BTreeSet::from(["game".to_string()]));

        // Windows hands the saved mute to the program's next session.
        let actions = mute.plan(&apps, GAME, &[session("s2", "game.exe", true)]);
        assert_eq!(actions.unmute, keys(&["s2"]));
        assert!(mute.owners().is_empty());
    }

    #[test]
    fn background_mute_recovers_after_a_crash() {
        let apps = [managed("game.exe", true)];
        let muted = [session("s1", "game.exe", true)];

        let mut restored = BackgroundMute::with_pending(BTreeSet::from(["game".to_string()]));
        assert_eq!(restored.plan(&apps, GAME, &muted).unmute, keys(&["s1"]));

        let mut adopted = BackgroundMute::with_pending(BTreeSet::from(["game".to_string()]));
        assert_eq!(adopted.plan(&apps, CHROME, &muted), MuteActions::default());
        assert_eq!(adopted.plan(&apps, GAME, &muted).unmute, keys(&["s1"]));
    }

    #[test]
    fn background_mute_restores_sound_when_turned_off() {
        let mut mute = BackgroundMute::default();
        mute.plan(
            &[managed("game.exe", true)],
            CHROME,
            &[session("s1", "game.exe", false)],
        );

        let actions = mute.plan(
            &[managed("game.exe", false)],
            CHROME,
            &[session("s1", "game.exe", true)],
        );
        assert_eq!(actions.unmute, keys(&["s1"]));
    }

    #[test]
    fn releasing_background_mute_keeps_closed_programs_for_later() {
        let apps = [managed("game.exe", true), managed("music.exe", true)];
        let mut mute = BackgroundMute::default();
        mute.plan(
            &apps,
            CHROME,
            &[
                session("s1", "game.exe", false),
                session("s2", "music.exe", false),
            ],
        );

        let actions = mute.release_all(&[session("s1", "game.exe", true)]);
        assert_eq!(actions.unmute, keys(&["s1"]));
        assert_eq!(mute.owners(), BTreeSet::from(["music".to_string()]));
    }

    #[test]
    fn muted_owners_file_is_removed_when_empty() {
        let path =
            std::env::temp_dir().join(format!("window-warden-muted-{}.json", std::process::id()));
        let owners = BTreeSet::from(["game".to_string()]);
        save_muted_owners(&path, &owners).unwrap();
        assert_eq!(load_muted_owners(&path).unwrap(), owners);

        save_muted_owners(&path, &BTreeSet::new()).unwrap();
        assert!(!path.exists());
        save_muted_owners(&path, &BTreeSet::new()).unwrap();
    }

    #[test]
    fn config_migrates_once_from_the_legacy_folder() {
        let root =
            std::env::temp_dir().join(format!("window-warden-migrate-{}", std::process::id()));
        let legacy = root.join("LockMeWindow");
        let current = root.join("WindowWarden");
        std::fs::create_dir_all(&legacy).unwrap();
        Settings {
            apps: Vec::new(),
            start_with_windows: true,
            language: Language::English,
        }
        .save_to(&legacy.join(SETTINGS_FILE_NAME))
        .unwrap();

        assert!(migrate_config(&legacy, &current).unwrap());
        let moved = Settings::load_from(&current.join(SETTINGS_FILE_NAME)).unwrap();
        assert!(moved.start_with_windows);
        assert_eq!(moved.language, Language::English);

        // A second run leaves the current settings alone.
        assert!(!migrate_config(&legacy, &current).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_location_uses_default_directory_until_customized() {
        let default_dir = Path::new(r"C:\Users\me\AppData\Roaming\WindowWarden");
        let mut location = ConfigLocation::default();
        assert_eq!(
            location.settings_path(default_dir),
            default_dir.join(SETTINGS_FILE_NAME)
        );

        location.directory = Some(PathBuf::from(r"D:\Sync\WindowWarden"));
        assert_eq!(
            location.settings_path(default_dir),
            Path::new(r"D:\Sync\WindowWarden\settings.json")
        );
    }

    #[test]
    fn config_location_round_trips_and_defaults_when_missing() {
        let dir =
            std::env::temp_dir().join(format!("window-warden-location-{}", std::process::id()));
        let path = dir.join(LOCATION_FILE_NAME);
        assert_eq!(
            ConfigLocation::load_from(&path).unwrap(),
            ConfigLocation::default()
        );

        let location = ConfigLocation {
            directory: Some(PathBuf::from(r"D:\Sync\WindowWarden")),
        };
        location.save_to(&path).unwrap();
        assert_eq!(ConfigLocation::load_from(&path).unwrap(), location);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_command_quotes_executable_and_starts_minimized() {
        assert_eq!(
            startup_command(Path::new(
                r"C:\Program Files\WindowWarden\window-warden.exe"
            )),
            r#""C:\Program Files\WindowWarden\window-warden.exe" --minimized"#
        );
    }

    #[test]
    fn user_registry_string_is_set_and_deleted() {
        let subkey = format!(r"Software\WindowWarden.Test.{}", std::process::id());
        let _cleanup = DeleteTestKey(subkey.clone());
        let command = r#""C:\Tools\window-warden.exe" --minimized"#;

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
    fn missing_registry_dword_reads_as_none() {
        let subkey = format!(r"Software\WindowWarden.Test.{}", std::process::id());
        assert_eq!(user_registry_dword(&subkey, "Missing"), None);
    }

    #[test]
    fn named_event_wakes_another_handle_once() {
        let name = format!(r"Local\WindowWarden.TestEvent.{}", std::process::id());
        let waiter = NamedEvent::open_or_create(&name).unwrap();
        let sender = NamedEvent::open_or_create(&name).unwrap();
        assert!(!waiter.wait(0));

        sender.signal().unwrap();
        assert!(waiter.wait(1000));
        assert!(!waiter.wait(0));
    }

    #[test]
    fn named_instance_is_exclusive_until_released() {
        let name = format!(r"Local\WindowWarden.Test.{}", std::process::id());
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
