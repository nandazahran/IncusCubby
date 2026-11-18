#[must_use]
pub fn package_manager_label(image_os: &str) -> &'static str {
    let os = image_os.trim().to_ascii_lowercase();
    match os.as_str() {
        "debian" | "ubuntu" => "apt",
        "alpine" => "apk",
        "archlinux" | "arch" => "pacman",
        "fedora" | "almalinux" | "rocky" | "rockylinux" | "centos" => "dnf",
        s if s.starts_with("opensuse") => "zypper",
        _ => "",
    }
}

pub fn upgrade_command(image_os: &str) -> Option<Vec<String>> {
    let os = image_os.trim().to_ascii_lowercase();
    let args: Vec<&str> = match os.as_str() {
        "debian" | "ubuntu" => vec![
            "sh",
            "-lc",
            "DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get upgrade -y",
        ],
        "alpine" => vec!["sh", "-lc", "apk update && apk upgrade"],
        "archlinux" | "arch" => vec!["pacman", "-Syu", "--noconfirm"],
        "fedora" | "almalinux" | "rocky" | "rockylinux" | "centos" => vec!["dnf", "upgrade", "-y"],
        s if s == "opensuse"
            || s == "opensuse tumbleweed"
            || s == "opensuse leap"
            || s.starts_with("opensuse") =>
        {
            vec!["zypper", "--non-interactive", "update"]
        }
        _ => return None,
    };

    Some(args.into_iter().map(str::to_string).collect())
}

fn validate_package_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.starts_with('-') {
        return Err(format!("Invalid package name: '{token}'"));
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
    {
        return Err(format!("Unsafe characters in package name: '{token}'"));
    }
    Ok(())
}

pub fn install_command(image_os: &str, packages_raw: &str) -> Result<Vec<String>, String> {
    let pkgs: Vec<String> = packages_raw
        .split_whitespace()
        .map(|tok| {
            validate_package_token(tok)?;
            Ok(tok.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;

    if pkgs.is_empty() {
        return Err("No packages specified".to_string());
    }

    let os = image_os.trim().to_ascii_lowercase();
    let mut cmd: Vec<String> = match os.as_str() {
        "debian" | "ubuntu" => vec![
            "sh".to_string(),
            "-lc".to_string(),
            "DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \"$@\"".to_string(),
            "sh".to_string(),
        ],
        "alpine" => vec![
            "apk".to_string(),
            "add".to_string(),
            "--no-cache".to_string(),
        ],
        "archlinux" | "arch" => vec![
            "pacman".to_string(),
            "-S".to_string(),
            "--noconfirm".to_string(),
        ],
        "fedora" | "almalinux" | "rocky" | "rockylinux" | "centos" => {
            vec!["dnf".to_string(), "install".to_string(), "-y".to_string()]
        }
        s if s.starts_with("opensuse") => vec![
            "zypper".to_string(),
            "--non-interactive".to_string(),
            "install".to_string(),
        ],
        _ => return Err(format!("Unsupported OS for package install: '{image_os}'")),
    };

    cmd.extend(pkgs);
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_commands_cover_supported_distros() {
        assert_eq!(
            upgrade_command("Debian"),
            Some(vec![
                "sh".into(),
                "-lc".into(),
                "DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get upgrade -y".into(),
            ])
        );
        assert_eq!(
            upgrade_command("ubuntu"),
            Some(vec![
                "sh".into(),
                "-lc".into(),
                "DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get upgrade -y".into(),
            ])
        );
        assert_eq!(
            upgrade_command("Alpine"),
            Some(vec![
                "sh".into(),
                "-lc".into(),
                "apk update && apk upgrade".into(),
            ])
        );
        assert_eq!(
            upgrade_command("Archlinux"),
            Some(vec!["pacman".into(), "-Syu".into(), "--noconfirm".into()])
        );
        assert_eq!(
            upgrade_command("Fedora"),
            Some(vec!["dnf".into(), "upgrade".into(), "-y".into()])
        );
        assert_eq!(
            upgrade_command("openSUSE Tumbleweed"),
            Some(vec![
                "zypper".into(),
                "--non-interactive".into(),
                "update".into(),
            ])
        );
        assert_eq!(upgrade_command("gentoo"), None);
    }

    #[test]
    fn install_commands_build_safe_argv() {
        assert_eq!(
            install_command("Ubuntu", "git build-essential g++ python3.12 lib_dev").unwrap(),
            vec![
                "sh",
                "-lc",
                "DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \"$@\"",
                "sh",
                "git",
                "build-essential",
                "g++",
                "python3.12",
                "lib_dev",
            ]
        );
        assert_eq!(
            install_command("alpine", "curl bash").unwrap(),
            vec!["apk", "add", "--no-cache", "curl", "bash"]
        );
        assert_eq!(
            install_command("arch", "neovim ripgrep").unwrap(),
            vec!["pacman", "-S", "--noconfirm", "neovim", "ripgrep"]
        );
        assert_eq!(
            install_command("rockylinux", "htop").unwrap(),
            vec!["dnf", "install", "-y", "htop"]
        );
        assert_eq!(
            install_command("opensuse-leap", "tmux").unwrap(),
            vec!["zypper", "--non-interactive", "install", "tmux"]
        );
    }

    #[test]
    fn install_command_rejects_empty_and_unsafe_tokens() {
        assert!(install_command("debian", "   ").is_err());
        assert!(install_command("debian", "; rm -rf /").is_err());
        assert!(install_command("debian", "git;id").is_err());
        assert!(install_command("alpine", "--force").is_err());
        assert!(install_command("ubuntu", "curl $(whoami)").is_err());
        assert!(install_command("unknown-os", "git").is_err());
    }
}
