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
    ApiImage, ApiInstance, ApiProfile, ApiResponse, CloneInstanceRequest, CloneSource,
    CreateInstanceRequest, CreateSnapshotRequest, ExecOutput, ExecRequest, ImageSource,
    ImageSummary, InstancePut, InstanceSummary, OperationMetadata, ProfileSummary,
    RestoreSnapshotRequest, StateActionRequest,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NestingMode {
    #[default]
    Disabled,
    Enabled,
}

pub struct CreateInstanceParams<'a> {
    pub name: &'a str,
    pub instance_type: &'a str,
    pub image_alias: &'a str,
    pub memory_limit: &'a str,
    pub cpu_limit: &'a str,
    pub disk_limit: &'a str,
    pub workspace_path: &'a str,
    pub profiles: Vec<String>,
    pub cloud_init_user_data: &'a str,
    pub nesting: NestingMode,
}

#[derive(Clone, Debug)]
pub struct IncusClient {
    socket_path: PathBuf,
    http: Client<UnixConnector, Full<Bytes>>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IncusClient>();
};

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
        let resp: ApiResponse<Vec<ApiInstance>> = self
            .send_json(Method::GET, "/1.0/instances?recursion=2", None::<&()>)
            .await?;
        let instances = resp.metadata.unwrap_or_default();
        let mut summaries: Vec<InstanceSummary> =
            instances.into_iter().map(InstanceSummary::from).collect();
        summaries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(summaries)
    }

    pub async fn create_instance(
        &self,
        params: CreateInstanceParams<'_>,
    ) -> Result<(), IncusError> {
        validate_name(params.name, "instance name")?;
        let instance_type = match params.instance_type.trim() {
            "" | "container" => "container",
            "virtual-machine" => "virtual-machine",
            other => {
                return Err(IncusError::InvalidInput(format!(
                    "Invalid instance type '{other}': expected container or virtual-machine"
                )));
            }
        };
        let alias = params.image_alias.trim();
        if alias.is_empty() {
            return Err(IncusError::InvalidInput(
                "Image alias cannot be empty".into(),
            ));
        }

        let mut config = HashMap::new();
        let mem = params.memory_limit.trim();
        if !mem.is_empty() {
            config.insert("limits.memory".to_string(), mem.to_string());
        }
        let cpu = params.cpu_limit.trim();
        if !cpu.is_empty() {
            config.insert("limits.cpu".to_string(), cpu.to_string());
        }
        if !params.cloud_init_user_data.trim().is_empty() {
            config.insert(
                "cloud-init.user-data".to_string(),
                params.cloud_init_user_data.to_string(),
            );
        }
        if params.nesting == NestingMode::Enabled {
            config.insert("security.nesting".to_string(), "true".to_string());
        }

        let profiles: Vec<String> = params
            .profiles
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let profiles = if profiles.is_empty() {
            vec!["default".to_string()]
        } else {
            profiles
        };

        let mut devices = HashMap::new();
        let disk = params.disk_limit.trim();
        if !disk.is_empty() {
            let pool = self.resolve_root_pool(&profiles).await;
            let mut root_dev = HashMap::new();
            root_dev.insert("type".to_string(), "disk".to_string());
            root_dev.insert("path".to_string(), "/".to_string());
            root_dev.insert("pool".to_string(), pool);
            root_dev.insert("size".to_string(), disk.to_string());
            devices.insert("root".to_string(), root_dev);
        }
        if let Some(ws_device) = build_workspace_device(params.workspace_path)? {
            devices.insert("workspace".to_string(), ws_device);
        }

        let payload = CreateInstanceRequest {
            name: params.name.to_string(),
            instance_type: instance_type.to_string(),
            architecture: host_incus_arch().to_string(),
            profiles,
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

        self.execute_async(Method::POST, "/1.0/instances", Some(&payload))
            .await
    }

    async fn resolve_root_pool(&self, profile_names: &[String]) -> String {
        if let Ok(resp) = self
            .send_json::<(), Vec<ApiProfile>>(
                Method::GET,
                "/1.0/profiles?recursion=1",
                None::<&()>,
            )
            .await
            && let Some(profiles) = resp.metadata
        {
            for name in profile_names.iter().rev() {
                if let Some(prof) = profiles.iter().find(|p| p.name == *name)
                    && let Some(root_dev) = prof.devices.get("root")
                    && let Some(pool) = root_dev.get("pool").map(|s| s.trim())
                    && !pool.is_empty()
                {
                    return pool.to_string();
                }
            }
        }
        if let Ok(pools) = self.list_storage_pools().await
            && let Some(first) = pools.first()
            && !first.name.trim().is_empty()
        {
            return first.name.trim().to_string();
        }
        "default".to_string()
    }

    pub async fn clone_instance(
        &self,
        source_name: &str,
        new_name: &str,
    ) -> Result<(), IncusError> {
        validate_name(source_name, "source instance name")?;
        validate_name(new_name, "new instance name")?;
        let payload = CloneInstanceRequest {
            name: new_name.to_string(),
            source: CloneSource {
                source_type: "copy".to_string(),
                source: source_name.to_string(),
                instance_only: false,
            },
        };
        self.execute_async(Method::POST, "/1.0/instances", Some(&payload))
            .await
    }

    pub async fn add_disk_mount(
        &self,
        instance: &str,
        dev_name: &str,
        host_path: &str,
        container_path: &str,
    ) -> Result<(), IncusError> {
        validate_name(dev_name, "device name")?;
        let cpath = container_path.trim();
        if cpath.is_empty() || !cpath.starts_with('/') {
            return Err(IncusError::InvalidInput(
                "Container path must be a non-empty absolute path starting with '/'".into(),
            ));
        }
        let dev = build_disk_device(host_path, cpath, "Host path")?;
        let key = dev_name.to_string();
        self.update_instance_devices(instance, move |devices| {
            devices.insert(key, dev);
        })
        .await
    }

    pub async fn add_proxy_device(
        &self,
        instance: &str,
        dev_name: &str,
        host_port: u16,
        container_port: u16,
    ) -> Result<(), IncusError> {
        validate_name(dev_name, "device name")?;
        if host_port == 0 || container_port == 0 {
            return Err(IncusError::InvalidInput(
                "Host port and container port must be greater than 0".into(),
            ));
        }
        let mut dev = HashMap::new();
        dev.insert("type".to_string(), "proxy".to_string());
        dev.insert("listen".to_string(), format!("tcp:127.0.0.1:{host_port}"));
        dev.insert(
            "connect".to_string(),
            format!("tcp:127.0.0.1:{container_port}"),
        );
        let key = dev_name.to_string();
        self.update_instance_devices(instance, move |devices| {
            devices.insert(key, dev);
        })
        .await
    }

    pub async fn remove_device(&self, instance: &str, dev_name: &str) -> Result<(), IncusError> {
        validate_name(dev_name, "device name")?;
        let key = dev_name.to_string();
        self.update_instance_devices(instance, move |devices| {
            devices.remove(&key);
        })
        .await
    }

    pub async fn update_instance_limits(
        &self,
        name: &str,
        cpu_limit: &str,
        memory_limit: &str,
    ) -> Result<(), IncusError> {
        let cpu = cpu_limit.trim().to_string();
        let mem = memory_limit.trim().to_string();
        self.update_instance_put(name, move |put| {
            set_or_remove(&mut put.config, "limits.cpu", &cpu);
            set_or_remove(&mut put.config, "limits.memory", &mem);
        })
        .await
    }

    pub async fn tag_cubby_app(&self, name: &str, slug: &str) -> Result<(), IncusError> {
        let trimmed = slug.trim().to_string();
        self.update_instance_put(name, move |put| {
            set_or_remove(&mut put.config, "user.cubby.app", &trimmed);
        })
        .await
    }

    pub(super) async fn update_instance_devices<F>(
        &self,
        name: &str,
        mutate: F,
    ) -> Result<(), IncusError>
    where
        F: FnOnce(&mut HashMap<String, HashMap<String, String>>),
    {
        self.update_instance_put(name, |put| mutate(&mut put.devices))
            .await
    }

    async fn update_instance_put<F>(&self, name: &str, mutate: F) -> Result<(), IncusError>
    where
        F: FnOnce(&mut InstancePut),
    {
        validate_name(name, "instance name")?;
        let path = format!("/1.0/instances/{name}");
        let resp: ApiResponse<InstancePut> =
            self.send_json(Method::GET, &path, None::<&()>).await?;
        let mut put = resp.metadata.ok_or_else(|| IncusError::Api {
            code: 500,
            message: format!("Missing instance metadata for '{name}'"),
        })?;
        mutate(&mut put);
        self.execute_async(Method::PUT, &path, Some(&put)).await
    }

    pub async fn set_instance_state(&self, name: &str, action: &str) -> Result<(), IncusError> {
        validate_name(name, "instance name")?;
        if !matches!(action, "start" | "stop" | "restart" | "freeze" | "unfreeze") {
            return Err(IncusError::InvalidInput(format!(
                "Invalid state action '{action}': expected start, stop, restart, freeze, or unfreeze"
            )));
        }
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
        self.execute_async(Method::POST, &path, Some(&payload))
            .await
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

    pub async fn list_images(&self) -> Result<Vec<ImageSummary>, IncusError> {
        let resp: ApiResponse<Vec<ApiImage>> = self
            .send_json(Method::GET, "/1.0/images?recursion=1", None::<&()>)
            .await?;
        let mut summaries: Vec<ImageSummary> = resp
            .metadata
            .unwrap_or_default()
            .into_iter()
            .map(ImageSummary::from)
            .collect();
        summaries.sort_by(|a, b| {
            a.alias
                .cmp(&b.alias)
                .then(a.fingerprint.cmp(&b.fingerprint))
        });
        Ok(summaries)
    }

    pub async fn delete_image(&self, fingerprint: &str) -> Result<(), IncusError> {
        let fp = fingerprint.trim();
        if fp.is_empty() || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(IncusError::InvalidInput(
                "Image fingerprint must be a non-empty hexadecimal string".into(),
            ));
        }
        let path = format!("/1.0/images/{fp}");
        self.execute_async(Method::DELETE, &path, None::<&()>).await
    }

    pub async fn list_profiles(&self) -> Result<Vec<ProfileSummary>, IncusError> {
        let resp: ApiResponse<Vec<ApiProfile>> = self
            .send_json(Method::GET, "/1.0/profiles?recursion=1", None::<&()>)
            .await?;
        let mut profiles: Vec<ProfileSummary> = resp
            .metadata
            .unwrap_or_default()
            .into_iter()
            .map(ProfileSummary::from)
            .collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    pub async fn exec_command(
        &self,
        instance: &str,
        command: &[String],
    ) -> Result<ExecOutput, IncusError> {
        validate_name(instance, "instance name")?;
        if command.is_empty() {
            return Err(IncusError::InvalidInput(
                "Exec command cannot be empty".into(),
            ));
        }
        let path = format!("/1.0/instances/{instance}/exec");
        let payload = ExecRequest {
            command: command.to_vec(),
            wait_for_websocket: false,
            record_output: true,
        };
        let op_meta = self
            .execute_async_with_metadata(Method::POST, &path, Some(&payload))
            .await?;

        let inner = op_meta.as_ref().and_then(|m| m.metadata.as_ref());
        let exit_code = inner
            .and_then(|v| v.get("return"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1);

        let stdout_path = inner
            .and_then(|v| v.get("output"))
            .and_then(|v| v.get("1"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let stderr_path = inner
            .and_then(|v| v.get("output"))
            .and_then(|v| v.get("2"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let stdout = match stdout_path {
            Some(p) => self.send_raw_get(p).await?,
            None => String::new(),
        };
        let stderr = match stderr_path {
            Some(p) => self.send_raw_get(p).await?,
            None => String::new(),
        };

        Ok(ExecOutput {
            exit_code,
            stdout,
            stderr,
        })
    }

    pub(super) async fn execute_async<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(), IncusError> {
        self.execute_async_with_metadata(method, path, body).await?;
        Ok(())
    }

    async fn execute_async_with_metadata<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<Option<OperationMetadata>, IncusError> {
        let resp: ApiResponse<OperationMetadata> = self.send_json(method, path, body).await?;
        if resp.response_type == "async" && !resp.operation.is_empty() {
            let wait_path = format!("{}/wait", resp.operation.trim_end_matches('/'));
            let op_resp: ApiResponse<OperationMetadata> =
                self.send_json(Method::GET, &wait_path, None::<&()>).await?;
            if let Some(ref meta) = op_resp.metadata
                && (meta.status_code >= 400 || !meta.err.is_empty())
            {
                let msg = if meta.err.is_empty() {
                    meta.status.clone()
                } else {
                    meta.err.clone()
                };
                return Err(IncusError::Api {
                    code: meta.status_code,
                    message: msg,
                });
            }
            return Ok(op_resp.metadata);
        }
        Ok(resp.metadata)
    }

    async fn send_request(
        &self,
        method: Method,
        path: &str,
        bytes: Bytes,
        headers: &[(&str, &str)],
    ) -> Result<(StatusCode, hyper::HeaderMap, Bytes), IncusError> {
        if !path.starts_with('/') || path.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(IncusError::InvalidInput(format!(
                "Invalid request path: '{path}'"
            )));
        }
        let uri: hyper::Uri = UnixUri::new(&self.socket_path, path).into();
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
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
        let resp_headers = response.headers().clone();
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| IncusError::Transport(e.to_string()))?
            .to_bytes();

        Ok((http_status, resp_headers, body_bytes))
    }

    pub(super) async fn send_raw_get(&self, path: &str) -> Result<String, IncusError> {
        let body_bytes = self
            .send_raw_bytes(Method::GET, path, Bytes::new(), &[])
            .await?;
        Ok(String::from_utf8_lossy(&body_bytes).into_owned())
    }

    pub(super) async fn send_raw_bytes(
        &self,
        method: Method,
        path: &str,
        body: Bytes,
        headers: &[(&str, &str)],
    ) -> Result<Bytes, IncusError> {
        self.send_raw_bytes_with_headers(method, path, body, headers)
            .await
            .map(|(_, b)| b)
    }

    pub(super) async fn send_raw_bytes_with_headers(
        &self,
        method: Method,
        path: &str,
        body: Bytes,
        headers: &[(&str, &str)],
    ) -> Result<(hyper::HeaderMap, Bytes), IncusError> {
        let (http_status, resp_headers, body_bytes) =
            self.send_request(method, path, body, headers).await?;
        if http_status.is_client_error() || http_status.is_server_error() {
            if let Ok(parsed) =
                serde_json::from_slice::<ApiResponse<serde_json::Value>>(&body_bytes)
                && (parsed.response_type == "error"
                    || parsed.error_code != 0
                    || !parsed.error.is_empty())
            {
                return Err(api_error_from_response(http_status, &parsed));
            }
            let message = String::from_utf8_lossy(&body_bytes).trim().to_string();
            let message = if message.is_empty() {
                canonical_reason_or_unknown(http_status.as_u16())
            } else {
                message
            };
            return Err(IncusError::Api {
                code: http_status.as_u16(),
                message,
            });
        }
        Ok((resp_headers, body_bytes))
    }

    pub(super) async fn send_json<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<ApiResponse<T>, IncusError> {
        let (bytes, headers): (Bytes, &[(&str, &str)]) = match body {
            Some(payload) => (
                Bytes::from(serde_json::to_vec(payload)?),
                &[("content-type", "application/json")],
            ),
            None => (Bytes::new(), &[]),
        };
        let (http_status, _, body_bytes) = self.send_request(method, path, bytes, headers).await?;

        let parsed: ApiResponse<T> = serde_json::from_slice(&body_bytes)?;
        if parsed.response_type == "error"
            || http_status.is_client_error()
            || http_status.is_server_error()
        {
            return Err(api_error_from_response(http_status, &parsed));
        }

        Ok(parsed)
    }
}

fn api_error_from_response<T>(http_status: StatusCode, parsed: &ApiResponse<T>) -> IncusError {
    let code = if parsed.error_code != 0 {
        parsed.error_code
    } else {
        http_status.as_u16()
    };
    let message = if !parsed.error.is_empty() {
        parsed.error.clone()
    } else if !parsed.status.is_empty() {
        parsed.status.clone()
    } else {
        canonical_reason_or_unknown(code)
    };
    IncusError::Api { code, message }
}

fn canonical_reason_or_unknown(code: u16) -> String {
    StatusCode::from_u16(code).map_or_else(
        |_| "Unknown error".to_string(),
        |s| s.canonical_reason().unwrap_or("Unknown error").to_string(),
    )
}

fn set_or_remove(map: &mut HashMap<String, String>, key: &str, value: &str) {
    if value.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), value.to_string());
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

pub(super) fn validate_name(value: &str, label: &str) -> Result<(), IncusError> {
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

fn build_workspace_device(
    workspace_path: &str,
) -> Result<Option<HashMap<String, String>>, IncusError> {
    if workspace_path.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(build_disk_device(
        workspace_path,
        "/workspace",
        "Workspace path",
    )?))
}

fn build_disk_device(
    host_path: &str,
    container_path: &str,
    label: &str,
) -> Result<HashMap<String, String>, IncusError> {
    let trimmed = host_path.trim();
    if trimmed.is_empty() {
        return Err(IncusError::InvalidInput(format!("{label} cannot be empty")));
    }
    let resolved = if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| IncusError::InvalidInput("Cannot expand '~': HOME is not set".into()))?;
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(trimmed)
    };
    if !resolved.is_absolute() || !resolved.is_dir() {
        return Err(IncusError::InvalidInput(format!(
            "{label} must be an existing absolute directory: {}",
            resolved.display()
        )));
    }
    let mut device = HashMap::new();
    device.insert("type".to_string(), "disk".to_string());
    device.insert(
        "source".to_string(),
        resolved.to_string_lossy().into_owned(),
    );
    device.insert("path".to_string(), container_path.to_string());
    device.insert("shift".to_string(), "true".to_string());
    Ok(device)
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
pub(super) mod tests {
    use super::*;
    use hyper::Response;
    use hyper::body::Incoming;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};
    use tokio::net::UnixListener;

    pub(in crate::incus) type HeaderList = Vec<(String, String)>;

    pub(in crate::incus) async fn spawn_mock_incus_raw<F>(
        handler: F,
    ) -> (PathBuf, tokio::task::JoinHandle<()>)
    where
        F: Fn(String, String, HeaderList, Vec<u8>) -> (StatusCode, HeaderList, Vec<u8>)
            + Send
            + Sync
            + 'static,
    {
        static NEXT_MOCK_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_MOCK_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("incus_cubby_test_{}_{id}", std::process::id()));
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
                            let uri = req
                                .uri()
                                .path_and_query()
                                .map_or("/", hyper::http::uri::PathAndQuery::as_str)
                                .to_string();
                            let req_headers: HeaderList = req
                                .headers()
                                .iter()
                                .map(|(k, v)| {
                                    (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())
                                })
                                .collect();
                            let body_bytes =
                                req.into_body().collect().await.unwrap().to_bytes().to_vec();
                            let (status, resp_headers, resp_body) =
                                handler(method, uri, req_headers, body_bytes);
                            let mut builder = Response::builder().status(status);
                            for (name, value) in resp_headers {
                                builder = builder.header(name, value);
                            }
                            Ok::<_, Infallible>(
                                builder.body(Full::new(Bytes::from(resp_body))).unwrap(),
                            )
                        }
                    });
                    let _ = http1::Builder::new().serve_connection(io, svc).await;
                });
            }
        });

        (sock_path, handle)
    }

    async fn spawn_mock_incus<F>(handler: F) -> (PathBuf, tokio::task::JoinHandle<()>)
    where
        F: Fn(String, String, String) -> (StatusCode, String) + Send + Sync + 'static,
    {
        spawn_mock_incus_raw(move |method, uri, _headers, body_bytes| {
            let body = String::from_utf8_lossy(&body_bytes).to_string();
            let (status, resp_json) = handler(method, uri, body);
            (
                status,
                vec![("content-type".to_string(), "application/json".to_string())],
                resp_json.into_bytes(),
            )
        })
        .await
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
        let ws_str = ws_dir.to_string_lossy().into_owned();
        client
            .create_instance(CreateInstanceParams {
                name: "box1",
                instance_type: "container",
                image_alias: "debian/12",
                memory_limit: "4GiB",
                cpu_limit: "2",
                disk_limit: "",
                workspace_path: &ws_str,
                profiles: vec!["default".to_string(), "dev".to_string()],
                cloud_init_user_data: "#cloud-config\npackages:\n  - git\n",
                nesting: NestingMode::Disabled,
            })
            .await
            .unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "POST");
        assert_eq!(calls[0].1, "/1.0/instances");
        let sent: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
        assert_eq!(sent["name"], "box1");
        assert_eq!(sent["type"], "container");
        assert_eq!(sent["profiles"], serde_json::json!(["default", "dev"]));
        assert_eq!(sent["config"]["limits.memory"], "4GiB");
        assert_eq!(sent["config"]["limits.cpu"], "2");
        assert_eq!(
            sent["config"]["cloud-init.user-data"],
            "#cloud-config\npackages:\n  - git\n"
        );
        assert!(sent["config"].get("security.nesting").is_none());
        assert_eq!(sent["devices"]["workspace"]["type"], "disk");
        assert_eq!(sent["devices"]["workspace"]["path"], "/workspace");
        assert_eq!(sent["devices"]["workspace"]["shift"], "true");
        assert_eq!(
            sent["source"]["server"],
            "https://images.linuxcontainers.org"
        );
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
        client
            .add_disk_mount("box1", "workspace", &ws_dir, "/workspace")
            .await
            .unwrap();
        client.remove_device("box1", "workspace").await.unwrap();
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

    async fn smoke_instance_exec_files_and_logs(client: &IncusClient, name: &str, ws_str: &str) {
        client
            .set_instance_state(name, "start")
            .await
            .expect("start");

        let exec_out = client
            .exec_command(name, &["sh".into(), "-c".into(), "echo ok-exec".into()])
            .await
            .expect("exec_command");
        assert_eq!(exec_out.exit_code, 0);
        assert!(exec_out.stdout.contains("ok-exec"));

        client
            .add_proxy_device(name, "web", 18080, 8080)
            .await
            .expect("add_proxy_device");

        let after_start = client.list_instances().await.expect("list after start");
        let created = after_start
            .iter()
            .find(|i| i.name == name)
            .expect("created instance present");
        assert!(created.is_running);
        assert_eq!(created.cpu, "1");
        assert_eq!(created.workspace_path, ws_str);
        assert_eq!(created.proxies.len(), 1);

        client
            .remove_device(name, "workspace")
            .await
            .expect("remove workspace");
        client
            .remove_device(name, "web")
            .await
            .expect("remove proxy");

        client
            .update_instance_limits(name, "2", "768MiB")
            .await
            .expect("update_instance_limits");

        client
            .push_instance_file(name, "/tmp/cubby-probe.txt", b"hello-live".to_vec())
            .await
            .expect("push_instance_file");
        let pulled = client
            .pull_instance_file(name, "/tmp/cubby-probe.txt")
            .await
            .expect("pull_instance_file");
        assert_eq!(pulled, b"hello-live");

        let logs = client
            .list_instance_logs(name)
            .await
            .expect("list_instance_logs");
        let first_log = logs.first().expect("expected at least one instance log");
        let _ = client
            .read_instance_log(name, first_log)
            .await
            .expect("read_instance_log");
    }

    async fn smoke_infra_pools_nets_and_profiles(
        client: &IncusClient,
        name: &str,
        vol_name: &str,
        net_name: &str,
        prof_name: &str,
    ) {
        let pools = client
            .list_storage_pools()
            .await
            .expect("list_storage_pools");
        assert!(pools.iter().any(|p| p.name == "default"));
        client
            .create_storage_volume("default", vol_name)
            .await
            .expect("create_storage_volume");
        let vols = client
            .list_storage_volumes("default")
            .await
            .expect("list_storage_volumes");
        assert!(vols.iter().any(|v| v.name == vol_name));
        client
            .attach_custom_volume(name, "cache", "default", vol_name, "/cache")
            .await
            .expect("attach_custom_volume");
        client
            .remove_device(name, "cache")
            .await
            .expect("detach custom volume");
        client
            .delete_storage_volume("default", vol_name)
            .await
            .expect("delete_storage_volume");

        let nets = client.list_networks().await.expect("list_networks");
        assert!(nets.iter().any(|n| n.name == "incusbr0"));
        client
            .create_network(net_name, crate::incus::infra::BridgeNat::Disabled)
            .await
            .expect("create_network");
        client
            .delete_network(net_name)
            .await
            .expect("delete_network");

        client
            .create_profile(prof_name, "smoke profile", "1", "256MiB")
            .await
            .expect("create_profile");
        client
            .update_profile(prof_name, "updated", "2", "512MiB")
            .await
            .expect("update_profile");
        client
            .delete_profile(prof_name)
            .await
            .expect("delete_profile");
    }

    #[tokio::test]
    #[ignore = "requires local running incusd socket at /var/lib/incus/unix.socket"]
    async fn live_incus_full_lifecycle_smoke() {
        let client = IncusClient::default_socket();
        let name = format!("cubby-smoke-{}", std::process::id());
        let clone_name = format!("cubby-clone-{}", std::process::id());
        let vol_name = format!("cubby-vol-{}", std::process::id());
        let net_name = format!("cubby-n{}", std::process::id() % 10000);
        let prof_name = format!("cubby-prof-{}", std::process::id());
        let ws_dir = std::env::temp_dir().join(format!("cubby-ws-{}", std::process::id()));
        std::fs::create_dir_all(&ws_dir).unwrap();
        let ws_str = ws_dir.to_string_lossy().into_owned();

        // Cleanup helper in case a prior run left anything behind.
        let _ = client.set_instance_state(&clone_name, "stop").await;
        let _ = client.delete_instance(&clone_name).await;
        let _ = client.set_instance_state(&name, "stop").await;
        let _ = client.delete_instance(&name).await;
        let _ = client.delete_storage_volume("default", &vol_name).await;
        let _ = client.delete_network(&net_name).await;
        let _ = client.delete_profile(&prof_name).await;

        let initial = client.list_instances().await.expect("list_instances");
        assert!(initial.iter().all(|i| i.name != name));

        let profiles = client.list_profiles().await.expect("list_profiles");
        assert!(profiles.iter().any(|p| p.name == "default"));

        client
            .create_instance(CreateInstanceParams {
                name: &name,
                instance_type: "container",
                image_alias: "alpine/3.24",
                memory_limit: "512MiB",
                cpu_limit: "1",
                disk_limit: "",
                workspace_path: &ws_str,
                profiles: Vec::new(),
                cloud_init_user_data: "",
                nesting: NestingMode::Disabled,
            })
            .await
            .expect("create_instance");

        async {
            let images = client.list_images().await.expect("list_images");
            assert_ne!(images.first(), None);

            smoke_instance_exec_files_and_logs(&client, &name, &ws_str).await;
            smoke_infra_pools_nets_and_profiles(&client, &name, &vol_name, &net_name, &prof_name)
                .await;

            client
                .set_instance_state(&name, "freeze")
                .await
                .expect("freeze");
            client
                .set_instance_state(&name, "unfreeze")
                .await
                .expect("unfreeze");
            client
                .set_instance_state(&name, "restart")
                .await
                .expect("restart");

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
                .clone_instance(&name, &clone_name)
                .await
                .expect("clone_instance");
            client
                .delete_instance(&clone_name)
                .await
                .expect("delete clone");

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

        let _ = client.set_instance_state(&clone_name, "stop").await;
        let _ = client.delete_instance(&clone_name).await;
        let _ = client.set_instance_state(&name, "stop").await;
        client
            .delete_instance(&name)
            .await
            .expect("delete_instance");
        let _ = client.delete_storage_volume("default", &vol_name).await;
        let _ = client.delete_network(&net_name).await;
        let _ = client.delete_profile(&prof_name).await;
        let _ = std::fs::remove_dir_all(&ws_dir);
    }

    #[tokio::test]
    async fn list_instances_parses_frozen_profiles_mounts_and_proxies() {
        let (sock, handle) = spawn_mock_incus(|_, _, _| {
            let json = r#"{
                "type": "sync",
                "status": "Success",
                "status_code": 200,
                "metadata": [{
                    "name": "box-frozen",
                    "status": "Frozen",
                    "architecture": "x86_64",
                    "profiles": ["default", "dev"],
                    "config": { "image.os": "Ubuntu", "image.release": "24.04" },
                    "devices": {
                        "workspace": { "type": "disk", "source": "/home/u/proj", "path": "/workspace" },
                        "data": { "type": "disk", "source": "/mnt/data", "path": "/data" },
                        "web": { "type": "proxy", "listen": "tcp:127.0.0.1:8080", "connect": "tcp:127.0.0.1:3000" },
                        "root": { "type": "disk", "path": "/" }
                    },
                    "snapshots": null
                }]
            }"#;
            (StatusCode::OK, json.to_string())
        })
        .await;

        let client = IncusClient::new(&sock);
        let list = client.list_instances().await.unwrap();
        assert_eq!(list.len(), 1);
        let inst = &list[0];
        assert!(inst.is_frozen);
        assert!(!inst.is_running);
        assert_eq!(inst.image_os, "Ubuntu");
        assert_eq!(inst.profiles, vec!["default", "dev"]);
        assert_eq!(inst.workspace_path, "/home/u/proj");
        assert_eq!(inst.mounts.len(), 2);
        assert_eq!(inst.mounts[0].name, "data");
        assert_eq!(inst.mounts[0].host_path, "/mnt/data");
        assert_eq!(inst.mounts[0].container_path, "/data");
        assert_eq!(inst.mounts[1].name, "workspace");
        assert_eq!(inst.proxies.len(), 1);
        assert_eq!(inst.proxies[0].name, "web");
        assert_eq!(inst.proxies[0].listen, "tcp:127.0.0.1:8080");
        assert_eq!(inst.proxies[0].connect, "tcp:127.0.0.1:3000");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn clone_instance_and_state_validation_work() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            seen_clone
                .lock()
                .unwrap()
                .push((method.clone(), uri.clone(), body));
            if method == "POST" && uri == "/1.0/instances" {
                let resp = r#"{
                    "type": "async",
                    "status": "Operation created",
                    "status_code": 100,
                    "operation": "/1.0/operations/op-clone"
                }"#;
                (StatusCode::ACCEPTED, resp.to_string())
            } else if method == "GET" && uri == "/1.0/operations/op-clone/wait" {
                let resp = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": { "status": "Success", "status_code": 200, "err": "" }
                }"#;
                (StatusCode::OK, resp.to_string())
            } else {
                let ok = r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#;
                (StatusCode::OK, ok.to_string())
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        client.clone_instance("src-box", "dst-box").await.unwrap();
        client
            .set_instance_state("dst-box", "freeze")
            .await
            .unwrap();
        client
            .set_instance_state("dst-box", "unfreeze")
            .await
            .unwrap();
        let invalid = client
            .set_instance_state("dst-box", "explode")
            .await
            .unwrap_err();
        assert!(matches!(invalid, IncusError::InvalidInput(_)));

        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "POST");
        assert_eq!(calls[0].1, "/1.0/instances");
        let sent: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
        assert_eq!(sent["name"], "dst-box");
        assert_eq!(sent["source"]["type"], "copy");
        assert_eq!(sent["source"]["source"], "src-box");
        assert_eq!(sent["source"]["instance_only"], false);
        assert_eq!(calls[1].1, "/1.0/operations/op-clone/wait");
        assert_eq!(calls[2].1, "/1.0/instances/dst-box/state");
        assert!(calls[2].2.contains(r#""action":"freeze""#));
        assert!(calls[3].2.contains(r#""action":"unfreeze""#));

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn device_mount_proxy_and_remove_update_instance_devices() {
        let state_devices = Arc::new(Mutex::new(serde_json::json!({
            "workspace": { "type": "disk", "source": "/tmp", "path": "/workspace", "shift": "true" }
        })));
        let state_clone = Arc::clone(&state_devices);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            assert_eq!(uri, "/1.0/instances/box1");
            if method == "GET" {
                let devs = state_clone.lock().unwrap().clone();
                let resp = serde_json::json!({
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": {
                        "architecture": "x86_64",
                        "config": {},
                        "description": "",
                        "devices": devs,
                        "ephemeral": false,
                        "profiles": ["default"],
                        "stateful": false
                    }
                });
                (StatusCode::OK, resp.to_string())
            } else if method == "PUT" {
                let put: serde_json::Value = serde_json::from_str(&body).unwrap();
                *state_clone.lock().unwrap() = put["devices"].clone();
                (
                    StatusCode::OK,
                    r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#
                        .to_string(),
                )
            } else {
                (StatusCode::METHOD_NOT_ALLOWED, "{}".to_string())
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        let host_dir = sock.parent().unwrap().to_string_lossy().into_owned();
        client
            .add_disk_mount("box1", "extra", &host_dir, "/mnt/extra")
            .await
            .unwrap();
        client
            .add_proxy_device("box1", "web", 8080, 3000)
            .await
            .unwrap();

        {
            let devs = state_devices.lock().unwrap();
            assert_eq!(devs["extra"]["type"], "disk");
            assert_eq!(devs["extra"]["source"], host_dir);
            assert_eq!(devs["extra"]["path"], "/mnt/extra");
            assert_eq!(devs["extra"]["shift"], "true");
            assert_eq!(devs["web"]["type"], "proxy");
            assert_eq!(devs["web"]["listen"], "tcp:127.0.0.1:8080");
            assert_eq!(devs["web"]["connect"], "tcp:127.0.0.1:3000");
        }

        client.remove_device("box1", "web").await.unwrap();
        {
            let devs = state_devices.lock().unwrap();
            assert!(devs.get("web").is_none());
            assert!(devs.get("extra").is_some());
        }

        assert!(matches!(
            client
                .add_disk_mount("box1", "bad", &host_dir, "relative")
                .await,
            Err(IncusError::InvalidInput(_))
        ));
        assert!(matches!(
            client.add_proxy_device("box1", "badp", 0, 80).await,
            Err(IncusError::InvalidInput(_))
        ));

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn images_and_profiles_endpoints_work() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, _| {
            seen_clone.lock().unwrap().push((method.clone(), uri.clone()));
            if method == "GET" && uri == "/1.0/images?recursion=1" {
                let json = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": [{
                        "fingerprint": "abcdef123456",
                        "aliases": [{"name": "debian/12"}],
                        "properties": {"description": "Debian bookworm amd64", "os": "Debian", "release": "bookworm"},
                        "architecture": "x86_64",
                        "size": 104857600,
                        "update_source": {"alias": "debian/12"}
                    }]
                }"#;
                (StatusCode::OK, json.to_string())
            } else if method == "DELETE" && uri == "/1.0/images/abcdef123456" {
                let json = r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#;
                (StatusCode::OK, json.to_string())
            } else if method == "GET" && uri == "/1.0/profiles?recursion=1" {
                let json = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": [
                        {"name": "gui", "description": "GUI profile"},
                        {"name": "default", "description": "Default Incus profile"}
                    ]
                }"#;
                (StatusCode::OK, json.to_string())
            } else {
                (StatusCode::NOT_FOUND, r#"{"type":"error","error":"unexpected"}"#.to_string())
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        let images = client.list_images().await.unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].fingerprint, "abcdef123456");
        assert_eq!(images[0].alias, "debian/12");
        assert_eq!(images[0].description, "Debian bookworm amd64");
        assert_eq!(images[0].architecture, "x86_64");
        assert_eq!(images[0].size_bytes, 104_857_600);
        assert_eq!(images[0].size_display, "100 MiB");

        client.delete_image("abcdef123456").await.unwrap();
        assert!(matches!(
            client.delete_image("not-hex!").await,
            Err(IncusError::InvalidInput(_))
        ));

        let profiles = client.list_profiles().await.unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "default");
        assert_eq!(profiles[1].name, "gui");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn exec_command_waits_and_fetches_stdout_stderr() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            seen_clone
                .lock()
                .unwrap()
                .push((method.clone(), uri.clone(), body));
            if method == "POST" && uri == "/1.0/instances/box1/exec" {
                let resp = r#"{
                    "type": "async",
                    "status": "Operation created",
                    "status_code": 100,
                    "operation": "/1.0/operations/op-exec-1"
                }"#;
                (StatusCode::ACCEPTED, resp.to_string())
            } else if method == "GET" && uri == "/1.0/operations/op-exec-1/wait" {
                let resp = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": {
                        "status": "Success",
                        "status_code": 200,
                        "err": "",
                        "metadata": {
                            "return": 0,
                            "output": {
                                "1": "/1.0/operations/op-exec-1/logs/exec_1.stdout",
                                "2": "/1.0/operations/op-exec-1/logs/exec_1.stderr"
                            }
                        }
                    }
                }"#;
                (StatusCode::OK, resp.to_string())
            } else if method == "GET" && uri == "/1.0/operations/op-exec-1/logs/exec_1.stdout" {
                (StatusCode::OK, "hello stdout\n".to_string())
            } else if method == "GET" && uri == "/1.0/operations/op-exec-1/logs/exec_1.stderr" {
                (StatusCode::OK, "minor warn\n".to_string())
            } else {
                (
                    StatusCode::NOT_FOUND,
                    r#"{"type":"error","error":"unexpected"}"#.to_string(),
                )
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        let cmd = vec!["echo".to_string(), "hi".to_string()];
        let out = client.exec_command("box1", &cmd).await.unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, "hello stdout\n");
        assert_eq!(out.stderr, "minor warn\n");

        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "POST");
        assert_eq!(calls[0].1, "/1.0/instances/box1/exec");
        let sent: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
        assert_eq!(sent["command"], serde_json::json!(["echo", "hi"]));
        assert_eq!(sent["wait-for-websocket"], false);
        assert_eq!(sent["record-output"], true);

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn list_instances_parses_telemetry_and_instance_type() {
        let (sock, handle) = spawn_mock_incus(|_, _, _| {
            let json = r#"{
                "type": "sync",
                "status": "Success",
                "status_code": 200,
                "metadata": [
                    {
                        "name": "vm-box",
                        "type": "virtual-machine",
                        "status": "Running",
                        "architecture": "x86_64",
                        "config": { "limits.cpu": "4", "limits.memory": "8GiB" },
                        "state": {
                            "cpu": { "usage": 12500000000 },
                            "memory": { "usage": 536870912 },
                            "disk": {
                                "root": { "usage": 2147483648, "total": 10737418240 }
                            },
                            "network": {
                                "lo": {
                                    "addresses": [{"family": "inet", "address": "127.0.0.1", "scope": "local"}],
                                    "counters": { "bytes_received": 999999999, "bytes_sent": 999999999 }
                                },
                                "enp5s0": {
                                    "addresses": [{"family": "inet", "address": "10.0.0.12", "scope": "global"}],
                                    "counters": { "bytes_received": 10485760, "bytes_sent": 5242880 }
                                },
                                "eth1": {
                                    "addresses": [],
                                    "counters": { "bytes_received": 5242880, "bytes_sent": 1048576 }
                                }
                            },
                            "processes": 42
                        }
                    },
                    {
                        "name": "ct-stopped",
                        "status": "Stopped",
                        "architecture": "x86_64",
                        "config": {},
                        "state": null
                    }
                ]
            }"#;
            (StatusCode::OK, json.to_string())
        })
        .await;

        let client = IncusClient::new(&sock);
        let list = client.list_instances().await.unwrap();
        assert_eq!(list.len(), 2);

        let ct = &list[0];
        assert_eq!(ct.name, "ct-stopped");
        assert_eq!(ct.instance_type, "container");
        assert_eq!(ct.cpu_usage_display, "-");
        assert_eq!(ct.disk_usage_display, "-");
        assert_eq!(ct.net_rx_display, "-");
        assert_eq!(ct.net_tx_display, "-");
        assert_eq!(ct.processes_display, "-");

        let vm = &list[1];
        assert_eq!(vm.name, "vm-box");
        assert_eq!(vm.instance_type, "virtual-machine");
        assert_eq!(vm.cpu_usage_display, "12.50s");
        assert_eq!(vm.disk_usage_display, "2.00 GiB");
        assert_eq!(vm.net_rx_display, "15 MiB");
        assert_eq!(vm.net_tx_display, "6 MiB");
        assert_eq!(vm.processes_display, "42");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn create_vm_instance_and_reject_invalid_instance_type() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            if method == "POST" && uri == "/1.0/instances" {
                seen_clone.lock().unwrap().push(body);
                (
                    StatusCode::OK,
                    r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#
                        .to_string(),
                )
            } else {
                (
                    StatusCode::NOT_FOUND,
                    r#"{"type":"error","error":"unexpected"}"#.to_string(),
                )
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        client
            .create_instance(CreateInstanceParams {
                name: "vm1",
                instance_type: "virtual-machine",
                image_alias: "ubuntu/24.04",
                memory_limit: "4GiB",
                cpu_limit: "2",
                disk_limit: "",
                workspace_path: "",
                profiles: Vec::new(),
                cloud_init_user_data: "",
                nesting: NestingMode::Disabled,
            })
            .await
            .unwrap();

        let err = client
            .create_instance(CreateInstanceParams {
                name: "bad-type",
                instance_type: "hypervisor",
                image_alias: "ubuntu/24.04",
                memory_limit: "",
                cpu_limit: "",
                disk_limit: "",
                workspace_path: "",
                profiles: Vec::new(),
                cloud_init_user_data: "",
                nesting: NestingMode::Disabled,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, IncusError::InvalidInput(_)));

        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let sent: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(sent["name"], "vm1");
        assert_eq!(sent["type"], "virtual-machine");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn update_instance_limits_sets_and_clears_config_keys() {
        let config_state = Arc::new(Mutex::new(serde_json::json!({
            "image.os": "Debian"
        })));
        let config_clone = Arc::clone(&config_state);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            assert_eq!(uri, "/1.0/instances/box1");
            if method == "GET" {
                let cfg = config_clone.lock().unwrap().clone();
                let resp = serde_json::json!({
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": {
                        "architecture": "x86_64",
                        "config": cfg,
                        "description": "",
                        "devices": {},
                        "ephemeral": false,
                        "profiles": ["default"],
                        "stateful": false
                    }
                });
                (StatusCode::OK, resp.to_string())
            } else if method == "PUT" {
                let put: serde_json::Value = serde_json::from_str(&body).unwrap();
                *config_clone.lock().unwrap() = put["config"].clone();
                (
                    StatusCode::OK,
                    r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#
                        .to_string(),
                )
            } else {
                (StatusCode::METHOD_NOT_ALLOWED, "{}".to_string())
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        client
            .update_instance_limits("box1", "4", "8GiB")
            .await
            .unwrap();
        {
            let cfg = config_state.lock().unwrap();
            assert_eq!(cfg["limits.cpu"], "4");
            assert_eq!(cfg["limits.memory"], "8GiB");
            assert_eq!(cfg["image.os"], "Debian");
        }

        client
            .update_instance_limits("box1", "   ", "")
            .await
            .unwrap();
        {
            let cfg = config_state.lock().unwrap();
            assert!(cfg.get("limits.cpu").is_none());
            assert!(cfg.get("limits.memory").is_none());
            assert_eq!(cfg["image.os"], "Debian");
        }

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn send_request_rejects_invalid_uri_paths_without_panicking() {
        let client = IncusClient::new("/tmp/unused_incus_cubby_test.socket");
        for bad_path in ["bad path", "/1.0/logs/foo bar", "/1.0/logs/foo\nbar"] {
            let err = client
                .send_request(Method::GET, bad_path, Bytes::new(), &[])
                .await
                .unwrap_err();
            assert!(
                matches!(err, IncusError::InvalidInput(_)),
                "expected InvalidInput for '{bad_path}', got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn create_instance_sets_security_nesting_when_enabled() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            if method == "POST" && uri == "/1.0/instances" {
                seen_clone.lock().unwrap().push(body);
                (
                    StatusCode::OK,
                    r#"{"type":"sync","status":"Success","status_code":200,"metadata":{}}"#
                        .to_string(),
                )
            } else {
                (
                    StatusCode::NOT_FOUND,
                    r#"{"type":"error","error":"unexpected"}"#.to_string(),
                )
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        client
            .create_instance(CreateInstanceParams {
                name: "nested-box",
                instance_type: "container",
                image_alias: "debian/12",
                memory_limit: "",
                cpu_limit: "",
                disk_limit: "",
                workspace_path: "",
                profiles: Vec::new(),
                cloud_init_user_data: "",
                nesting: NestingMode::Enabled,
            })
            .await
            .unwrap();

        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let sent: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(sent["config"]["security.nesting"], "true");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }

    #[tokio::test]
    async fn create_instance_sets_root_disk_size_with_resolved_pool() {
        let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let seen_clone = Arc::clone(&seen);

        let (sock, handle) = spawn_mock_incus(move |method, uri, body| {
            seen_clone.lock().unwrap().push((method.clone(), uri.clone(), body));
            if method == "GET" && uri == "/1.0/profiles?recursion=1" {
                let resp = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": [
                        {
                            "name": "default",
                            "description": "Default profile",
                            "config": {},
                            "devices": {
                                "root": { "type": "disk", "path": "/", "pool": "nvme-pool" }
                            }
                        }
                    ]
                }"#;
                (StatusCode::OK, resp.to_string())
            } else if method == "POST" && uri == "/1.0/instances" {
                let resp = r#"{
                    "type": "sync",
                    "status": "Success",
                    "status_code": 200,
                    "metadata": {}
                }"#;
                (StatusCode::OK, resp.to_string())
            } else {
                (
                    StatusCode::NOT_FOUND,
                    r#"{"type":"error","error":"unexpected"}"#.to_string(),
                )
            }
        })
        .await;

        let client = IncusClient::new(&sock);
        client
            .create_instance(CreateInstanceParams {
                name: "sized-box",
                instance_type: "container",
                image_alias: "debian/13",
                memory_limit: "2GiB",
                cpu_limit: "2",
                disk_limit: "8GiB",
                workspace_path: "",
                profiles: vec!["default".to_string()],
                cloud_init_user_data: "",
                nesting: NestingMode::Disabled,
            })
            .await
            .unwrap();

        let calls = seen.lock().unwrap();
        let create_call = calls
            .iter()
            .find(|(m, u, _)| m == "POST" && u == "/1.0/instances")
            .expect("POST /1.0/instances");
        let sent: serde_json::Value = serde_json::from_str(&create_call.2).unwrap();
        assert_eq!(sent["devices"]["root"]["type"], "disk");
        assert_eq!(sent["devices"]["root"]["path"], "/");
        assert_eq!(sent["devices"]["root"]["pool"], "nvme-pool");
        assert_eq!(sent["devices"]["root"]["size"], "8GiB");

        handle.abort();
        let _ = std::fs::remove_dir_all(sock.parent().unwrap());
    }
}
