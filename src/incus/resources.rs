use hyper::Method;
use serde::Deserialize;

use super::client::{IncusClient, IncusError};
use super::models::{ApiResponse, format_bytes, null_to_default};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ApiResources {
    cpu: ApiCpu,
    load: ApiLoad,
    memory: ApiMemory,
    system: ApiSystem,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ApiCpu {
    architecture: String,
    #[serde(default, deserialize_with = "null_to_default")]
    sockets: Vec<ApiCpuSocket>,
    total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ApiCpuSocket {
    name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ApiLoad {
    #[serde(rename = "Average1Min")]
    avg1: f64,
    #[serde(rename = "Average5Min")]
    avg5: f64,
    #[serde(rename = "Average10Min")]
    avg10: f64,
    #[serde(rename = "Processes")]
    processes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ApiMemory {
    used: u64,
    total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ApiSystem {
    vendor: String,
    product: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostResources {
    pub cpu_summary: String,
    pub mem_summary: String,
    pub detail_cpu: String,
    pub detail_mem: String,
    pub detail_system: String,
    pub load_detail: String,
}

impl IncusClient {
    pub async fn host_resources(&self) -> Result<HostResources, IncusError> {
        let resp: ApiResponse<ApiResources> = self
            .send_json(Method::GET, "/1.0/resources", None::<&()>)
            .await?;
        Ok(summarize(&resp.metadata.unwrap_or_default()))
    }
}

fn summarize(r: &ApiResources) -> HostResources {
    let cpu_summary = format!("load {:.1}", r.load.avg1);
    let mem_summary = format!(
        "{} / {}",
        format_bytes(r.memory.used),
        format_bytes(r.memory.total)
    );

    let detail_cpu = match r.cpu.sockets.first() {
        Some(socket) => format!("{} · {} threads", socket.name, r.cpu.total),
        None => r.cpu.architecture.clone(),
    };

    let used_pct = (r.memory.used * 100)
        .checked_div(r.memory.total)
        .unwrap_or(0);
    let detail_mem = format!(
        "{} / {} ({:.0}%)",
        format_bytes(r.memory.used),
        format_bytes(r.memory.total),
        used_pct
    );

    let detail_system = format!("{} {}", r.system.vendor, r.system.product)
        .trim()
        .to_string();

    let load_detail = format!(
        "{:.2} · {:.2} · {:.2}",
        r.load.avg1, r.load.avg5, r.load.avg10
    );

    HostResources {
        cpu_summary,
        mem_summary,
        detail_cpu,
        detail_mem,
        detail_system,
        load_detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_formats_host_metrics() {
        let json = r#"{
            "cpu": {
                "architecture": "x86_64",
                "sockets": [{"name": "AMD Ryzen 7 7735HS with Radeon Graphics"}],
                "total": 16
            },
            "load": {
                "Average1Min": 6.62,
                "Average5Min": 5.47,
                "Average10Min": 4.89,
                "Processes": 582
            },
            "memory": {
                "used": 14058037248,
                "total": 16642998272
            },
            "system": {
                "vendor": "LENOVO",
                "product": "83S0"
            }
        }"#;
        let api: ApiResources = serde_json::from_str(json).unwrap();
        let summary = summarize(&api);
        assert!(summary.load_detail.starts_with("6.62"));
        assert_eq!(summary.cpu_summary, "load 6.6");
        assert!(
            summary
                .detail_cpu
                .contains("AMD Ryzen 7 7735HS with Radeon Graphics")
        );
        assert!(summary.detail_cpu.contains("16 threads"));
        assert_ne!(summary.mem_summary, "");
        assert!(summary.mem_summary.contains('/'));
        assert!(summary.detail_mem.contains("(84%)"));
        assert_eq!(summary.detail_system, "LENOVO 83S0");
    }

    #[test]
    fn summarize_handles_null_and_missing_fields() {
        let json = r#"{
            "cpu": {
                "architecture": "x86_64",
                "sockets": null,
                "total": 16
            }
        }"#;
        let api: ApiResources = serde_json::from_str(json).unwrap();
        let summary = summarize(&api);
        assert_eq!(summary.detail_cpu, "x86_64");
        assert_eq!(summary.load_detail, "0.00 · 0.00 · 0.00");
        assert_eq!(summary.detail_system, "");
        assert_eq!(summary.cpu_summary, "load 0.0");
    }
}
