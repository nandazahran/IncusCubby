use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub preferred_terminal: String,
    pub preferred_ide: String,
    pub simple_mode: bool,
    pub light_theme: bool,
    pub sidebar_collapsed: bool,
    pub nav_width: i32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            preferred_terminal: String::new(),
            preferred_ide: String::new(),
            simple_mode: false,
            light_theme: false,
            sidebar_collapsed: false,
            nav_width: 208,
        }
    }
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

    pub fn display_config_path() -> String {
        Self::config_path().map_or_else(
            || "~/.config/incus-cubby/settings.json".to_string(),
            |p| p.display().to_string(),
        )
    }

    pub fn load() -> Self {
        Self::config_path().map_or_default(|path| Self::load_from(&path))
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()
            .context("Cannot resolve config directory (XDG_CONFIG_HOME and HOME are unset)")?;
        self.save_to(&path)
    }

    pub fn reset_file() -> anyhow::Result<()> {
        let path = Self::config_path()
            .context("Cannot resolve config directory (XDG_CONFIG_HOME and HOME are unset)")?;
        Self::delete_from(&path)
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

    pub fn delete_from(path: &Path) -> anyhow::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

fn exists_in_path(bin: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| dir.join(bin).is_file())
}

pub const TERMINAL_CANDIDATES: &[&str] = &[
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "ghostty",
    "alacritty",
    "kitty",
    "foot",
    "wezterm",
    "xfce4-terminal",
    "xterm",
];

pub fn available_terminals() -> Vec<String> {
    let mut result = vec!["auto".to_string()];
    for &name in TERMINAL_CANDIDATES {
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

#[must_use]
pub fn display_terminal_label(preferred_terminal: &str) -> String {
    let pref = preferred_terminal.trim();
    let resolved = if !pref.is_empty() && pref != "auto" {
        Some(pref.to_string())
    } else if let Ok(env_term) = env::var("TERMINAL")
        && !env_term.trim().is_empty()
        && exists_in_path(env_term.trim())
    {
        Some(env_term.trim().to_string())
    } else {
        TERMINAL_CANDIDATES
            .iter()
            .find(|&&name| exists_in_path(name))
            .map(|&s| s.to_string())
    };
    match resolved.as_deref() {
        Some("x-terminal-emulator") => "System Terminal".to_string(),
        Some("gnome-terminal") => "GNOME Terminal".to_string(),
        Some("xfce4-terminal") => "Xfce Terminal".to_string(),
        Some("ghostty") => "Ghostty".to_string(),
        Some("alacritty") => "Alacritty".to_string(),
        Some("konsole") => "Konsole".to_string(),
        Some("wezterm") => "WezTerm".to_string(),
        Some("kitty") => "Kitty".to_string(),
        Some("foot") => "Foot".to_string(),
        Some("xterm") => "XTerm".to_string(),
        Some(other) => Path::new(other)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(other)
            .to_string(),
        None => "Terminal".to_string(),
    }
}

#[must_use]
pub fn display_ide_label(preferred_ide: &str) -> String {
    let pref = preferred_ide.trim();
    let resolved = if !pref.is_empty() && pref != "auto" {
        Some(pref)
    } else {
        IDE_CANDIDATES
            .iter()
            .copied()
            .find(|name| exists_in_path(name))
    };
    let Some(bin) = resolved else {
        return "IDE".to_string();
    };
    let base = Path::new(bin)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(bin);
    match base {
        "codium" => "Codium".to_string(),
        "code" => "VS Code".to_string(),
        "zed" | "zeditor" => "Zed".to_string(),
        "kate" => "Kate".to_string(),
        "subl" => "Sublime".to_string(),
        "hx" => "Helix".to_string(),
        "nvim" => "Neovim".to_string(),
        "emacs" => "Emacs".to_string(),
        other => other.to_string(),
    }
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
            simple_mode: true,
            light_theme: true,
            sidebar_collapsed: true,
            nav_width: 208,
        };

        settings.save_to(&path).expect("save_to should succeed");
        let loaded = AppSettings::load_from(&path);
        assert_eq!(loaded, settings);

        AppSettings::delete_from(&path).expect("delete_from should succeed");
        assert_eq!(AppSettings::load_from(&path), AppSettings::default());
        AppSettings::delete_from(&path).expect("deleting missing settings should be a no-op");

        if let Some(dir) = path.parent() {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn load_from_legacy_json_defaults_new_fields() {
        let path = temp_settings_path("legacy");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            &path,
            r#"{"preferred_terminal":"kitty","preferred_ide":"codium"}"#,
        )
        .unwrap();
        let loaded = AppSettings::load_from(&path);
        assert_eq!(loaded.preferred_terminal, "kitty");
        assert_eq!(loaded.preferred_ide, "codium");
        assert!(!loaded.simple_mode);
        assert!(!loaded.light_theme);
        assert_eq!(loaded.nav_width, 208);

        if let Some(dir) = path.parent() {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn display_labels_format_known_tools() {
        assert_eq!(display_ide_label("zeditor"), "Zed");
        assert_eq!(display_ide_label("codium"), "Codium");
        assert_eq!(display_terminal_label("ghostty"), "Ghostty");
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
