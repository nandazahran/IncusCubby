use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub preferred_terminal: String,
    pub preferred_ide: String,
}

impl AppSettings {
    pub fn config_path() -> Option<PathBuf> {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(xdg).join("incus-cubby").join("settings.json"));
        }
        let home = env::var_os("HOME").filter(|v| !v.is_empty())?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("incus-cubby")
                .join("settings.json"),
        )
    }

    pub fn load() -> Self {
        Self::config_path()
            .map(|path| Self::load_from(&path))
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()
            .context("Cannot resolve config directory (XDG_CONFIG_HOME and HOME are unset)")?;
        self.save_to(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}

fn exists_in_path(bin: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| dir.join(bin).is_file())
}

pub fn available_terminals() -> Vec<String> {
    const TERMINALS: &[&str] = &[
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "alacritty",
        "kitty",
        "foot",
        "wezterm",
        "xfce4-terminal",
        "xterm",
    ];
    let mut result = vec!["auto".to_string()];
    for &name in TERMINALS {
        if exists_in_path(name) {
            result.push(name.to_string());
        }
    }
    result
}

pub const IDE_CANDIDATES: &[&str] = &[
    "codium", "code", "zed", "zeditor", "kate", "subl", "hx", "nvim", "emacs",
];

pub fn available_ides() -> Vec<String> {
    let mut result = vec!["auto".to_string()];
    for &name in IDE_CANDIDATES {
        if exists_in_path(name) {
            result.push(name.to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_settings_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir()
            .join(format!("incus-cubby-test-{tag}-{nanos}"))
            .join("settings.json")
    }

    #[test]
    fn save_to_and_load_from_round_trip() {
        let path = temp_settings_path("roundtrip");
        let settings = AppSettings {
            preferred_terminal: "alacritty".to_string(),
            preferred_ide: "zeditor".to_string(),
        };

        settings.save_to(&path).expect("save_to should succeed");
        let loaded = AppSettings::load_from(&path);
        assert_eq!(loaded, settings);

        if let Some(dir) = path.parent() {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn load_from_missing_or_invalid_returns_default() {
        let missing = temp_settings_path("missing");
        assert_eq!(AppSettings::load_from(&missing), AppSettings::default());

        let invalid = temp_settings_path("invalid");
        if let Some(parent) = invalid.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&invalid, "{not json").unwrap();
        assert_eq!(AppSettings::load_from(&invalid), AppSettings::default());

        if let Some(dir) = invalid.parent() {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn available_lists_always_start_with_auto() {
        assert_eq!(
            available_terminals().first().map(String::as_str),
            Some("auto")
        );
        assert_eq!(available_ides().first().map(String::as_str), Some("auto"));
    }
}
