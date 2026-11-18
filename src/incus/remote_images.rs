use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde::Deserialize;

const SIMPLESTREAMS_URL: &str = "https://images.linuxcontainers.org/streams/v1/images.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteImage {
    pub alias: String,
    pub os: String,
    pub release: String,
    pub variant: String,
}

#[derive(Debug, Deserialize)]
struct SimplestreamsIndex {
    #[serde(default)]
    products: HashMap<String, SimplestreamsProduct>,
}

#[derive(Debug, Deserialize)]
struct SimplestreamsProduct {
    #[serde(default)]
    aliases: Option<String>,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    release: Option<String>,
    #[serde(default)]
    variant: Option<String>,
}

#[cfg(test)]
fn default_preset_aliases() -> Vec<String> {
    vec![
        "debian/12".to_string(),
        "debian/13".to_string(),
        "ubuntu/24.04".to_string(),
        "ubuntu/26.04".to_string(),
        "alpine/3.24".to_string(),
        "archlinux".to_string(),
        "fedora/44".to_string(),
    ]
}

#[must_use]
pub fn default_preset_images() -> Vec<RemoteImage> {
    vec![
        RemoteImage {
            alias: "debian/12".to_string(),
            os: "Debian".to_string(),
            release: "bookworm · LTS default".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "debian/13".to_string(),
            os: "Debian".to_string(),
            release: "trixie · testing".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "ubuntu/24.04".to_string(),
            os: "Ubuntu".to_string(),
            release: "noble · LTS".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "ubuntu/26.04".to_string(),
            os: "Ubuntu".to_string(),
            release: "resolute".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "alpine/3.24".to_string(),
            os: "Alpine".to_string(),
            release: "3.24 · minimal musl".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "archlinux".to_string(),
            os: "Archlinux".to_string(),
            release: "rolling · pacman".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "fedora/44".to_string(),
            os: "Fedora".to_string(),
            release: "44 · dnf5".to_string(),
            variant: "default".to_string(),
        },
        RemoteImage {
            alias: "opensuse/tumbleweed".to_string(),
            os: "openSUSE".to_string(),
            release: "rolling · zypper".to_string(),
            variant: "default".to_string(),
        },
    ]
}

fn map_host_arch(host_arch: &str) -> &str {
    match host_arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "riscv64" => "riscv64",
        "arm" => "armhf",
        other => other,
    }
}

fn pick_clean_alias(aliases_raw: &str) -> Option<String> {
    let candidates: Vec<&str> = aliases_raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if candidates.is_empty() {
        return None;
    }

    let non_tinycloud: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|alias| !alias.to_ascii_lowercase().contains("tinycloud"))
        .collect();

    let pool = if non_tinycloud.is_empty() {
        &candidates
    } else {
        &non_tinycloud
    };

    pool.iter()
        .min_by_key(|alias| (alias.len(), **alias))
        .map(|alias| (*alias).to_string())
}

pub fn parse_simplestreams_json(
    json_str: &str,
    host_arch: &str,
) -> Result<Vec<RemoteImage>, serde_json::Error> {
    let target_arch = map_host_arch(host_arch);
    let index: SimplestreamsIndex = serde_json::from_str(json_str)?;

    let mut by_alias: BTreeMap<String, RemoteImage> = BTreeMap::new();

    for product in index.products.into_values() {
        let Some(arch) = product.arch.as_deref() else {
            continue;
        };
        if arch != target_arch {
            continue;
        }

        let Some(aliases_raw) = product.aliases.as_deref() else {
            continue;
        };
        let Some(alias) = pick_clean_alias(aliases_raw) else {
            continue;
        };

        let image = RemoteImage {
            alias: alias.clone(),
            os: product.os.unwrap_or_default(),
            release: product.release.unwrap_or_default(),
            variant: product.variant.unwrap_or_default(),
        };

        match by_alias.get(&alias) {
            Some(existing) if existing.variant == "default" || image.variant != "default" => {}
            _ => {
                by_alias.insert(alias, image);
            }
        }
    }

    Ok(by_alias.into_values().collect())
}

pub async fn fetch_remote_images() -> anyhow::Result<Vec<RemoteImage>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let response = client
        .get(SIMPLESTREAMS_URL)
        .send()
        .await?
        .error_for_status()?;
    let text = response.text().await?;
    let images = parse_simplestreams_json(&text, std::env::consts::ARCH)?;
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_presets_match_expected_list() {
        assert_eq!(
            default_preset_aliases(),
            vec![
                "debian/12",
                "debian/13",
                "ubuntu/24.04",
                "ubuntu/26.04",
                "alpine/3.24",
                "archlinux",
                "fedora/44",
            ]
        );
    }

    #[test]
    fn parse_simplestreams_filters_arch_picks_clean_alias_and_sorts() {
        let sample = r#"{
            "products": {
                "debian:bookworm:amd64:default": {
                    "aliases": "debian/12/amd64,debian/bookworm,debian/12,debian/12/default",
                    "arch": "amd64",
                    "os": "Debian",
                    "release": "bookworm",
                    "variant": "default"
                },
                "debian:bookworm:arm64:default": {
                    "aliases": "debian/12,debian/bookworm",
                    "arch": "arm64",
                    "os": "Debian",
                    "release": "bookworm",
                    "variant": "default"
                },
                "archlinux:current:amd64:default": {
                    "aliases": "archlinux/current/amd64,archlinux,archlinux/current",
                    "arch": "amd64",
                    "os": "Archlinux",
                    "release": "current",
                    "variant": "default"
                },
                "alpine:3.24:amd64:tinycloud": {
                    "aliases": "tinycloud,alpine/3.24/tinycloud,alpine/3.24",
                    "arch": "amd64",
                    "os": "Alpine",
                    "release": "3.24",
                    "variant": "tinycloud"
                },
                "ubuntu:noble:amd64:tinycloud-only": {
                    "aliases": "ubuntu/24.04/tinycloud",
                    "arch": "amd64",
                    "os": "Ubuntu",
                    "release": "noble",
                    "variant": "tinycloud"
                },
                "missing:aliases:amd64": {
                    "arch": "amd64",
                    "os": "Other",
                    "release": "1",
                    "variant": "default"
                }
            }
        }"#;

        let parsed = parse_simplestreams_json(sample, "x86_64").expect("valid sample json");
        assert_eq!(
            parsed,
            vec![
                RemoteImage {
                    alias: "alpine/3.24".to_string(),
                    os: "Alpine".to_string(),
                    release: "3.24".to_string(),
                    variant: "tinycloud".to_string(),
                },
                RemoteImage {
                    alias: "archlinux".to_string(),
                    os: "Archlinux".to_string(),
                    release: "current".to_string(),
                    variant: "default".to_string(),
                },
                RemoteImage {
                    alias: "debian/12".to_string(),
                    os: "Debian".to_string(),
                    release: "bookworm".to_string(),
                    variant: "default".to_string(),
                },
                RemoteImage {
                    alias: "ubuntu/24.04/tinycloud".to_string(),
                    os: "Ubuntu".to_string(),
                    release: "noble".to_string(),
                    variant: "tinycloud".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_simplestreams_maps_aarch64_and_deduplicates() {
        let sample = r#"{
            "products": {
                "p1": {
                    "aliases": "debian/12",
                    "arch": "arm64",
                    "os": "Debian",
                    "release": "bookworm",
                    "variant": "cloud"
                },
                "p2": {
                    "aliases": "debian/12",
                    "arch": "arm64",
                    "os": "Debian",
                    "release": "bookworm",
                    "variant": "default"
                }
            }
        }"#;

        let parsed = parse_simplestreams_json(sample, "aarch64").expect("valid sample json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].alias, "debian/12");
        assert_eq!(parsed[0].variant, "default");
    }
}
