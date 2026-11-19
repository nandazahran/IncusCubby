use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
use std::time::Duration;

use anyhow::Context as _;
use serde::Deserialize;

const COMMUNITY_ORG_URL: &str = "https://community-scripts.org/";
const GITHUB_INCUS_TREE_URL: &str =
    "https://api.github.com/repos/community-scripts/Incus/git/trees/main?recursive=1";
const GITHUB_DEVSCRIPTS_TREE_URL: &str =
    "https://api.github.com/repos/community-scripts/DevScripts/git/trees/main?recursive=1";
const COMMUNITY_INCUS_RAW_URL: &str =
    "https://raw.githubusercontent.com/community-scripts/Incus/main";
const COMMUNITY_DEVSCRIPTS_RAW_URL: &str =
    "https://raw.githubusercontent.com/community-scripts/DevScripts/main";
const COMMUNITY_PROXMOX_RAW_URL: &str =
    "https://raw.githubusercontent.com/community-scripts/ProxmoxVE/main";
const COMMUNITY_SCRIPTS_CORE_URL: &str =
    "https://raw.githubusercontent.com/community-scripts/core/main";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommunityRepo {
    #[default]
    Incus,
    DevScripts,
    ProxmoxVe,
}

impl CommunityRepo {
    #[must_use]
    pub const fn raw_base_url(self) -> &'static str {
        match self {
            Self::Incus => COMMUNITY_INCUS_RAW_URL,
            Self::DevScripts => COMMUNITY_DEVSCRIPTS_RAW_URL,
            Self::ProxmoxVe => COMMUNITY_PROXMOX_RAW_URL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityApp {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub logo_url: String,
    pub repo: CommunityRepo,
    pub is_dev: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityAppSpec {
    pub slug: String,
    pub app_name: String,
    pub description: String,
    pub tags: String,
    pub image_alias: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub nesting: bool,
    pub repo: CommunityRepo,
    pub ct_script: String,
    pub install_script: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCtMetadata {
    pub app_name: String,
    pub tags: String,
    pub os: String,
    pub version: String,
    pub image_alias: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub nesting: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubTreeResponse {
    #[serde(default)]
    tree: Vec<GitHubTreeEntry>,
}

#[derive(Debug, Deserialize)]
struct GitHubTreeEntry {
    #[serde(default)]
    path: String,
    #[serde(rename = "type", default)]
    entry_type: String,
}

#[derive(Debug, Clone, Copy)]
struct IfBranch {
    is_alpine: bool,
    in_else: bool,
}

#[must_use]
pub fn curated_community_apps() -> Vec<CommunityApp> {
    [
        ("caddy", "Caddy"),
        ("code-server", "Code Server"),
        ("docker", "Docker"),
        ("forgejo", "Forgejo"),
        ("gitea", "Gitea"),
        ("homepage", "Homepage"),
        ("nginx", "Nginx"),
        ("ollama", "Ollama"),
        ("postgresql", "PostgreSQL"),
        ("redis", "Redis"),
        ("sqlite", "SQLite"),
        ("traefik", "Traefik"),
    ]
    .into_iter()
    .map(|(slug, name)| CommunityApp {
        slug: slug.to_string(),
        name: name.to_string(),
        description: String::new(),
        logo_url: String::new(),
        repo: CommunityRepo::Incus,
        is_dev: false,
    })
    .collect()
}

fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn validate_slug(slug: &str) -> anyhow::Result<&str> {
    if is_valid_slug(slug) {
        Ok(slug)
    } else {
        anyhow::bail!("invalid community app slug: {slug:?}")
    }
}

fn slug_to_display_name(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                let mut word = first.to_uppercase().to_string();
                word.push_str(chars.as_str());
                word
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_github_tree_json(
    json_str: &str,
    repo: CommunityRepo,
) -> Result<Vec<CommunityApp>, serde_json::Error> {
    let response: GitHubTreeResponse = serde_json::from_str(json_str)?;
    let mut ct_slugs: BTreeSet<String> = BTreeSet::new();
    let mut install_slugs: HashSet<String> = HashSet::new();

    for entry in response.tree {
        if entry.entry_type != "blob" {
            continue;
        }

        if let Some(slug) = entry
            .path
            .strip_prefix("ct/")
            .and_then(|rest| rest.strip_suffix(".sh"))
        {
            if is_valid_slug(slug) {
                ct_slugs.insert(slug.to_string());
            }
        } else if let Some(slug) = entry
            .path
            .strip_prefix("install/")
            .and_then(|rest| rest.strip_suffix("-install.sh"))
            && is_valid_slug(slug)
        {
            install_slugs.insert(slug.to_string());
        }
    }

    Ok(ct_slugs
        .into_iter()
        .filter(|slug| install_slugs.contains(slug))
        .map(|slug| CommunityApp {
            name: slug_to_display_name(&slug),
            slug,
            description: String::new(),
            logo_url: String::new(),
            repo,
            is_dev: repo == CommunityRepo::DevScripts,
        })
        .collect())
}

fn sanitize_logo_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        String::new()
    }
}

#[derive(Debug, Deserialize)]
struct SiteScriptEntry {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    logo: String,
    #[serde(default)]
    is_dev: bool,
    #[serde(default)]
    is_deleted: bool,
    #[serde(default)]
    is_disabled: bool,
    #[serde(default)]
    install_command: String,
}

#[must_use]
pub fn parse_community_org_html(html: &str) -> Option<Vec<CommunityApp>> {
    const PREFIX: &str = "self.__next_f.push([1,";
    let mut search_from = 0usize;

    while let Some(rel_start) = html[search_from..].find(PREFIX) {
        let lit_start = search_from + rel_start + PREFIX.len();
        let Some(lit_end) = find_json_string_end(&html[lit_start..]) else {
            search_from = lit_start;
            continue;
        };
        let literal = &html[lit_start..lit_start + lit_end];
        search_from = lit_start + lit_end;

        let Ok(payload) = serde_json::from_str::<String>(literal) else {
            continue;
        };
        let Some(arr_pos) = payload.find("\"scripts\":[") else {
            continue;
        };
        let arr_slice = &payload[arr_pos + "\"scripts\":".len()..];
        let Some(arr_end) = find_json_array_end(arr_slice) else {
            continue;
        };
        let Ok(items) = serde_json::from_str::<Vec<SiteScriptEntry>>(&arr_slice[..arr_end]) else {
            continue;
        };

        let mut by_slug: BTreeMap<String, CommunityApp> = BTreeMap::new();
        for item in items {
            if item.is_deleted
                || item.is_disabled
                || !item.install_command.contains("/ct/")
                || !is_valid_slug(&item.slug)
            {
                continue;
            }
            let repo = if item
                .install_command
                .contains("/community-scripts/DevScripts/")
                || item.is_dev
            {
                CommunityRepo::DevScripts
            } else {
                CommunityRepo::Incus
            };
            let display_name = if item.name.trim().is_empty() {
                slug_to_display_name(&item.slug)
            } else {
                item.name.trim().to_string()
            };
            by_slug.insert(
                item.slug.clone(),
                CommunityApp {
                    slug: item.slug,
                    name: display_name,
                    description: item.description.trim().to_string(),
                    logo_url: sanitize_logo_url(&item.logo),
                    repo,
                    is_dev: item.is_dev || repo == CommunityRepo::DevScripts,
                },
            );
        }
        if !by_slug.is_empty() {
            return Some(by_slug.into_values().collect());
        }
    }
    None
}

fn find_json_string_end(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.first().copied() != Some(b'"') {
        return None;
    }
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if b == b'\\' {
            escaped = true;
        } else if b == b'"' {
            return Some(idx + 1);
        }
    }
    None
}

fn find_json_array_end(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.first().copied() != Some(b'[') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate() {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
        } else if b == b'"' {
            in_str = true;
        } else if b == b'[' {
            depth += 1;
        } else if b == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(idx + 1);
            }
        }
    }
    None
}

fn is_alpine_condition(line: &str) -> bool {
    line.contains("var_os") && line.contains("alpine") && !line.contains("!=")
}

fn strip_shell_quotes_and_comment(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        return rest.split_once('"').map_or(trimmed, |(inner, _)| inner);
    }
    if let Some(rest) = trimmed.strip_prefix('\'') {
        return rest.split_once('\'').map_or(trimmed, |(inner, _)| inner);
    }
    trimmed
        .split_once(" #")
        .map_or(trimmed, |(before, _)| before.trim_end())
}

fn extract_shell_var(line: &str, name: &str) -> Option<String> {
    let line = line.strip_prefix("export ").map_or(line, str::trim_start);
    let rhs = line.strip_prefix(name)?.strip_prefix('=')?;
    let unquoted = strip_shell_quotes_and_comment(rhs);
    let value = if let Some(inner) = unquoted
        .strip_prefix("${")
        .and_then(|s| s.strip_suffix('}'))
    {
        if let Some((_, default_val)) = inner.split_once(":-") {
            default_val
        } else if let Some((_, default_val)) = inner.split_once('-') {
            default_val
        } else {
            inner
        }
    } else {
        unquoted
    };
    Some(strip_shell_quotes_and_comment(value).to_string())
}

#[must_use]
pub fn parse_ct_metadata(ct_source: &str, slug: &str) -> ParsedCtMetadata {
    let mut app_name: Option<String> = None;
    let mut tags: Option<String> = None;
    let mut os: Option<String> = None;
    let mut version: Option<String> = None;
    let mut cpu: Option<String> = None;
    let mut ram: Option<String> = None;
    let mut nesting = true;
    let mut if_stack: Vec<IfBranch> = Vec::new();

    for line in ct_source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with("if ") || trimmed.starts_with("if\t") {
            let closes_inline = trimmed.ends_with("; fi") || trimmed.ends_with(" fi");
            if !closes_inline {
                if_stack.push(IfBranch {
                    is_alpine: is_alpine_condition(trimmed),
                    in_else: false,
                });
            }
            continue;
        }

        if trimmed.starts_with("elif ") || trimmed.starts_with("elif\t") {
            if let Some(top) = if_stack.last_mut() {
                top.is_alpine = is_alpine_condition(trimmed);
                top.in_else = false;
            }
            continue;
        }

        if trimmed == "else" || trimmed.starts_with("else ") || trimmed.starts_with("else#") {
            if let Some(top) = if_stack.last_mut() {
                top.in_else = true;
            }
            continue;
        }

        if trimmed == "fi"
            || trimmed.starts_with("fi ")
            || trimmed.starts_with("fi;")
            || trimmed.starts_with("fi#")
        {
            let _ = if_stack.pop();
            continue;
        }

        let in_alpine_then = if_stack
            .iter()
            .any(|branch| branch.is_alpine && !branch.in_else);

        if let Some(val) = extract_shell_var(trimmed, "APP")
            && !val.is_empty()
        {
            app_name = Some(val);
        }
        if let Some(val) = extract_shell_var(trimmed, "var_tags") {
            tags = Some(val);
        }
        if let Some(val) = extract_shell_var(trimmed, "var_os")
            && !val.is_empty()
        {
            os = Some(val);
        }
        if !in_alpine_then {
            if let Some(val) = extract_shell_var(trimmed, "var_version")
                && !val.is_empty()
            {
                version = Some(val);
            }
            if let Some(val) = extract_shell_var(trimmed, "var_cpu")
                && !val.is_empty()
            {
                cpu = Some(val);
            }
            if let Some(val) = extract_shell_var(trimmed, "var_ram")
                && !val.is_empty()
            {
                ram = Some(val);
            }
        }
        if let Some(val) = extract_shell_var(trimmed, "var_nesting")
            .or_else(|| extract_shell_var(trimmed, "ENABLE_NESTING"))
            && !val.is_empty()
        {
            nesting = val != "0";
        }
    }

    let app_name = app_name.unwrap_or_else(|| slug_to_display_name(slug));
    let tags = tags.unwrap_or_default();
    let os = os.unwrap_or_else(|| "debian".to_string());
    let version = version.unwrap_or_else(|| {
        match os.as_str() {
            "ubuntu" => "24.04",
            "alpine" => "3.24",
            _ => "13",
        }
        .to_string()
    });
    let image_alias = if os == "archlinux" {
        "archlinux".to_string()
    } else {
        format!("{os}/{version}")
    };
    let cpu_limit = cpu.unwrap_or_else(|| "2".to_string());
    let ram_raw = ram.unwrap_or_else(|| "1024".to_string());
    let memory_limit = if !ram_raw.is_empty() && ram_raw.bytes().all(|b| b.is_ascii_digit()) {
        format!("{ram_raw}MiB")
    } else {
        ram_raw
    };

    ParsedCtMetadata {
        app_name,
        tags,
        os,
        version,
        image_alias,
        cpu_limit,
        memory_limit,
        nesting,
    }
}

fn http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("IncusCubby")
        .timeout(HTTP_TIMEOUT)
        .build()
        .context("failed to construct HTTP client")
}

pub async fn fetch_community_catalog() -> anyhow::Result<Vec<CommunityApp>> {
    let client = http_client()?;
    if let Ok(html) = fetch_text(&client, COMMUNITY_ORG_URL, "community-scripts.org").await
        && let Some(apps) = parse_community_org_html(&html)
    {
        return Ok(apps);
    }

    let (incus_res, dev_res) = tokio::join!(
        fetch_github_tree_apps(&client, GITHUB_INCUS_TREE_URL, CommunityRepo::Incus),
        fetch_github_tree_apps(
            &client,
            GITHUB_DEVSCRIPTS_TREE_URL,
            CommunityRepo::DevScripts
        ),
    );
    let mut merged: BTreeMap<String, CommunityApp> = BTreeMap::new();
    if let Ok(dev_apps) = dev_res {
        for app in dev_apps {
            merged.insert(app.slug.clone(), app);
        }
    }
    if let Ok(incus_apps) = incus_res {
        for app in incus_apps {
            merged.insert(app.slug.clone(), app);
        }
    }
    if merged.is_empty() {
        anyhow::bail!("failed to load catalog from community-scripts.org and GitHub API");
    }
    Ok(merged.into_values().collect())
}

async fn fetch_github_tree_apps(
    client: &reqwest::Client,
    url: &str,
    repo: CommunityRepo,
) -> anyhow::Result<Vec<CommunityApp>> {
    let body = fetch_text(client, url, "GitHub tree").await?;
    parse_github_tree_json(&body, repo).context("failed to parse GitHub tree JSON")
}

async fn fetch_text(client: &reqwest::Client, url: &str, label: &str) -> anyhow::Result<String> {
    client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {label}"))?
        .error_for_status()
        .with_context(|| format!("{label} returned error status"))?
        .text()
        .await
        .with_context(|| format!("failed to read {label} body"))
}

pub async fn fetch_community_app_spec(
    slug: &str,
    preferred_repo: CommunityRepo,
    description: &str,
) -> anyhow::Result<CommunityAppSpec> {
    let slug = validate_slug(slug)?;
    let client = http_client()?;

    let mut repos_to_try = vec![preferred_repo];
    for fallback in [
        CommunityRepo::Incus,
        CommunityRepo::ProxmoxVe,
        CommunityRepo::DevScripts,
    ] {
        if !repos_to_try.contains(&fallback) {
            repos_to_try.push(fallback);
        }
    }

    let mut last_err = None;
    for repo in repos_to_try {
        let base = repo.raw_base_url();
        let ct_url = format!("{base}/ct/{slug}.sh");
        let install_url = format!("{base}/install/{slug}-install.sh");
        match tokio::try_join!(
            fetch_text(&client, &ct_url, "ct script"),
            fetch_text(&client, &install_url, "install script"),
        ) {
            Ok((ct_script, install_script)) => {
                let meta = parse_ct_metadata(&ct_script, slug);
                return Ok(CommunityAppSpec {
                    slug: slug.to_string(),
                    app_name: meta.app_name,
                    description: description.to_string(),
                    tags: meta.tags,
                    image_alias: meta.image_alias,
                    cpu_limit: meta.cpu_limit,
                    memory_limit: meta.memory_limit,
                    nesting: meta.nesting,
                    repo,
                    ct_script,
                    install_script,
                });
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no script source found for '{slug}'")))
}

pub async fn fetch_install_func() -> anyhow::Result<String> {
    let client = http_client()?;
    let url = format!("{COMMUNITY_SCRIPTS_CORE_URL}/lxc/install.func");
    fetch_text(&client, &url, "install.func").await
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[must_use]
pub fn build_container_runner_script(
    spec: &CommunityAppSpec,
    instance_name: &str,
    image_alias: &str,
) -> String {
    let (os_type, os_version) = image_alias
        .split_once('/')
        .filter(|(os, ver)| !os.is_empty() && !ver.is_empty())
        .unwrap_or(("debian", "13"));

    let mut script = String::new();
    let _ = writeln!(script, "#!/usr/bin/env bash");
    let _ = writeln!(script, "export CS_STDIN_OK=0");
    let _ = writeln!(script, "read() {{");
    let _ = writeln!(script, "  if [[ \"${{CS_STDIN_OK:-0}}\" == \"0\" ]]; then");
    let _ = writeln!(script, "    local _has_prompt=0");
    let _ = writeln!(script, "    local _prev_takes_arg=0");
    let _ = writeln!(script, "    local _arg");
    let _ = writeln!(script, "    for _arg in \"$@\"; do");
    let _ = writeln!(script, "      if [[ $_prev_takes_arg -eq 1 ]]; then");
    let _ = writeln!(script, "        _prev_takes_arg=0");
    let _ = writeln!(script, "        continue");
    let _ = writeln!(script, "      fi");
    let _ = writeln!(script, "      case \"$_arg\" in");
    let _ = writeln!(script, "        -p)");
    let _ = writeln!(script, "          _has_prompt=1");
    let _ = writeln!(script, "          _prev_takes_arg=1");
    let _ = writeln!(script, "          ;;");
    let _ = writeln!(script, "        -p*)");
    let _ = writeln!(script, "          _has_prompt=1");
    let _ = writeln!(script, "          ;;");
    let _ = writeln!(script, "        -a|-d|-i|-n|-N|-t|-u)");
    let _ = writeln!(script, "          _prev_takes_arg=1");
    let _ = writeln!(script, "          ;;");
    let _ = writeln!(script, "      esac");
    let _ = writeln!(script, "    done");
    let _ = writeln!(
        script,
        "    if [[ $_has_prompt -eq 1 ]] || [[ -t 0 ]]; then"
    );
    let _ = writeln!(script, "      return 0");
    let _ = writeln!(script, "    fi");
    let _ = writeln!(script, "  fi");
    let _ = writeln!(script, "  builtin read \"$@\"");
    let _ = writeln!(script, "}}");
    let _ = writeln!(script, "export -f read");
    let _ = writeln!(
        script,
        "export FUNCTIONS_FILE_PATH=\"$(cat /tmp/incus-functions)\""
    );
    let _ = writeln!(script, "export DIAGNOSTICS=no");
    let _ = writeln!(script, "export VERBOSE=yes");
    let _ = writeln!(script, "export STD=\"\"");
    let _ = writeln!(script, "export APP={}", shell_quote(&spec.app_name));
    let _ = writeln!(script, "export APPLICATION={}", shell_quote(&spec.app_name));
    let _ = writeln!(script, "export NSAPP={}", shell_quote(&spec.slug));
    let _ = writeln!(script, "export app={}", shell_quote(&spec.slug));
    let _ = writeln!(script, "export CTID={}", shell_quote(instance_name));
    let _ = writeln!(script, "export PCT_OSTYPE={}", shell_quote(os_type));
    let _ = writeln!(script, "export PCT_OSVERSION={}", shell_quote(os_version));
    let _ = writeln!(script, "export TELEMETRY_PLATFORM=incus");
    let _ = writeln!(
        script,
        "export COMMUNITY_SCRIPTS_URL={}",
        shell_quote(spec.repo.raw_base_url())
    );
    let _ = writeln!(
        script,
        "export COMMUNITY_SCRIPTS_CORE_URL={}",
        shell_quote(COMMUNITY_SCRIPTS_CORE_URL)
    );
    let _ = writeln!(script, "export INSTALL_LOG=/root/.install-cubby.log");
    let _ = writeln!(script, "bash /tmp/cubby-install.sh");
    let _ = writeln!(script, "_rc=$?");
    let _ = writeln!(
        script,
        "rm -f /tmp/incus-functions /tmp/cubby-install.sh /tmp/cubby-run.sh"
    );
    let _ = writeln!(script, "exit \"$_rc\"");
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_apps_are_sorted_and_non_empty() {
        let apps = curated_community_apps();
        assert_eq!(apps.len(), 12);
        assert!(apps.windows(2).all(|w| w[0].slug < w[1].slug));
        assert_eq!(apps[0].slug, "caddy");
        assert_eq!(apps[1].slug, "code-server");
        assert_eq!(apps[1].name, "Code Server");
    }

    #[test]
    fn parse_github_tree_requires_both_ct_and_install_and_valid_slug() {
        let json = r#"{
            "tree": [
                { "path": "ct/code-server.sh", "type": "blob" },
                { "path": "install/code-server-install.sh", "type": "blob" },
                { "path": "ct/caddy.sh", "type": "blob" },
                { "path": "install/caddy-install.sh", "type": "blob" },
                { "path": "ct/only-ct.sh", "type": "blob" },
                { "path": "install/only-install-install.sh", "type": "blob" },
                { "path": "ct/nested/bad.sh", "type": "blob" },
                { "path": "install/nested/bad-install.sh", "type": "blob" },
                { "path": "ct/not_a_blob.sh", "type": "tree" },
                { "path": "install/not_a_blob-install.sh", "type": "blob" },
                { "path": "ct/my_app.sh", "type": "blob" },
                { "path": "install/my_app-install.sh", "type": "blob" }
            ]
        }"#;

        let apps =
            parse_github_tree_json(json, CommunityRepo::Incus).expect("valid github tree json");
        assert_eq!(
            apps,
            vec![
                CommunityApp {
                    slug: "caddy".to_string(),
                    name: "Caddy".to_string(),
                    description: String::new(),
                    logo_url: String::new(),
                    repo: CommunityRepo::Incus,
                    is_dev: false,
                },
                CommunityApp {
                    slug: "code-server".to_string(),
                    name: "Code Server".to_string(),
                    description: String::new(),
                    logo_url: String::new(),
                    repo: CommunityRepo::Incus,
                    is_dev: false,
                },
                CommunityApp {
                    slug: "my_app".to_string(),
                    name: "My App".to_string(),
                    description: String::new(),
                    logo_url: String::new(),
                    repo: CommunityRepo::Incus,
                    is_dev: false,
                },
            ]
        );
    }

    #[test]
    fn parse_community_org_html_extracts_stable_and_dev_apps() {
        let inner_json = r#"34:["$","$c",null,{"initData":{"scripts":[{"slug":"surfsense","name":"SurfSense","description":"NotebookLM alternative","logo":"https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/surfsense.webp","is_dev":true,"is_deleted":false,"is_disabled":false,"install_command":"bash -c \"$(curl -fsSL https://raw.githubusercontent.com/community-scripts/DevScripts/main/ct/surfsense.sh)\""},{"slug":"t3code","name":"T3 Code","description":"Agent surface","logo":"https://cdn.jsdelivr.net/gh/example/t3code/icon.svg","is_dev":true,"is_deleted":false,"is_disabled":false,"install_command":"bash -c \"$(curl -fsSL https://raw.githubusercontent.com/community-scripts/DevScripts/main/ct/t3code.sh)\""},{"slug":"docker","name":"Docker","description":"Containers","logo":"http://insecure.example.com/docker.png","is_dev":false,"is_deleted":false,"is_disabled":false,"install_command":"var_os='debian' bash -c \"$(curl -fsSL https://raw.githubusercontent.com/community-scripts/ProxmoxVE/main/ct/docker.sh)\""},{"slug":"old-vm","name":"Old VM","description":"Skip VM","is_dev":false,"is_deleted":false,"is_disabled":false,"install_command":"bash -c \"$(curl -fsSL https://raw.githubusercontent.com/community-scripts/ProxmoxVE/main/vm/old-vm.sh)\""},{"slug":"gone","name":"Gone","description":"Deleted","is_dev":false,"is_deleted":true,"is_disabled":false,"install_command":"bash -c \"$(curl -fsSL https://raw.githubusercontent.com/community-scripts/ProxmoxVE/main/ct/gone.sh)\""}]}}]"#;
        let escaped = serde_json::to_string(inner_json).unwrap();
        let html =
            format!("<html><body><script>self.__next_f.push([1,{escaped}])</script></body></html>");

        let apps = parse_community_org_html(&html).expect("should parse scripts array");
        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0].slug, "docker");
        assert_eq!(apps[0].repo, CommunityRepo::Incus);
        assert_eq!(apps[0].logo_url, "");
        assert!(!apps[0].is_dev);
        assert_eq!(apps[1].slug, "surfsense");
        assert_eq!(apps[1].name, "SurfSense");
        assert_eq!(
            apps[1].logo_url,
            "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/surfsense.webp"
        );
        assert_eq!(apps[1].repo, CommunityRepo::DevScripts);
        assert!(apps[1].is_dev);
        assert_eq!(apps[2].slug, "t3code");
        assert_eq!(apps[2].name, "T3 Code");
        assert_eq!(
            apps[2].logo_url,
            "https://cdn.jsdelivr.net/gh/example/t3code/icon.svg"
        );
        assert_eq!(apps[2].repo, CommunityRepo::DevScripts);
    }

    #[tokio::test]
    #[ignore = "requires internet access"]
    async fn live_community_org_catalog_includes_surfsense_and_t3code() {
        let apps = fetch_community_catalog()
            .await
            .expect("fetch_community_catalog");
        assert!(apps.len() > 700, "expected >700 apps, got {}", apps.len());

        let surf = apps
            .iter()
            .find(|a| a.slug == "surfsense")
            .expect("surfsense should be in catalog");
        assert_eq!(surf.repo, CommunityRepo::DevScripts);
        assert!(surf.logo_url.starts_with("https://"));
        let surf_spec = fetch_community_app_spec(&surf.slug, surf.repo, &surf.description)
            .await
            .expect("fetch surfsense spec");
        assert_ne!(surf_spec.install_script, "");

        let t3 = apps
            .iter()
            .find(|a| a.slug == "t3code")
            .expect("t3code should be in catalog");
        assert_eq!(t3.repo, CommunityRepo::DevScripts);
        assert!(t3.logo_url.starts_with("https://"));
        let t3_spec = fetch_community_app_spec(&t3.slug, t3.repo, &t3.description)
            .await
            .expect("fetch t3code spec");
        assert_eq!(t3_spec.app_name, "T3Code");
        assert_ne!(t3_spec.install_script, "");
    }

    #[test]
    fn parse_ct_metadata_respects_alpine_if_else_branches() {
        let ct = r#"#!/usr/bin/env bash
APP="Forgejo"
var_tags="${var_tags:-git;vcs}"
var_os="${var_os:-debian}"
if [[ "${var_os:-}" == "alpine" ]]; then
  var_cpu="${var_cpu:-1}"
  var_ram="${var_ram:-512}"
  var_version="${var_version:-3.24}"
else
  var_cpu="${var_cpu:-2}"
  var_ram="${var_ram:-2048}"
  var_version="${var_version:-13}"
fi
var_nesting="${var_nesting:-0}"
"#;

        let meta = parse_ct_metadata(ct, "forgejo");
        assert_eq!(
            meta,
            ParsedCtMetadata {
                app_name: "Forgejo".to_string(),
                tags: "git;vcs".to_string(),
                os: "debian".to_string(),
                version: "13".to_string(),
                image_alias: "debian/13".to_string(),
                cpu_limit: "2".to_string(),
                memory_limit: "2048MiB".to_string(),
                nesting: false,
            }
        );
    }

    #[test]
    fn parse_ct_metadata_handles_alpine_then_without_else_and_defaults() {
        let ct = r#"
APP="Caddy"
var_os="${var_os:-ubuntu}"
var_cpu="${var_cpu:-4}"
var_ram="${var_ram:-4096}"
if [[ "${var_os:-}" == "alpine" ]]; then
  var_cpu="${var_cpu:-1}"
  var_ram="${var_ram:-256}"
  var_version="${var_version:-3.24}"
fi
"#;

        let meta = parse_ct_metadata(ct, "caddy");
        assert_eq!(meta.app_name, "Caddy");
        assert_eq!(meta.os, "ubuntu");
        assert_eq!(meta.version, "24.04");
        assert_eq!(meta.image_alias, "ubuntu/24.04");
        assert_eq!(meta.cpu_limit, "4");
        assert_eq!(meta.memory_limit, "4096MiB");
        assert!(meta.nesting);
    }

    #[test]
    fn parse_ct_metadata_handles_archlinux_and_non_numeric_ram() {
        let ct = r#"
var_os="${var_os:-archlinux}"
var_ram="${var_ram:-2GiB}"
ENABLE_NESTING="0"
"#;

        let meta = parse_ct_metadata(ct, "arch-dev");
        assert_eq!(meta.app_name, "Arch Dev");
        assert_eq!(meta.os, "archlinux");
        assert_eq!(meta.image_alias, "archlinux");
        assert_eq!(meta.cpu_limit, "2");
        assert_eq!(meta.memory_limit, "2GiB");
        assert!(!meta.nesting);
    }

    #[test]
    fn validate_slug_checks_allowed_characters() {
        assert_eq!(validate_slug("code-server").unwrap(), "code-server");
        assert_eq!(validate_slug("my_app_2").unwrap(), "my_app_2");
        assert!(validate_slug("").is_err());
        assert!(validate_slug("../etc/passwd").is_err());
        assert!(validate_slug("foo/bar").is_err());
        assert!(validate_slug("foo bar").is_err());
        assert!(validate_slug("foo;rm").is_err());
    }

    #[test]
    fn build_container_runner_script_quotes_single_quotes_and_includes_shim() {
        let spec = CommunityAppSpec {
            slug: "code-server".to_string(),
            app_name: "Bob's Code".to_string(),
            description: String::new(),
            tags: "ide".to_string(),
            image_alias: "ubuntu/24.04".to_string(),
            cpu_limit: "2".to_string(),
            memory_limit: "2048MiB".to_string(),
            nesting: true,
            repo: CommunityRepo::Incus,
            ct_script: String::new(),
            install_script: String::new(),
        };

        let script = build_container_runner_script(&spec, "dev'box", "ubuntu/24.04");
        assert!(script.contains("export CS_STDIN_OK=0"));
        assert!(script.contains("read() {"));
        assert!(script.contains("export -f read"));
        assert!(script.contains("export FUNCTIONS_FILE_PATH=\"$(cat /tmp/incus-functions)\""));
        assert!(script.contains("export APP='Bob'\\''s Code'"));
        assert!(script.contains("export APPLICATION='Bob'\\''s Code'"));
        assert!(script.contains("export NSAPP='code-server'"));
        assert!(script.contains("export app='code-server'"));
        assert!(script.contains("export CTID='dev'\\''box'"));
        assert!(script.contains("export PCT_OSTYPE='ubuntu'"));
        assert!(script.contains("export PCT_OSVERSION='24.04'"));
        assert!(script.contains("export TELEMETRY_PLATFORM=incus"));
        assert!(script.contains("export INSTALL_LOG=/root/.install-cubby.log"));
        assert!(script.contains("bash /tmp/cubby-install.sh"));
        assert!(
            script.contains("rm -f /tmp/incus-functions /tmp/cubby-install.sh /tmp/cubby-run.sh")
        );
        assert!(script.contains("exit \"$_rc\""));
    }

    #[test]
    fn build_container_runner_script_falls_back_when_alias_has_no_slash() {
        let spec = CommunityAppSpec {
            slug: "caddy".to_string(),
            app_name: "Caddy".to_string(),
            description: String::new(),
            tags: String::new(),
            image_alias: "archlinux".to_string(),
            cpu_limit: "2".to_string(),
            memory_limit: "1024MiB".to_string(),
            nesting: true,
            repo: CommunityRepo::DevScripts,
            ct_script: String::new(),
            install_script: String::new(),
        };

        let script = build_container_runner_script(&spec, "caddy-1", "archlinux");
        assert!(script.contains("export PCT_OSTYPE='debian'"));
        assert!(script.contains("export PCT_OSVERSION='13'"));
        assert!(script.contains("DevScripts/main"));
    }

    #[tokio::test]
    #[ignore = "requires internet access and local running incusd socket"]
    async fn live_community_script_smoke() {
        use super::super::client::{CreateInstanceParams, IncusClient, NestingMode};

        let catalog = fetch_community_catalog()
            .await
            .expect("fetch_community_catalog");
        assert!(catalog.iter().any(|a| a.slug == "redis"));
        assert!(catalog.iter().any(|a| a.slug == "surfsense"));
        assert!(catalog.iter().any(|a| a.slug == "t3code"));

        let spec = fetch_community_app_spec("redis", CommunityRepo::Incus, "")
            .await
            .expect("fetch_community_app_spec");
        assert_eq!(spec.app_name, "Redis");

        let install_func = fetch_install_func().await.expect("fetch_install_func");
        let client = IncusClient::default_socket();
        let name = format!("cubby-cs-{}", std::process::id());
        let _ = client.set_instance_state(&name, "stop").await;
        let _ = client.delete_instance(&name).await;

        client
            .create_instance(CreateInstanceParams {
                name: &name,
                instance_type: "container",
                image_alias: "alpine/3.24",
                memory_limit: "512MiB",
                cpu_limit: "1",
                workspace_path: "",
                profiles: Vec::new(),
                cloud_init_user_data: "",
                nesting: NestingMode::Enabled,
            })
            .await
            .expect("create alpine container");

        let run_res: anyhow::Result<()> = async {
            client.set_instance_state(&name, "start").await?;
            tokio::time::sleep(Duration::from_secs(3)).await;

            let bootstrap = vec![
                "sh".to_string(),
                "-c".to_string(),
                "apk update && apk add --no-cache bash curl".to_string(),
            ];
            let boot_out = client.exec_command(&name, &bootstrap).await?;
            anyhow::ensure!(
                boot_out.exit_code == 0,
                "bootstrap exit {}: {}",
                boot_out.exit_code,
                boot_out.stderr
            );

            let runner = build_container_runner_script(&spec, &name, "alpine/3.24");
            client
                .push_instance_file(&name, "/tmp/incus-functions", install_func.into_bytes())
                .await?;
            client
                .push_instance_file(
                    &name,
                    "/tmp/cubby-install.sh",
                    spec.install_script.as_bytes().to_vec(),
                )
                .await?;
            client
                .push_instance_file(&name, "/tmp/cubby-run.sh", runner.into_bytes())
                .await?;

            let out = client
                .exec_command(
                    &name,
                    &["bash".to_string(), "/tmp/cubby-run.sh".to_string()],
                )
                .await?;
            anyhow::ensure!(
                out.exit_code == 0,
                "community script failed:\nstdout:\n{}\nstderr:\n{}",
                out.stdout,
                out.stderr
            );
            Ok(())
        }
        .await;

        let _ = client.set_instance_state(&name, "stop").await;
        client
            .delete_instance(&name)
            .await
            .expect("cleanup instance");
        run_res.expect("live_community_script_smoke");
    }
}
