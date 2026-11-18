use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

pub(super) fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub error_code: u16,
    #[serde(default)]
    pub error: String,
    pub metadata: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct OperationMetadata {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: u16,
    #[serde(default)]
    pub err: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ApiInstance {
    pub name: String,
    #[serde(rename = "type", default)]
    pub instance_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub profiles: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub config: HashMap<String, String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub devices: HashMap<String, HashMap<String, String>>,
    pub state: Option<InstanceState>,
    #[serde(default)]
    pub snapshots: Option<Vec<ApiSnapshot>>,
}

#[derive(Debug, Deserialize)]
pub struct InstanceState {
    pub cpu: Option<CpuState>,
    pub memory: Option<MemoryState>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub disk: HashMap<String, DiskState>,
    pub network: Option<HashMap<String, NetworkState>>,
    #[serde(default)]
    pub processes: i64,
}

#[derive(Debug, Deserialize)]
pub struct CpuState {
    #[serde(default)]
    pub usage: i64,
}

#[derive(Debug, Deserialize)]
pub struct DiskState {
    #[serde(default)]
    pub usage: i64,
}

#[derive(Debug, Deserialize)]
pub struct MemoryState {
    #[serde(default)]
    pub usage: i64,
}

#[derive(Debug, Deserialize)]
pub struct NetworkState {
    #[serde(default, deserialize_with = "null_to_default")]
    pub addresses: Vec<NetworkAddress>,
    pub counters: Option<NetworkCounters>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkCounters {
    #[serde(default)]
    pub bytes_received: i64,
    #[serde(default)]
    pub bytes_sent: i64,
}

#[derive(Debug, Deserialize)]
pub struct NetworkAddress {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub scope: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiSnapshot {
    pub name: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct CreateInstanceRequest {
    pub name: String,
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub instance_type: String,
    pub architecture: String,
    pub profiles: Vec<String>,
    pub config: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub devices: HashMap<String, HashMap<String, String>>,
    pub source: ImageSource,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InstancePut {
    #[serde(default)]
    pub architecture: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub config: HashMap<String, String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub devices: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default, deserialize_with = "null_to_default")]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub stateful: bool,
}

#[derive(Debug, Serialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub mode: String,
    pub server: String,
    pub protocol: String,
    pub alias: String,
}

#[derive(Debug, Serialize)]
pub struct StateActionRequest {
    pub action: String,
    pub timeout: u32,
    pub force: bool,
    pub stateful: bool,
}

#[derive(Debug, Serialize)]
pub struct CreateSnapshotRequest {
    pub name: String,
    pub stateful: bool,
}

#[derive(Debug, Serialize)]
pub struct RestoreSnapshotRequest {
    pub restore: String,
}

#[derive(Debug, Serialize)]
pub struct CloneInstanceRequest {
    pub name: String,
    pub source: CloneSource,
}

#[derive(Debug, Serialize)]
pub struct CloneSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub source: String,
    pub instance_only: bool,
}

#[derive(Debug, Serialize)]
pub struct ExecRequest {
    pub command: Vec<String>,
    #[serde(rename = "wait-for-websocket")]
    pub wait_for_websocket: bool,
    #[serde(rename = "record-output")]
    pub record_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiImage {
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub aliases: Vec<ApiImageAlias>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub properties: HashMap<String, String>,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub update_source: Option<ApiImageUpdateSource>,
}

#[derive(Debug, Deserialize)]
pub struct ApiImageAlias {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiImageUpdateSource {
    #[serde(default)]
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSummary {
    pub fingerprint: String,
    pub alias: String,
    pub description: String,
    pub architecture: String,
    pub size_bytes: u64,
    pub size_display: String,
}

impl From<ApiImage> for ImageSummary {
    fn from(api: ApiImage) -> Self {
        let alias = api
            .aliases
            .iter()
            .find(|a| !a.name.is_empty())
            .map(|a| a.name.clone())
            .or_else(|| {
                api.update_source
                    .as_ref()
                    .map(|u| u.alias.clone())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_default();

        let description = api
            .properties
            .get("description")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                format_os_release(
                    api.properties.get("os").map(String::as_str),
                    api.properties.get("release").map(String::as_str),
                    "",
                )
            });

        let size_bytes = api.size.max(0) as u64;
        Self {
            fingerprint: api.fingerprint,
            alias,
            description,
            architecture: if api.architecture.is_empty() {
                "-".to_string()
            } else {
                api.architecture
            },
            size_bytes,
            size_display: format_bytes(size_bytes),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiProfile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub config: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSummary {
    pub name: String,
    pub description: String,
    pub cpu: String,
    pub memory: String,
}

impl From<ApiProfile> for ProfileSummary {
    fn from(api: ApiProfile) -> Self {
        let cpu = api.config.get("limits.cpu").cloned().unwrap_or_default();
        let memory = api.config.get("limits.memory").cloned().unwrap_or_default();
        Self {
            name: api.name,
            description: api.description,
            cpu,
            memory,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub name: String,
    pub host_path: String,
    pub container_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEntry {
    pub name: String,
    pub listen: String,
    pub connect: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceSummary {
    pub name: String,
    pub instance_type: String,
    pub status: String,
    pub is_running: bool,
    pub is_frozen: bool,
    pub architecture: String,
    pub image: String,
    pub image_os: String,
    pub app_slug: String,
    pub distro_id: String,
    pub ip: String,
    pub net_iface: String,
    pub memory: String,
    pub memory_usage_display: String,
    pub memory_limit_display: String,
    pub memory_limit_raw: String,
    pub cpu: String,
    pub cpu_limit_raw: String,
    pub root_pool: String,
    pub cpu_usage_display: String,
    pub disk_usage_display: String,
    pub net_rx_display: String,
    pub net_tx_display: String,
    pub processes_display: String,
    pub workspace_path: String,
    pub profiles: Vec<String>,
    pub mounts: Vec<MountEntry>,
    pub proxies: Vec<ProxyEntry>,
    pub snapshots: Vec<SnapshotSummary>,
}

impl From<ApiInstance> for InstanceSummary {
    fn from(api: ApiInstance) -> Self {
        let is_running = api.status.eq_ignore_ascii_case("Running");
        let is_frozen = api.status.eq_ignore_ascii_case("Frozen");
        let instance_type = if api.instance_type.is_empty() {
            "container".to_string()
        } else {
            api.instance_type
        };
        let image_os = api.config.get("image.os").cloned().unwrap_or_default();
        let app_slug = api
            .config
            .get("user.cubby.app")
            .cloned()
            .unwrap_or_default();
        let image = format_os_release(
            api.config.get("image.os").map(String::as_str),
            api.config.get("image.release").map(String::as_str),
            "-",
        );
        let distro_id = detect_distro_id(&app_slug, &image_os, &image).to_string();

        let (net_iface, ip) = api
            .state
            .as_ref()
            .and_then(|s| s.network.as_ref())
            .and_then(extract_primary_ipv4_with_iface)
            .unwrap_or_else(|| (String::new(), "-".to_string()));

        let usage_bytes = api
            .state
            .as_ref()
            .and_then(|s| s.memory.as_ref())
            .map_or(0, |m| m.usage.max(0) as u64);
        let limit_opt = api
            .config
            .get("limits.memory")
            .map(String::as_str)
            .filter(|s| !s.is_empty());
        let memory = format_memory(is_running, usage_bytes, limit_opt);
        let memory_usage_display = if is_running && usage_bytes > 0 {
            format_bytes(usage_bytes)
        } else {
            "-".to_string()
        };
        let memory_limit_raw = limit_opt.unwrap_or_default().to_string();
        let memory_limit_display = limit_opt.map_or_else(
            || "no memory limit".to_string(),
            |lim| format!("limit {lim}"),
        );

        let cpu_limit_raw = api
            .config
            .get("limits.cpu")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default();
        let cpu = if cpu_limit_raw.is_empty() {
            "-".to_string()
        } else {
            cpu_limit_raw.clone()
        };
        let root_pool = api
            .devices
            .get("root")
            .and_then(|d| d.get("pool"))
            .cloned()
            .unwrap_or_default();

        let cpu_usage_display = api
            .state
            .as_ref()
            .and_then(|s| s.cpu.as_ref())
            .map(|c| c.usage)
            .filter(|&u| is_running && u > 0)
            .map_or_else(
                || "-".to_string(),
                |u| format!("{:.2}s", u as f64 / 1_000_000_000.0),
            );

        let disk_usage_display = api
            .state
            .as_ref()
            .and_then(|s| s.disk.get("root"))
            .map(|d| d.usage)
            .filter(|&u| u > 0)
            .map_or_else(|| "-".to_string(), |u| format_bytes(u as u64));

        let (rx_bytes, tx_bytes) = api
            .state
            .as_ref()
            .and_then(|s| s.network.as_ref())
            .map_or((0, 0), sum_network_counters);
        let rx_text = if rx_bytes > 0 {
            format_bytes(rx_bytes)
        } else {
            "-".to_string()
        };
        let tx_text = if tx_bytes > 0 {
            format_bytes(tx_bytes)
        } else {
            "-".to_string()
        };

        let processes_display = api
            .state
            .as_ref()
            .map(|s| s.processes)
            .filter(|&p| is_running && p > 0)
            .map_or_else(|| "-".to_string(), |p| p.to_string());

        let workspace_path = api
            .devices
            .get("workspace")
            .and_then(|d| d.get("source"))
            .cloned()
            .unwrap_or_default();

        let mut mounts = Vec::new();
        let mut proxies = Vec::new();
        for (dev_name, props) in &api.devices {
            match props.get("type").map(String::as_str) {
                Some("disk") => {
                    if let (Some(source), Some(path)) = (props.get("source"), props.get("path")) {
                        mounts.push(MountEntry {
                            name: dev_name.clone(),
                            host_path: source.clone(),
                            container_path: path.clone(),
                        });
                    }
                }
                Some("proxy") => {
                    if let (Some(listen), Some(connect)) =
                        (props.get("listen"), props.get("connect"))
                    {
                        proxies.push(ProxyEntry {
                            name: dev_name.clone(),
                            listen: listen.clone(),
                            connect: connect.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
        mounts.sort_by(|a, b| a.name.cmp(&b.name));
        proxies.sort_by(|a, b| a.name.cmp(&b.name));

        let mut snapshots: Vec<SnapshotSummary> = api
            .snapshots
            .unwrap_or_default()
            .into_iter()
            .map(|s| SnapshotSummary {
                name: s.name,
                created_at: s.created_at,
            })
            .collect();
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Self {
            name: api.name,
            instance_type,
            status: api.status,
            is_running,
            is_frozen,
            architecture: if api.architecture.is_empty() {
                "-".to_string()
            } else {
                api.architecture
            },
            image,
            image_os,
            app_slug,
            distro_id,
            ip,
            net_iface,
            memory,
            memory_usage_display,
            memory_limit_display,
            memory_limit_raw,
            cpu,
            cpu_limit_raw,
            root_pool,
            cpu_usage_display,
            disk_usage_display,
            net_rx_display: rx_text,
            net_tx_display: tx_text,
            processes_display,
            workspace_path,
            profiles: api.profiles,
            mounts,
            proxies,
            snapshots,
        }
    }
}

#[must_use]
pub fn detect_distro_id(app_slug: &str, image_os: &str, alias_or_image: &str) -> &'static str {
    let app = app_slug.trim().to_ascii_lowercase();
    if !app.is_empty() {
        match app.as_str() {
            "docker" => return "docker",
            "ollama" => return "ollama",
            "forgejo" => return "forgejo",
            "postgresql" | "postgres" => return "postgresql",
            "nginx" => return "nginx",
            "redis" => return "redis",
            "caddy" => return "caddy",
            "gitea" => return "gitea",
            _ => {}
        }
    }

    for candidate in [image_os, alias_or_image] {
        let lower = candidate.trim().to_ascii_lowercase();
        if lower.is_empty() {
            continue;
        }
        if lower == "debian" || lower.starts_with("debian") {
            return "debian";
        }
        if lower == "ubuntu" || lower.starts_with("ubuntu") {
            return "ubuntu";
        }
        if lower == "fedora" || lower.starts_with("fedora") {
            return "fedora";
        }
        if lower == "arch" || lower == "archlinux" || lower.starts_with("arch") {
            return "arch";
        }
        if lower == "alpine" || lower.starts_with("alpine") {
            return "alpine";
        }
        if lower.starts_with("opensuse") {
            return "opensuse";
        }
        if lower == "alma" || lower == "almalinux" || lower.starts_with("alma") {
            return "alma";
        }
        if lower == "rocky" || lower == "rockylinux" || lower.starts_with("rocky") {
            return "rocky";
        }
        if lower == "centos" || lower.starts_with("centos") {
            return "centos";
        }
    }

    "generic"
}

fn format_os_release(os: Option<&str>, rel: Option<&str>, fallback: &str) -> String {
    match (os, rel) {
        (Some(os), Some(rel)) if !os.is_empty() && !rel.is_empty() => format!("{os} {rel}"),
        (Some(os), _) if !os.is_empty() => os.to_string(),
        _ => fallback.to_string(),
    }
}

fn extract_primary_ipv4_with_iface(
    network: &HashMap<String, NetworkState>,
) -> Option<(String, String)> {
    let mut keys: Vec<&String> = network.keys().filter(|k| k.as_str() != "lo").collect();
    keys.sort();
    for iface in keys {
        if let Some(state) = network.get(iface) {
            for addr in &state.addresses {
                if addr.family == "inet" && addr.scope == "global" && !addr.address.is_empty() {
                    return Some((iface.clone(), addr.address.clone()));
                }
            }
        }
    }
    None
}

fn sum_network_counters(network: &HashMap<String, NetworkState>) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for (iface, state) in network {
        if iface == "lo" {
            continue;
        }
        if let Some(counters) = &state.counters {
            rx = rx.saturating_add(counters.bytes_received.max(0) as u64);
            tx = tx.saturating_add(counters.bytes_sent.max(0) as u64);
        }
    }
    (rx, tx)
}

fn format_memory(is_running: bool, usage_bytes: u64, limit: Option<&str>) -> String {
    let limit = limit.filter(|s| !s.is_empty());
    match (is_running, usage_bytes, limit) {
        (true, bytes, Some(lim)) if bytes > 0 => format!("{} / {lim}", format_bytes(bytes)),
        (true, bytes, None) if bytes > 0 => format_bytes(bytes),
        (_, _, Some(lim)) => format!("0 / {lim}"),
        _ => "-".to_string(),
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else {
        format!("{:.0} MiB", b / MIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_distro_id_prioritizes_app_slug_then_os_then_alias() {
        assert_eq!(detect_distro_id("docker", "debian", "debian/12"), "docker");
        assert_eq!(detect_distro_id("postgres", "debian", ""), "postgresql");
        assert_eq!(detect_distro_id("", "Ubuntu", "ubuntu/24.04"), "ubuntu");
        assert_eq!(detect_distro_id("", "openSUSE Tumbleweed", ""), "opensuse");
        assert_eq!(detect_distro_id("", "", "rockylinux/9"), "rocky");
        assert_eq!(detect_distro_id("", "unknown", "custom"), "generic");
    }
}
