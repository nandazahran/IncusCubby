use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

#[derive(Debug, Deserialize)]
pub struct ApiInstance {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub config: HashMap<String, String>,
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
    pub usage: u64,
}

#[derive(Debug, Deserialize)]
pub struct NetworkState {
    #[serde(default)]
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
    pub source: ImageSource,
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
    pub architecture: String,
    pub image: String,
    pub ip: String,
    pub memory: String,
    pub snapshots: Vec<SnapshotSummary>,
}

impl From<ApiInstance> for InstanceSummary {
    fn from(api: ApiInstance) -> Self {
        let is_running = api.status.eq_ignore_ascii_case("Running");
        let image = match (api.config.get("image.os"), api.config.get("image.release")) {
            (Some(os), Some(rel)) if !os.is_empty() && !rel.is_empty() => format!("{os} {rel}"),
            (Some(os), _) if !os.is_empty() => os.clone(),
            _ => "-".to_string(),
        };

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
            .map(|m| m.usage)
            .unwrap_or(0);
        let limit = api.config.get("limits.memory").map(String::as_str);
        let memory = format_memory(is_running, usage_bytes, limit);

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
            architecture: if api.architecture.is_empty() {
                "-".to_string()
            } else {
                api.architecture
            },
            image,
            ip,
            memory,
            snapshots,
        }
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
