# IncusCubby: Incus REST API & Simplestreams Reference

IncusCubby communicates with the local Incus daemon over **HTTP/1.1 on Unix domain socket `/var/lib/incus/unix.socket`** (`DEFAULT_INCUS_SOCKET`) using `hyper` + `hyperlocal`. No TCP listener or TLS client certificate is used.

---

## 1. Transport & Operation Model

JSON payloads use `content-type: application/json`, and responses deserialize into `ApiResponse<T>` (`type`, `status`, `operation`, `error_code`, `error`, `metadata`).

When a mutating endpoint returns `type == "async"` with a non-empty `operation` path (for example `/1.0/operations/<uuid>`), `execute_async_with_metadata` issues `GET {operation}/wait` and maps `status_code >= 400` or a non-empty `err` to `IncusError::Api`.

Go nil slices and maps arrive as JSON `null` (`config`, `devices`, `profiles`, `addresses`, `disk`) and decode through `null_to_default` into empty Rust collections.

`PATCH /1.0/instances/{name}` rejects `null` entries when removing devices, so IncusCubby `GET`s the full `InstancePut` or `ProfilePut`, mutates it in memory, and `PUT`s it back.

---

## 2. Incus 1.0 Endpoints Used

### Instances, Lifecycle & Telemetry (`src/incus/client.rs`)

| Method & Path | Sync/Async | Purpose |
|---|---|---|
| `GET /1.0/instances?recursion=2` | Sync | List all instances with config, devices, live state, network counters, and snapshots (`InstanceSummary`). |
| `POST /1.0/instances` | Async | Create `container` or `virtual-machine` (`protocol: "simplestreams"`, `mode: "pull"`, server `https://images.linuxcontainers.org`), or CoW clone (`source.type: "copy"`, `instance_only: false`). |
| `GET /1.0/instances/{name}` | Sync | Fetch `InstancePut` before updating limits or devices. |
| `PUT /1.0/instances/{name}` | Async | Apply updated `InstancePut` (limits, disk mounts, proxy devices) or restore a snapshot (`{"restore": "<snapshot>"}`). |
| `PUT /1.0/instances/{name}/state` | Async | Run `start`, `stop`, `restart`, `freeze`, or `unfreeze` (`timeout: 30`, `force: false`, `stateful: false`). |
| `DELETE /1.0/instances/{name}` | Async | Delete an instance. |

### Snapshots, Exec, Logs & Files (`src/incus/client.rs`, `src/incus/infra.rs`)

| Method & Path | Sync/Async | Purpose |
|---|---|---|
| `POST /1.0/instances/{name}/snapshots` | Async | Create stateless snapshot (`stateful: false`). |
| `DELETE /1.0/instances/{name}/snapshots/{snap}` | Async | Delete snapshot. |
| `POST /1.0/instances/{name}/exec` | Async | Execute non-interactive command (`wait-for-websocket: false`, `record-output: true`) and read exit code from `metadata.return`. |
| `GET {stdout_path}` / `GET {stderr_path}` | Sync | Fetch recorded command output URLs returned in `metadata.output["1"]` and `["2"]`. |
| `GET /1.0/instances/{name}/logs` | Sync | List available log paths and extract basenames. |
| `GET /1.0/instances/{name}/logs/{log}` | Sync | Read raw log text (filename restricted to `[A-Za-z0-9._-]`, rejecting `.` and `..`). |
| `POST /1.0/instances/{name}/files?path=...` | Sync | Push host file bytes with `content-type: application/octet-stream`, `X-Incus-type: file`, `X-Incus-mode: 0644`. |
| `GET /1.0/instances/{name}/files?path=...` | Sync | Pull file bytes; rejects responses with header `x-incus-type: directory`. |

### Images, Profiles, Storage & Networks (`src/incus/client.rs`, `src/incus/infra.rs`)

| Method & Path | Sync/Async | Purpose |
|---|---|---|
| `GET /1.0/images?recursion=1` | Sync | List locally cached images (`ImageSummary`). |
| `DELETE /1.0/images/{fingerprint}` | Async | Delete cached image by hexadecimal fingerprint. |
| `GET /1.0/profiles?recursion=1` | Sync | List profiles (`ProfileSummary`). |
| `POST /1.0/profiles` | Async | Create profile with description, `limits.cpu`, `limits.memory`. |
| `GET /1.0/profiles/{name}` + `PUT /1.0/profiles/{name}` | Async | Read-modify-write `ProfilePut` preserving unrelated `config` and `devices`. |
| `DELETE /1.0/profiles/{name}` | Async | Delete non-`default` profile. |
| `GET /1.0/storage-pools?recursion=1` | Sync | List storage pools (`StoragePoolSummary`). |
| `GET /1.0/storage-pools/{pool}/resources` | Sync | Read `space.used` and `space.total` per pool. |
| `GET /1.0/storage-pools/{pool}/volumes/custom?recursion=1` | Sync | List custom volumes in a pool. |
| `POST /1.0/storage-pools/{pool}/volumes/custom` | Async | Create custom filesystem volume (`type: "custom"`, `content_type: "filesystem"`). |
| `DELETE /1.0/storage-pools/{pool}/volumes/custom/{name}` | Async | Delete custom volume. |
| `GET /1.0/networks?recursion=1` | Sync | List networks (`type`, `managed`, `ipv4.address`, `ipv4.nat`). |
| `GET /1.0/networks/{name}/leases` | Sync | Fetch DHCP leases for managed networks. |
| `POST /1.0/networks` | Async | Create `bridge` network with `ipv4.nat` and `ipv6.nat` (`"true"`/`"false"`). |
| `DELETE /1.0/networks/{name}` | Async | Delete network. |

### Host Resources (`src/incus/resources.rs`)

| Method & Path | Sync/Async | Purpose |
|---|---|---|
| `GET /1.0/resources` | Sync | Host CPU (sockets, total threads), load averages (`Average1Min`/`5Min`/`10Min`, capitalized keys), memory used/total, and system vendor/product for the persistent resource bar and popup (`HostResources`). Polled every 10 s alongside `list_storage_pools`. |

---

## 3. Remote Simplestreams Index (`src/incus/remote_images.rs`)

`default_preset_images()`, which seeds the Create tab at startup, carries eight presets: `debian/12`, `debian/13`, `ubuntu/24.04`, `ubuntu/26.04`, `alpine/3.24`, `archlinux`, `fedora/44`, and `opensuse/tumbleweed`. `default_preset_aliases()` returns the first seven, and the unit test `default_presets_match_expected_list` pins exactly that list. Either way the presets work offline, so the Create tab lists images without network access.

The *Fetch remote aliases* control issues an HTTPS `GET` with a 10-second `reqwest` timeout over `rustls`:

```
https://images.linuxcontainers.org/streams/v1/images.json
```

Alias filtering maps `std::env::consts::ARCH`: `x86_64` to `amd64`, `aarch64` to `arm64`, `arm` to `armhf`, and `riscv64` to `riscv64`. It picks the shortest alias from the comma-separated `aliases` value, prefers non-`tinycloud` aliases, and prefers `variant == "default"` when aliases collide.

---

## 4. Device Schemas Built by IncusCubby

- **Host workspace / disk bind-mount**:
  ```json
  { "type": "disk", "source": "/abs/host/dir", "path": "/workspace", "shift": "true" }
  ```
- **Custom storage volume mount**:
  ```json
  { "type": "disk", "pool": "<pool>", "source": "<volume>", "path": "/container/path" }
  ```
- **Proxy port forward**:
  ```json
  { "type": "proxy", "listen": "tcp:127.0.0.1:<host_port>", "connect": "tcp:127.0.0.1:<container_port>" }
  ```
