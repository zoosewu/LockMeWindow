use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;

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
        };

        settings.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), settings);
        std::fs::remove_file(path).unwrap();
    }
}
