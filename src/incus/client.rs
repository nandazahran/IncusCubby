use std::collections::HashMap;
use std::error::Error as StdError;
use std::io;
use std::path::{Path, PathBuf};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyperlocal::{UnixClientExt, UnixConnector, Uri as UnixUri};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::models::{
    ApiInstance, ApiResponse, CreateInstanceRequest, CreateSnapshotRequest, ImageSource,
    InstancePut, InstanceSummary, OperationMetadata, RestoreSnapshotRequest, StateActionRequest,
};

pub const DEFAULT_INCUS_SOCKET: &str = "/var/lib/incus/unix.socket";
const LINUXCONTAINERS_SERVER: &str = "https://images.linuxcontainers.org";

#[derive(Debug, Error)]
pub enum IncusError {
    #[error(
        "Cannot access Incus socket ({path}): {kind:?}. Ensure incusd is running and execute: sudo usermod -aG incus-admin $USER"
    )]
    SocketUnavailable { path: PathBuf, kind: io::ErrorKind },

    #[error("HTTP transport error: {0}")]
    Transport(String),

    #[error("Incus API error ({code}): {message}")]
    Api { code: u16, message: String },

    #[error("Invalid JSON response from Incus: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl IncusError {
    pub fn socket_warning(&self) -> Option<String> {
        match self {
            Self::SocketUnavailable { path, kind } => {
                let reason = match kind {
                    io::ErrorKind::PermissionDenied => "Permission denied",
                    io::ErrorKind::NotFound => "Socket not found",
                    _ => "Connection failed",
                };
                Some(format!(
                    "{reason} for {}. Ensure incusd is running and execute: sudo usermod -aG incus-admin $USER",
                    path.display()
                ))
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct IncusClient {
    socket_path: PathBuf,
    http: Client<UnixConnector, Full<Bytes>>,
}

impl IncusClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            http: Client::unix(),
        }
    }

    pub fn default_socket() -> Self {
        Self::new(DEFAULT_INCUS_SOCKET)
    }

    pub async fn list_instances(&self) -> Result<Vec<InstanceSummary>, IncusError> {
        let resp: ApiResponse<Vec<ApiInstance>> =
            self.send_json(Method::GET, "/1.0/instances?recursion=2", None::<&()>).await?;
        let instances = resp.metadata.unwrap_or_default();
        let mut summaries: Vec<InstanceSummary> =
            instances.into_iter().map(InstanceSummary::from).collect();
        summaries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(summaries)
    }

    pub async fn create_instance(
        &self,
        name: &str,
        image_alias: &str,
        memory_limit: &str,
        cpu_limit: &str,
        workspace_path: &str,
    ) -> Result<(), IncusError> {
        validate_name(name, "instance name")?;
        let alias = image_alias.trim();
        if alias.is_empty() {
            return Err(IncusError::InvalidInput("Image alias cannot be empty".into()));
        }

        let mut config = HashMap::new();
        let mem = memory_limit.trim();
        if !mem.is_empty() {
            config.insert("limits.memory".to_string(), mem.to_string());
        }
        let cpu = cpu_limit.trim();
        if !cpu.is_empty() {
            config.insert("limits.cpu".to_string(), cpu.to_string());
        }

        let mut devices = HashMap::new();
        if let Some(ws_device) = build_workspace_device(workspace_path)? {
            devices.insert("workspace".to_string(), ws_device);
        }

        let payload = CreateInstanceRequest {
            name: name.to_string(),
            architecture: host_incus_arch().to_string(),
            profiles: vec!["default".to_string()],
            config,
            devices,
            source: ImageSource {
                source_type: "image".to_string(),
                mode: "pull".to_string(),
                server: LINUXCONTAINERS_SERVER.to_string(),
                protocol: "simplestreams".to_string(),
                alias: alias.to_string(),
            },
        };

        self.execute_async(Method::POST, "/1.0/instances", Some(&payload)).await
    }

    pub async fn set_workspace_mount(
        &self,
        name: &str,
        workspace_path: &str,
    ) -> Result<(), IncusError> {
        validate_name(name, "instance name")?;
        let new_device = build_workspace_device(workspace_path)?;
        let path = format!("/1.0/instances/{name}");
        let resp: ApiResponse<InstancePut> =
            self.send_json(Method::GET, &path, None::<&()>).await?;
        let mut put = resp.metadata.ok_or_else(|| IncusError::Api {
            code: 500,
            message: format!("Missing instance metadata for '{name}'"),
        })?;
        match new_device {
            Some(dev) => {
                put.devices.insert("workspace".to_string(), dev);
            }
            None => {
                put.devices.remove("workspace");
            }
        }
        self.execute_async(Method::PUT, &path, Some(&put)).await
    }

    pub async fn set_instance_state(&self, name: &str, action: &str) -> Result<(), IncusError> {
        validate_name(name, "instance name")?;
        let path = format!("/1.0/instances/{name}/state");
        let payload = StateActionRequest {
            action: action.to_string(),
            timeout: 30,
            force: false,
            stateful: false,
        };
        self.execute_async(Method::PUT, &path, Some(&payload)).await
    }

    pub async fn delete_instance(&self, name: &str) -> Result<(), IncusError> {
        validate_name(name, "instance name")?;
        let path = format!("/1.0/instances/{name}");
        self.execute_async(Method::DELETE, &path, None::<&()>).await
    }

    pub async fn create_snapshot(&self, instance: &str, snapshot: &str) -> Result<(), IncusError> {
        validate_name(instance, "instance name")?;
        validate_name(snapshot, "snapshot name")?;
        let path = format!("/1.0/instances/{instance}/snapshots");
        let payload = CreateSnapshotRequest {
            name: snapshot.to_string(),
            stateful: false,
        };
        self.execute_async(Method::POST, &path, Some(&payload)).await
    }

    pub async fn restore_snapshot(&self, instance: &str, snapshot: &str) -> Result<(), IncusError> {
        validate_name(instance, "instance name")?;
        validate_name(snapshot, "snapshot name")?;
        let path = format!("/1.0/instances/{instance}");
        let payload = RestoreSnapshotRequest {
            restore: snapshot.to_string(),
        };
        self.execute_async(Method::PUT, &path, Some(&payload)).await
    }

    pub async fn delete_snapshot(&self, instance: &str, snapshot: &str) -> Result<(), IncusError> {
        validate_name(instance, "instance name")?;
        validate_name(snapshot, "snapshot name")?;
        let path = format!("/1.0/instances/{instance}/snapshots/{snapshot}");
        self.execute_async(Method::DELETE, &path, None::<&()>).await
    }

    async fn execute_async<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(), IncusError> {
        let resp: ApiResponse<OperationMetadata> = self.send_json(method, path, body).await?;
        if resp.response_type == "async" && !resp.operation.is_empty() {
            let wait_path = format!("{}/wait", resp.operation.trim_end_matches('/'));
            let op_resp: ApiResponse<OperationMetadata> =
                self.send_json(Method::GET, &wait_path, None::<&()>).await?;
            if let Some(meta) = op_resp.metadata
                && (meta.status_code >= 400 || !meta.err.is_empty())
            {
                let msg = if meta.err.is_empty() {
                    meta.status
                } else {
                    meta.err
                };
                return Err(IncusError::Api {
                    code: meta.status_code,
                    message: msg,
                });
            }
        }
        Ok(())
    }

    async fn send_json<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<ApiResponse<T>, IncusError> {
        let uri: hyper::Uri = UnixUri::new(&self.socket_path, path).into();
        let bytes = match body {
            Some(payload) => Bytes::from(serde_json::to_vec(payload)?),
            None => Bytes::new(),
        };

        let mut builder = Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let req = builder
            .body(Full::new(bytes))
            .map_err(|e| IncusError::Transport(e.to_string()))?;

        let response = self
            .http
            .request(req)
            .await
            .map_err(|err| classify_transport_error(&self.socket_path, &err))?;

        let http_status = response.status();
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| IncusError::Transport(e.to_string()))?
            .to_bytes();

        let parsed: ApiResponse<T> = serde_json::from_slice(&body_bytes)?;
        if parsed.response_type == "error" || http_status.is_client_error() || http_status.is_server_error() {
            let code = if parsed.error_code != 0 {
                parsed.error_code
            } else {
                http_status.as_u16()
            };
            let message = if !parsed.error.is_empty() {
                parsed.error
            } else if !parsed.status.is_empty() {
                parsed.status
            } else {
                StatusCode::from_u16(code)
                    .map(|s| s.canonical_reason().unwrap_or("Unknown error").to_string())
                    .unwrap_or_else(|_| "Unknown error".to_string())
            };
            return Err(IncusError::Api { code, message });
        }

        Ok(parsed)
    }
}

fn classify_transport_error(socket_path: &Path, err: &(dyn StdError + 'static)) -> IncusError {
    let mut cur: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(e) = cur {
        if let Some(io_err) = e.downcast_ref::<io::Error>() {
            match io_err.kind() {
                io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound => {
                    return IncusError::SocketUnavailable {
                        path: socket_path.to_path_buf(),
                        kind: io_err.kind(),
                    };
                }
                _ => {}
            }
        }
        cur = e.source();
    }
    IncusError::Transport(err.to_string())
}

fn validate_name(value: &str, label: &str) -> Result<(), IncusError> {
    if value.is_empty() {
        return Err(IncusError::InvalidInput(format!("{label} cannot be empty")));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(IncusError::InvalidInput(format!(
            "{label} may only contain ASCII letters, digits, '-' and '_'"
        )));
    }
    Ok(())
}

fn build_workspace_device(workspace_path: &str) -> Result<Option<HashMap<String, String>>, IncusError> {
    let trimmed = workspace_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let resolved = if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            IncusError::InvalidInput("Cannot expand '~': HOME is not set".into())
        })?;
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(trimmed)
    };
    if !resolved.is_absolute() || !resolved.is_dir() {
        return Err(IncusError::InvalidInput(format!(
            "Workspace path must be an existing absolute directory: {}",
            resolved.display()
        )));
    }
    let mut device = HashMap::new();
    device.insert("type".to_string(), "disk".to_string());
    device.insert(
        "source".to_string(),
        resolved.to_string_lossy().into_owned(),
    );
    device.insert("path".to_string(), "/workspace".to_string());
    device.insert("shift".to_string(), "true".to_string());
    Ok(Some(device))
}

fn host_incus_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armv7l",
        "riscv64" => "riscv64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};
    use hyper::body::Incoming;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::Response;
    use hyper_util::rt::TokioIo;
    use tokio::net::UnixListener;

    async fn spawn_mock_incus<F>(handler: F) -> (PathBuf, tokio::task::JoinHandle<()>)
    where
        F: Fn(String, String, String) -> (StatusCode, String) + Send + Sync + 'static,
    {
        let dir = std::env::temp_dir().join(format!(
            "incus_cubby_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let sock_path = dir.join("unix.socket");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let handler = Arc::new(handler);

        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req: Request<Incoming>| {
                        let handler = Arc::clone(&handler);
                        async move {
                            let method = req.method().to_string();
                            let uri = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/").to_string();
                            let body_bytes = req.into_body().collect().await.unwrap().to_bytes();
                            let body = String::from_utf8_lossy(&body_bytes).to_string();
                            let (status, resp_json) = handler(method, uri, body);
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(status)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from(resp_json)))
                                    .unwrap(),
                            )
                        }
                    });
                    let _ = http1::Builder::new().serve_connection(io, svc).await;
                });
            }
        });

        (sock_path, handle)
    }

    #[tokio::test]
    async fn missing_socket_maps_to_socket_unavailable() {
        let client = IncusClient::new("/tmp/definitely_missing_incus_cubby_test.socket");
        let err = client.list_instances().await.unwrap_err();
        match &err {
            IncusError::SocketUnavailable { kind, .. } => {
                assert_eq!(*kind, io::ErrorKind::NotFound);
                let warn = err.socket_warning().expect("warning expected");
                assert!(warn.contains("sudo usermod -aG incus-admin $USER"));
            }
            other => panic!("Expected SocketUnavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_instances_parses_state_ip_ram_and_snapshots() {
        let (sock, handle) = spawn_mock_incus(|method, uri, _| {
            assert_eq!(method, "GET");
            assert_eq!(uri, "/1.0/instances?recursion=2");
            let json = r#"{
                "type": "sync",
                "status": "Success",
                "status_code": 200,
                "metadata": [{
                    "name": "dev-rust",
                    "status": "Running",
                    "architecture": "x86_64",
                    "config": {
                        "limits.memory": "4GiB",
                        "image.os": "Debian",
                        "image.release": "bookworm"
                    },
                    "state": {
                        "status": "Running",
                        "memory": { "usage": 268435456, "usage_peak": 536870912 },
                        "network": {
                            "lo": { "addresses": [{"family": "inet", "address": "127.0.0.1", "scope": "local"}] },
                            "eth0": { "addresses": [{"family": "inet", "address": "10.12.34.56", "scope": "global"}] }
                        }
                    },
                    "snapshots": [
                        { "name": "clean-base", "created_at": "2026-11-14T10:00:00Z" }
                    ]
                }]
            }"#;
            (StatusCode::OK, json.to_string())
        })
        .await;

        let client = IncusClient::new(&sock);
        let list = client.list_instances().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "dev-rust");
        assert!(list[0].is_running);
        assert_eq!(list[0].ip, "10.12.34.56");
        assert_eq!(list[0].memory, "256 MiB / 4GiB");
        assert_eq!(list[0].image, "Debian bookworm");
        assert_eq!(list[0].snapshots.len(), 1);
        assert_eq!(list[0].snapshots[0].name, "clean-base");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn create_instance_sends_simplestreams_payload_and_waits_operation() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            seen_clone.lock().unwrap().push((method.clone(), uri.clone(), body));
            if method == "POST" && uri == "/1.0/instances" {
                let resp = r#"{
                    "type": "async",
                    "status": "Operation created",
                    "status_code": 100,
                    "operation": "/1.0/operations/op-123"
                }"#;
                (StatusCode::ACCEPTED, resp.to_string())
            } else if method == "GET" && uri == "/1.0/operations/op-123/wait" {
                let resp = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": { "id": "op-123", "status": "Success", "status_code": 200, "err": "" }
                }"#;
                (StatusCode::OK, resp.to_string())
            } else {
                (StatusCode::NOT_FOUND, r#"{"type":"error","error":"unexpected"}"#.to_string())
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        let ws_dir = sock.parent().unwrap();
        client
            .create_instance("box1", "debian/12", "4GiB", "2", &ws_dir.to_string_lossy())
            .await
            .unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "POST");
        assert_eq!(calls[0].1, "/1.0/instances");
        let sent: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
        assert_eq!(sent["name"], "box1");
        assert_eq!(sent["config"]["limits.memory"], "4GiB");
        assert_eq!(sent["config"]["limits.cpu"], "2");
        assert_eq!(sent["devices"]["workspace"]["type"], "disk");
        assert_eq!(sent["devices"]["workspace"]["path"], "/workspace");
        assert_eq!(sent["devices"]["workspace"]["shift"], "true");
        assert_eq!(sent["source"]["server"], "https://images.linuxcontainers.org");
        assert_eq!(sent["source"]["protocol"], "simplestreams");
        assert_eq!(sent["source"]["alias"], "debian/12");
        assert_eq!(calls[1].0, "GET");
        assert_eq!(calls[1].1, "/1.0/operations/op-123/wait");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn lifecycle_and_snapshot_endpoints_work_and_surface_api_errors() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            seen_clone
                .lock()
                .unwrap()
                .push((method.clone(), uri.clone(), body));
            if uri == "/1.0/instances/box1/snapshots/bad" && method == "DELETE" {
                let err_resp = r#"{"type":"error","error_code":404,"error":"Snapshot not found"}"#;
                return (StatusCode::NOT_FOUND, err_resp.to_string());
            }
            if uri == "/1.0/instances/box1" && method == "GET" {
                let get_resp = r#"{"type":"sync","status":"Success","status_code":200,"metadata":{"architecture":"x86_64","config":{},"description":"","devices":{"workspace":{"type":"disk","source":"/tmp","path":"/workspace","shift":"true"}},"ephemeral":false,"profiles":["default"],"stateful":false}}"#;
                return (StatusCode::OK, get_resp.to_string());
            }
            let ok = r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#;
            (StatusCode::OK, ok.to_string())
        })
        .await;

        let client = IncusClient::new(&sock);
        let ws_dir = sock.parent().unwrap().to_string_lossy().into_owned();
        client.set_instance_state("box1", "start").await.unwrap();
        client.set_workspace_mount("box1", &ws_dir).await.unwrap();
        client.set_workspace_mount("box1", "").await.unwrap();
        client.create_snapshot("box1", "s1").await.unwrap();
        client.restore_snapshot("box1", "s1").await.unwrap();
        client.delete_snapshot("box1", "s1").await.unwrap();
        client.delete_instance("box1").await.unwrap();

        let err = client.delete_snapshot("box1", "bad").await.unwrap_err();
        assert!(matches!(err, IncusError::Api { code: 404, .. }));

        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "PUT");
        assert_eq!(calls[0].1, "/1.0/instances/box1/state");
        assert_eq!(calls[1].0, "GET");
        assert_eq!(calls[1].1, "/1.0/instances/box1");
        assert_eq!(calls[2].0, "PUT");
        assert_eq!(calls[2].1, "/1.0/instances/box1");
        assert!(calls[2].2.contains(r#""shift":"true""#));
        assert_eq!(calls[3].0, "GET");
        assert_eq!(calls[3].1, "/1.0/instances/box1");
        assert_eq!(calls[4].0, "PUT");
        assert_eq!(calls[4].1, "/1.0/instances/box1");
        assert!(!calls[4].2.contains("workspace"));
        assert_eq!(calls[5].0, "POST");
        assert_eq!(calls[5].1, "/1.0/instances/box1/snapshots");
        assert_eq!(calls[6].0, "PUT");
        assert_eq!(calls[6].1, "/1.0/instances/box1");
        assert!(calls[6].2.contains(r#""restore":"s1""#));
        assert_eq!(calls[7].0, "DELETE");
        assert_eq!(calls[7].1, "/1.0/instances/box1/snapshots/s1");
        assert_eq!(calls[8].0, "DELETE");
        assert_eq!(calls[8].1, "/1.0/instances/box1");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    #[ignore = "requires local running incusd socket at /var/lib/incus/unix.socket"]
    async fn live_incus_full_lifecycle_smoke() {
        let client = IncusClient::default_socket();
        let name = format!("cubby-smoke-{}", std::process::id());
        let ws_dir = std::env::temp_dir().join(format!("cubby-ws-{}", std::process::id()));
        std::fs::create_dir_all(&ws_dir).unwrap();
        let ws_str = ws_dir.to_string_lossy().into_owned();

        // Cleanup helper in case a prior run left anything behind.
        let _ = client.set_instance_state(&name, "stop").await;
        let _ = client.delete_instance(&name).await;

        let initial = client.list_instances().await.expect("list_instances");
        assert!(initial.iter().all(|i| i.name != name));

        client
            .create_instance(&name, "alpine/3.24", "512MiB", "1", &ws_str)
            .await
            .expect("create_instance");

        let res = async {
            client
                .set_instance_state(&name, "start")
                .await
                .expect("start");

            let after_start = client.list_instances().await.expect("list after start");
            let created = after_start
                .iter()
                .find(|i| i.name == name)
                .expect("created instance present");
            assert!(created.is_running);
            assert_eq!(created.cpu, "1");
            assert_eq!(created.workspace_path, ws_str);

            client
                .set_workspace_mount(&name, "")
                .await
                .expect("unmount workspace");
            let after_unmount = client.list_instances().await.expect("list after unmount");
            let unmounted = after_unmount.iter().find(|i| i.name == name).unwrap();
            assert_eq!(unmounted.workspace_path, "");

            client
                .create_snapshot(&name, "snap1")
                .await
                .expect("create_snapshot");

            let with_snap = client.list_instances().await.expect("list with snap");
            let found = with_snap.iter().find(|i| i.name == name).unwrap();
            assert!(found.snapshots.iter().any(|s| s.name == "snap1"));

            client
                .set_instance_state(&name, "stop")
                .await
                .expect("stop");
            client
                .restore_snapshot(&name, "snap1")
                .await
                .expect("restore_snapshot");
            client
                .delete_snapshot(&name, "snap1")
                .await
                .expect("delete_snapshot");
        }
        .await;

        let _ = client.set_instance_state(&name, "stop").await;
        client.delete_instance(&name).await.expect("delete_instance");
        let _ = std::fs::remove_dir_all(&ws_dir);
        res
    }
}
