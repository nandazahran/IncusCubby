use std::env;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ide {
    Code,
    Zed,
}

impl Ide {
    fn binary(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Zed => "zed",
        }
    }
}

pub fn open_shell(instance_name: &str) -> anyhow::Result<()> {
    if instance_name.is_empty()
        || !instance_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Invalid instance name: {instance_name}");
    }

    let term = detect_terminal()
        .context("No supported terminal emulator found in $TERMINAL or PATH")?;

    let mut cmd = Command::new(&term);
    let term_name = term
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    match term_name {
        "gnome-terminal" => {
            cmd.arg("--");
        }
        "wezterm" => {
            cmd.args(["start", "--"]);
        }
        "foot" => {}
        _ => {
            cmd.arg("-e");
        }
    }

    cmd.args(["incus", "exec", instance_name, "--", "su", "-l", "root"]);
    cmd.spawn()
        .with_context(|| format!("Failed to spawn terminal emulator '{}'", term.display()))?;
    Ok(())
}

pub fn open_ide(ide: Ide) -> anyhow::Result<()> {
    let bin = ide.binary();
    Command::new(bin).spawn().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            anyhow::anyhow!("IDE executable '{bin}' not found in PATH")
        } else {
            anyhow::anyhow!("Failed to launch '{bin}': {err}")
        }
    })?;
    Ok(())
}

fn detect_terminal() -> Option<PathBuf> {
    if let Ok(env_term) = env::var("TERMINAL") {
        let trimmed = env_term.trim();
        if !trimmed.is_empty()
            && let Some(path) = find_in_path(trimmed)
        {
            return Some(path);
        }
    }

    const CANDIDATES: &[&str] = &[
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
    CANDIDATES.iter().find_map(|name| find_in_path(name))
}

fn find_in_path(bin: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(bin);
    if candidate.is_absolute() {
        return candidate.is_file().then_some(candidate);
    }
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(bin))
        .find(|p| p.is_file())
}
