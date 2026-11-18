use std::collections::HashMap;

use hyper::Method;
use hyper::body::Bytes;
use serde::{Deserialize, Serialize};

use super::client::{IncusClient, IncusError, validate_name};
use super::models::{ApiResponse, format_bytes, null_to_default};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePoolSummary {
    pub name: String,
    pub driver: String,
    pub status: String,
    pub used_display: String,
    pub total_display: String,
    pub used_per_mille: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolumeSummary {
    pub name: String,
    pub pool: String,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkLeaseSummary {
    pub hostname: String,
    pub address: String,
    pub hwaddr: String,
    pub lease_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeNat {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSummary {
    pub name: String,
    pub net_type: String,
    pub managed: bool,
    pub ipv4: String,
    pub nat: bool,
    pub leases: Vec<NetworkLeaseSummary>,
}

#[derive(Debug, Deserialize)]
struct ApiStoragePool {
    #[serde(default)]
    name: String,
    #[serde(default)]
    driver: String,
    #[serde(default)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct ApiStoragePoolResources {
    #[serde(default)]
    space: Option<ApiStorageSpace>,
}

#[derive(Debug, Deserialize)]
struct ApiStorageSpace {
    #[serde(default)]
    used: u64,
    #[serde(default)]
    total: u64,
}

#[derive(Debug, Deserialize)]
struct ApiStorageVolume {
    #[serde(default)]
    name: String,
    #[serde(default)]
    content_type: String,
}

#[derive(Debug, Serialize)]
struct CreateStorageVolumeRequest {
    name: String,
    content_type: String,
    #[serde(rename = "type")]
    volume_type: String,
}

#[derive(Debug, Deserialize)]
struct ApiNetwork {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    net_type: String,
    #[serde(default)]
    managed: bool,
    #[serde(default, deserialize_with = "null_to_default")]
    config: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ApiNetworkLease {
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    hwaddr: String,
    #[serde(rename = "type", default)]
    lease_type: String,
}

#[derive(Debug, Serialize)]
struct CreateNetworkRequest {
    name: String,
    #[serde(rename = "type")]
    net_type: String,
    config: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct CreateProfileRequest {
    name: String,
    description: String,
    config: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProfilePut {
    #[serde(default)]
    description: String,
    #[serde(default, deserialize_with = "null_to_default")]
    config: HashMap<String, String>,
    #[serde(default, deserialize_with = "null_to_default")]
    devices: HashMap<String, HashMap<String, String>>,
}

impl IncusClient {
    pub async fn list_storage_pools(&self) -> Result<Vec<StoragePoolSummary>, IncusError> {
        let resp: ApiResponse<Vec<ApiStoragePool>> = self
            .send_json(Method::GET, "/1.0/storage-pools?recursion=1", None::<&()>)
            .await?;
        let mut pools = Vec::new();
        for pool in resp.metadata.unwrap_or_default() {
            let res_path = format!("/1.0/storage-pools/{}/resources", pool.name);
            let space = match self
                .send_json::<(), ApiStoragePoolResources>(Method::GET, &res_path, None)
                .await
            {
                Ok(r) => r.metadata.and_then(|m| m.space),
                Err(err @ IncusError::SocketUnavailable { .. }) => return Err(err),
                Err(_) => None,
            };
            let (used_display, total_display, used_per_mille) = match space {
                Some(s) if s.total > 0 => {
                    let ratio =
                        ((u128::from(s.used) * 1000) / u128::from(s.total)).min(1000) as u16;
                    (format_bytes(s.used), format_bytes(s.total), ratio)
                }
                Some(s) if s.used > 0 => (format_bytes(s.used), "-".to_string(), 0),
                _ => ("-".to_string(), "-".to_string(), 0),
            };
            pools.push(StoragePoolSummary {
                name: pool.name,
                driver: pool.driver,
                status: pool.status,
                used_display,
                total_display,
                used_per_mille,
            });
        }
        pools.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(pools)
    }

    pub async fn list_storage_volumes(
        &self,
        pool: &str,
    ) -> Result<Vec<StorageVolumeSummary>, IncusError> {
        validate_name(pool, "storage pool")?;
        let path = format!("/1.0/storage-pools/{pool}/volumes/custom?recursion=1");
        let resp: ApiResponse<Vec<ApiStorageVolume>> =
            self.send_json(Method::GET, &path, None::<&()>).await?;
        let mut volumes: Vec<StorageVolumeSummary> = resp
            .metadata
            .unwrap_or_default()
            .into_iter()
            .map(|v| StorageVolumeSummary {
                name: v.name,
                pool: pool.to_string(),
                content_type: v.content_type,
            })
            .collect();
        volumes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(volumes)
    }

    pub async fn create_storage_volume(&self, pool: &str, name: &str) -> Result<(), IncusError> {
        validate_name(pool, "storage pool")?;
        validate_name(name, "volume name")?;
        let path = format!("/1.0/storage-pools/{pool}/volumes/custom");
        let payload = CreateStorageVolumeRequest {
            name: name.to_string(),
            content_type: "filesystem".to_string(),
            volume_type: "custom".to_string(),
        };
        self.execute_async(Method::POST, &path, Some(&payload))
            .await
    }

    pub async fn delete_storage_volume(&self, pool: &str, name: &str) -> Result<(), IncusError> {
        validate_name(pool, "storage pool")?;
        validate_name(name, "volume name")?;
        let path = format!("/1.0/storage-pools/{pool}/volumes/custom/{name}");
        self.execute_async(Method::DELETE, &path, None::<&()>).await
    }

    pub async fn attach_custom_volume(
        &self,
        instance: &str,
        dev_name: &str,
        pool: &str,
        volume: &str,
        container_path: &str,
    ) -> Result<(), IncusError> {
        validate_name(dev_name, "device name")?;
        validate_name(pool, "storage pool")?;
        validate_name(volume, "volume name")?;
        let cpath = validate_container_path(container_path)?;
        let mut dev = HashMap::new();
        dev.insert("type".to_string(), "disk".to_string());
        dev.insert("pool".to_string(), pool.to_string());
        dev.insert("source".to_string(), volume.to_string());
        dev.insert("path".to_string(), cpath.to_string());
        let key = dev_name.to_string();
        self.update_instance_devices(instance, move |devices| {
            devices.insert(key, dev);
        })
        .await
    }

    pub async fn list_networks(&self) -> Result<Vec<NetworkSummary>, IncusError> {
        let resp: ApiResponse<Vec<ApiNetwork>> = self
            .send_json(Method::GET, "/1.0/networks?recursion=1", None::<&()>)
            .await?;
        let mut networks = Vec::new();
        for net in resp.metadata.unwrap_or_default() {
            let ipv4 = net
                .config
                .get("ipv4.address")
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| "-".to_string());
            let nat = net
                .config
                .get("ipv4.nat")
                .is_some_and(|v| v.eq_ignore_ascii_case("true"));
            let mut leases = if net.managed {
                let leases_path = format!("/1.0/networks/{}/leases", net.name);
                match self
                    .send_json::<(), Vec<ApiNetworkLease>>(Method::GET, &leases_path, None)
                    .await
                {
                    Ok(r) => r
                        .metadata
                        .unwrap_or_default()
                        .into_iter()
                        .map(|l| NetworkLeaseSummary {
                            hostname: l.hostname,
                            address: l.address,
                            hwaddr: l.hwaddr,
                            lease_type: l.lease_type,
                        })
                        .collect(),
                    Err(err @ IncusError::SocketUnavailable { .. }) => return Err(err),
                    Err(_) => Vec::new(),
                }
            } else {
                Vec::new()
            };
            leases.sort_by(|a, b| a.hostname.cmp(&b.hostname).then(a.address.cmp(&b.address)));
            networks.push(NetworkSummary {
                name: net.name,
                net_type: net.net_type,
                managed: net.managed,
                ipv4,
                nat,
                leases,
            });
        }
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(networks)
    }

    pub async fn create_network(&self, name: &str, nat: BridgeNat) -> Result<(), IncusError> {
        validate_name(name, "network name")?;
        let nat_str = match nat {
            BridgeNat::Enabled => "true",
            BridgeNat::Disabled => "false",
        }
        .to_string();
        let mut config = HashMap::new();
        config.insert("ipv4.nat".to_string(), nat_str.clone());
        config.insert("ipv6.nat".to_string(), nat_str);
        let payload = CreateNetworkRequest {
            name: name.to_string(),
            net_type: "bridge".to_string(),
            config,
        };
        self.execute_async(Method::POST, "/1.0/networks", Some(&payload))
            .await
    }

    pub async fn delete_network(&self, name: &str) -> Result<(), IncusError> {
        validate_name(name, "network name")?;
        let path = format!("/1.0/networks/{name}");
        self.execute_async(Method::DELETE, &path, None::<&()>).await
    }

    pub async fn create_profile(
        &self,
        name: &str,
        description: &str,
        cpu_limit: &str,
        memory_limit: &str,
    ) -> Result<(), IncusError> {
        validate_name(name, "profile name")?;
        let mut config = HashMap::new();
        set_or_remove_limit(&mut config, "limits.cpu", cpu_limit);
        set_or_remove_limit(&mut config, "limits.memory", memory_limit);
        let payload = CreateProfileRequest {
            name: name.to_string(),
            description: description.trim().to_string(),
            config,
        };
        self.execute_async(Method::POST, "/1.0/profiles", Some(&payload))
            .await
    }

    pub async fn update_profile(
        &self,
        name: &str,
        description: &str,
        cpu_limit: &str,
        memory_limit: &str,
    ) -> Result<(), IncusError> {
        validate_name(name, "profile name")?;
        let path = format!("/1.0/profiles/{name}");
        let resp: ApiResponse<ProfilePut> = self.send_json(Method::GET, &path, None::<&()>).await?;
        let mut put = resp.metadata.ok_or_else(|| IncusError::Api {
            code: 500,
            message: format!("Missing profile metadata for '{name}'"),
        })?;
        put.description = description.trim().to_string();
        set_or_remove_limit(&mut put.config, "limits.cpu", cpu_limit);
        set_or_remove_limit(&mut put.config, "limits.memory", memory_limit);
        self.execute_async(Method::PUT, &path, Some(&put)).await
    }

    pub async fn delete_profile(&self, name: &str) -> Result<(), IncusError> {
        validate_name(name, "profile name")?;
        if name == "default" {
            return Err(IncusError::InvalidInput(
                "Cannot delete the 'default' profile".into(),
            ));
        }
        let path = format!("/1.0/profiles/{name}");
        self.execute_async(Method::DELETE, &path, None::<&()>).await
    }

    pub async fn list_instance_logs(&self, instance: &str) -> Result<Vec<String>, IncusError> {
        validate_name(instance, "instance name")?;
        let path = format!("/1.0/instances/{instance}/logs");
        let resp: ApiResponse<Vec<String>> =
            self.send_json(Method::GET, &path, None::<&()>).await?;
        let mut logs: Vec<String> = resp
            .metadata
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| {
                p.rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .collect();
        logs.sort();
        Ok(logs)
    }

    pub async fn read_instance_log(
        &self,
        instance: &str,
        log_name: &str,
    ) -> Result<String, IncusError> {
        validate_name(instance, "instance name")?;
        if log_name.is_empty()
            || matches!(log_name, "." | "..")
            || log_name.contains("..")
            || !log_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(IncusError::InvalidInput("Invalid log file name".into()));
        }
        self.send_raw_get(&format!("/1.0/instances/{instance}/logs/{log_name}"))
            .await
    }

    pub async fn push_instance_file(
        &self,
        instance: &str,
        container_path: &str,
        data: Vec<u8>,
    ) -> Result<(), IncusError> {
        validate_name(instance, "instance name")?;
        let cpath = validate_container_path(container_path)?;
        let encoded = percent_encode(cpath);
        let path = format!("/1.0/instances/{instance}/files?path={encoded}");
        let headers = [
            ("content-type", "application/octet-stream"),
            ("X-Incus-type", "file"),
            ("X-Incus-mode", "0644"),
        ];
        self.send_raw_bytes(Method::POST, &path, Bytes::from(data), &headers)
            .await?;
        Ok(())
    }

    pub async fn pull_instance_file(
        &self,
        instance: &str,
        container_path: &str,
    ) -> Result<Vec<u8>, IncusError> {
        validate_name(instance, "instance name")?;
        let cpath = validate_container_path(container_path)?;
        let encoded = percent_encode(cpath);
        let path = format!("/1.0/instances/{instance}/files?path={encoded}");
        let (resp_headers, bytes) = self
            .send_raw_bytes_with_headers(Method::GET, &path, Bytes::new(), &[])
            .await?;
        if resp_headers
            .get("x-incus-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.eq_ignore_ascii_case("directory"))
        {
            return Err(IncusError::InvalidInput(format!(
                "Cannot pull directory '{cpath}': specify a file path"
            )));
        }
        Ok(bytes.to_vec())
    }
}

fn validate_container_path(container_path: &str) -> Result<&str, IncusError> {
    let cpath = container_path.trim();
    if cpath.is_empty() || !cpath.starts_with('/') {
        return Err(IncusError::InvalidInput(
            "Container path must be a non-empty absolute path starting with '/'".into(),
        ));
    }
    Ok(cpath)
}

fn set_or_remove_limit(config: &mut HashMap<String, String>, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        config.remove(key);
    } else {
        config.insert(key.to_string(), trimmed.to_string());
    }
}

fn percent_encode(input: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incus::client::tests::{HeaderList, spawn_mock_incus_raw};
    use hyper::StatusCode;
    use std::sync::{Arc, Mutex};

    #[test]
    fn percent_encode_encodes_paths_and_spaces() {
        assert_eq!(percent_encode("/etc/hosts"), "%2Fetc%2Fhosts");
        assert_eq!(
            percent_encode("/tmp/my file-1_2.txt"),
            "%2Ftmp%2Fmy%20file-1_2.txt"
        );
    }

    #[test]
    fn deserialize_handles_null_maps_and_slices() {
        let json = r#"{
            "type": "sync",
            "metadata": [{
                "name": "incusbr0",
                "type": "bridge",
                "managed": true,
                "config": null
            }]
        }"#;
        let resp: ApiResponse<Vec<ApiNetwork>> = serde_json::from_str(json).unwrap();
        let nets = resp.metadata.unwrap();
        assert_eq!(nets.len(), 1);
        assert!(nets[0].config.is_empty());

        let profile_json = r#"{"description":"dev","config":null,"devices":null}"#;
        let put: ProfilePut = serde_json::from_str(profile_json).unwrap();
        assert_eq!(put.description, "dev");
        assert!(put.config.is_empty());
        assert!(put.devices.is_empty());
    }

    #[tokio::test]
    async fn validation_rejects_bad_inputs() {
        let client = IncusClient::new("/tmp/unused_infra_test.socket");
        assert!(matches!(
            client.delete_profile("default").await,
            Err(IncusError::InvalidInput(_))
        ));
        for bad_log in ["../etc/passwd", "", ".", "..", "foo bar", "a?b"] {
            assert!(matches!(
                client.read_instance_log("box1", bad_log).await,
                Err(IncusError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            client
                .push_instance_file("box1", "relative/path", vec![1])
                .await,
            Err(IncusError::InvalidInput(_))
        ));
        assert!(matches!(
            client
                .attach_custom_volume("box1", "data", "default", "vol1", "relative")
                .await,
            Err(IncusError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn storage_pools_volumes_and_attach_work() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus_raw(move |method, uri, _, body| {
            let body_str = String::from_utf8_lossy(&body).into_owned();
            seen_clone
                .lock()
                .unwrap()
                .push((method.clone(), uri.clone(), body_str));
            let (status, resp_bytes) = match (method.as_str(), uri.as_str()) {
                ("GET", "/1.0/storage-pools?recursion=1") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":[{"name":"default","driver":"zfs","status":"Created"}]}"#
                        .to_vec(),
                ),
                ("GET", "/1.0/storage-pools/default/resources") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":{"space":{"used":536870912,"total":2147483648}}}"#
                        .to_vec(),
                ),
                ("GET", "/1.0/storage-pools/default/volumes/custom?recursion=1") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":[{"name":"cargo-cache","content_type":"filesystem"}]}"#
                        .to_vec(),
                ),
                ("POST", "/1.0/storage-pools/default/volumes/custom")
                | ("DELETE", "/1.0/storage-pools/default/volumes/custom/cargo-cache")
                | ("PUT", "/1.0/instances/box1") => (
                    StatusCode::OK,
                    br#"{"type":"sync","status":"Success"}"#.to_vec(),
                ),
                ("GET", "/1.0/instances/box1") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":{"architecture":"x86_64","config":{},"description":"","devices":{},"ephemeral":false,"profiles":["default"],"stateful":false}}"#
                        .to_vec(),
                ),
                _ => (StatusCode::NOT_FOUND, br#"{"type":"error","error":"not found"}"#.to_vec()),
            };
            (status, Vec::new(), resp_bytes)
        })
        .await;

        let client = IncusClient::new(&sock);
        let pools = client.list_storage_pools().await.unwrap();
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].name, "default");
        assert_eq!(pools[0].used_display, "512 MiB");
        assert_eq!(pools[0].total_display, "2.00 GiB");

        let vols = client.list_storage_volumes("default").await.unwrap();
        assert_eq!(vols.len(), 1);
        assert_eq!(vols[0].name, "cargo-cache");
        assert_eq!(vols[0].pool, "default");

        client
            .create_storage_volume("default", "cargo-cache")
            .await
            .unwrap();
        client
            .attach_custom_volume("box1", "cargo", "default", "cargo-cache", "/root/.cargo")
            .await
            .unwrap();
        client
            .delete_storage_volume("default", "cargo-cache")
            .await
            .unwrap();

        let calls = seen.lock().unwrap();
        let create_body: serde_json::Value = serde_json::from_str(&calls[3].2).unwrap();
        assert_eq!(create_body["type"], "custom");
        assert_eq!(create_body["content_type"], "filesystem");
        let put_body: serde_json::Value = serde_json::from_str(&calls[5].2).unwrap();
        assert_eq!(put_body["devices"]["cargo"]["type"], "disk");
        assert_eq!(put_body["devices"]["cargo"]["pool"], "default");
        assert_eq!(put_body["devices"]["cargo"]["source"], "cargo-cache");
        assert_eq!(put_body["devices"]["cargo"]["path"], "/root/.cargo");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn networks_profiles_logs_and_files_work() {
        let seen = Arc::new(Mutex::new(
            Vec::<(String, String, HeaderList, Vec<u8>)>::new(),
        ));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus_raw(move |method, uri, headers, body| {
            seen_clone
                .lock()
                .unwrap()
                .push((method.clone(), uri.clone(), headers, body));
            let (status, resp_bytes) = match (method.as_str(), uri.as_str()) {
                ("GET", "/1.0/networks?recursion=1") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":[{"name":"incusbr0","type":"bridge","managed":true,"config":{"ipv4.address":"10.0.0.1/24","ipv4.nat":"true"}}]}"#
                        .to_vec(),
                ),
                ("GET", "/1.0/networks/incusbr0/leases") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":[{"hostname":"box1","address":"10.0.0.10","hwaddr":"00:16:3e:aa:bb:cc","type":"dynamic"}]}"#
                        .to_vec(),
                ),
                ("POST", "/1.0/networks" | "/1.0/profiles" | "/1.0/instances/box1/files?path=%2Ftmp%2Fhello.txt")
                | ("DELETE", "/1.0/networks/devnet0" | "/1.0/profiles/dev")
                | ("PUT", "/1.0/profiles/dev") => (
                    StatusCode::OK,
                    br#"{"type":"sync","status":"Success"}"#.to_vec(),
                ),
                ("GET", "/1.0/profiles/dev") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":{"description":"old","config":{"limits.cpu":"4","user.keep":"yes"},"devices":{"eth0":{"type":"nic"}}}}"#
                        .to_vec(),
                ),
                ("GET", "/1.0/instances/box1/logs") => (
                    StatusCode::OK,
                    br#"{"type":"sync","metadata":["/1.0/instances/box1/logs/lxc.log","/1.0/instances/box1/logs/lxc.conf"]}"#
                        .to_vec(),
                ),
                ("GET", "/1.0/instances/box1/logs/lxc.log") => {
                    (StatusCode::OK, b"boot ok\n".to_vec())
                }
                ("GET", "/1.0/instances/box1/files?path=%2Ftmp%2Fhello.txt") => {
                    (StatusCode::OK, b"hello world".to_vec())
                }
                _ => (StatusCode::NOT_FOUND, br#"{"type":"error","error":"not found"}"#.to_vec()),
            };
            (status, Vec::new(), resp_bytes)
        })
        .await;

        let client = IncusClient::new(&sock);
        let nets = client.list_networks().await.unwrap();
        assert_eq!(nets.len(), 1);
        assert!(nets[0].nat);
        assert_eq!(nets[0].ipv4, "10.0.0.1/24");
        assert_eq!(nets[0].leases[0].hostname, "box1");

        client
            .create_network("devnet0", BridgeNat::Disabled)
            .await
            .unwrap();
        client.delete_network("devnet0").await.unwrap();

        client
            .create_profile("dev", "Dev profile", "2", "4GiB")
            .await
            .unwrap();
        client
            .update_profile("dev", "Updated", "", "8GiB")
            .await
            .unwrap();
        client.delete_profile("dev").await.unwrap();

        let logs = client.list_instance_logs("box1").await.unwrap();
        assert_eq!(logs, vec!["lxc.conf", "lxc.log"]);
        let log_body = client.read_instance_log("box1", "lxc.log").await.unwrap();
        assert_eq!(log_body, "boot ok\n");

        client
            .push_instance_file("box1", "/tmp/hello.txt", b"hello world".to_vec())
            .await
            .unwrap();
        let pulled = client
            .pull_instance_file("box1", "/tmp/hello.txt")
            .await
            .unwrap();
        assert_eq!(pulled, b"hello world");

        let calls = seen.lock().unwrap();
        let update_call = calls
            .iter()
            .find(|(m, u, _, _)| m == "PUT" && u == "/1.0/profiles/dev")
            .unwrap();
        let update_json: serde_json::Value = serde_json::from_slice(&update_call.3).unwrap();
        assert_eq!(update_json["description"], "Updated");
        assert!(update_json["config"].get("limits.cpu").is_none());
        assert_eq!(update_json["config"]["limits.memory"], "8GiB");
        assert_eq!(update_json["config"]["user.keep"], "yes");
        assert_eq!(update_json["devices"]["eth0"]["type"], "nic");

        let push_call = calls
            .iter()
            .find(|(m, u, _, _)| m == "POST" && u.starts_with("/1.0/instances/box1/files"))
            .unwrap();
        assert!(
            push_call
                .2
                .iter()
                .any(|(k, v)| k == "x-incus-type" && v == "file")
        );
        assert!(
            push_call
                .2
                .iter()
                .any(|(k, v)| k == "x-incus-mode" && v == "0644")
        );

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn pull_instance_file_rejects_directory() {
        let (sock, handle) = spawn_mock_incus_raw(|method, uri, _, _| {
            if method == "GET" && uri == "/1.0/instances/box1/files?path=%2Fetc" {
                (
                    StatusCode::OK,
                    vec![("X-Incus-Type".to_string(), "directory".to_string())],
                    Vec::new(),
                )
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Vec::new(),
                    br#"{"type":"error","error":"not found"}"#.to_vec(),
                )
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        let err = client.pull_instance_file("box1", "/etc").await.unwrap_err();
        assert!(matches!(err, IncusError::InvalidInput(msg) if msg.contains("directory")));

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }
}
