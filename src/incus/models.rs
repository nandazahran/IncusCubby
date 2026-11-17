use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
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
    pub memory: Option<MemoryState>,
    pub network: Option<HashMap<String, NetworkState>>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSummary {
    pub name: String,
    pub description: String,
}

impl From<ApiProfile> for ProfileSummary {
    fn from(api: ApiProfile) -> Self {
        Self {
            name: api.name,
            description: api.description,
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
    pub status: String,
    pub is_running: bool,
    pub is_frozen: bool,
    pub architecture: String,
    pub image: String,
    pub image_os: String,
    pub ip: String,
    pub memory: String,
    pub cpu: String,
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
        let image_os = api.config.get("image.os").cloned().unwrap_or_default();
        let image = format_os_release(
            api.config.get("image.os").map(String::as_str),
            api.config.get("image.release").map(String::as_str),
            "-",
        );

        let ip = api
            .state
            .as_ref()
            .and_then(|s| s.network.as_ref())
            .and_then(extract_primary_ipv4)
            .unwrap_or_else(|| "-".to_string());

        let usage_bytes = api
            .state
            .as_ref()
            .and_then(|s| s.memory.as_ref())
            .map(|m| m.usage.max(0) as u64)
            .unwrap_or(0);
        let limit = api.config.get("limits.memory").map(String::as_str);
        let memory = format_memory(is_running, usage_bytes, limit);
        let cpu = api
            .config
            .get("limits.cpu")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "-".to_string());
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
            ip,
            memory,
            cpu,
            workspace_path,
            profiles: api.profiles,
            mounts,
            proxies,
            snapshots,
        }
    }
}

fn format_os_release(os: Option<&str>, rel: Option<&str>, fallback: &str) -> String {
    match (os, rel) {
        (Some(os), Some(rel)) if !os.is_empty() && !rel.is_empty() => format!("{os} {rel}"),
        (Some(os), _) if !os.is_empty() => os.to_string(),
        _ => fallback.to_string(),
    }
}

fn extract_primary_ipv4(network: &HashMap<String, NetworkState>) -> Option<String> {
    let mut keys: Vec<&String> = network.keys().filter(|k| k.as_str() != "lo").collect();
    keys.sort();
    for iface in keys {
        if let Some(state) = network.get(iface) {
            for addr in &state.addresses {
                if addr.family == "inet" && addr.scope == "global" && !addr.address.is_empty() {
                    return Some(addr.address.clone());
                }
            }
        }
    }
    None
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

fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else {
        format!("{:.0} MiB", b / MIB)
    }
}
