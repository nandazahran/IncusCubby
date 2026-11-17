use std::env;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ide {
    Codium,
    Zed,
}

pub fn open_shell(instance_name: &str, preferred_terminal: &str) -> anyhow::Result<()> {
    if instance_name.is_empty()
        || !instance_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Invalid instance name: {instance_name}");
    }

    let term = detect_terminal(preferred_terminal)
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

pub fn open_ide(
    ide: Ide,
    preferred_ide: &str,
    workspace_path: Option<&str>,
) -> anyhow::Result<()> {
    let bin = resolve_ide_binary(ide, preferred_ide)?;
    let mut cmd = Command::new(&bin);
    if let Some(path) = workspace_path.map(str::trim).filter(|p| !p.is_empty()) {
        cmd.arg(path);
    }
    cmd.spawn().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            anyhow::anyhow!("IDE executable '{}' not found in PATH", bin.display())
        } else {
            anyhow::anyhow!("Failed to launch '{}': {err}", bin.display())
        }
    })?;
    Ok(())
}

fn resolve_ide_binary(ide: Ide, preferred_ide: &str) -> anyhow::Result<PathBuf> {
    let pref = preferred_ide.trim();
    if !pref.is_empty() && pref != "auto" {
        return find_in_path(pref)
            .with_context(|| format!("Preferred IDE '{pref}' not found in PATH"));
    }

    let candidates: &[&str] = match ide {
        Ide::Codium => &["codium", "code"],
        Ide::Zed => &["zed", "zeditor"],
    };
    candidates
        .iter()
        .find_map(|name| find_in_path(name))
        .with_context(|| format!("IDE executable '{}' not found in PATH", candidates[0]))
}

fn detect_terminal(preferred_terminal: &str) -> Option<PathBuf> {
    let pref = preferred_terminal.trim();
    if !pref.is_empty()
        && pref != "auto"
        && let Some(path) = find_in_path(pref)
    {
        return Some(path);
    }

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
