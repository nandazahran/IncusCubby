# IncusCubby — Roadmap, Risks, and Decisions

Companion to [ARCHITECTURE.md](ARCHITECTURE.md). API details: [INCUS_API.md](INCUS_API.md).

Observed dev environment: Linux desktop with `rustc 1.98.0`, `edition = "2024"`, Slint `1.18`, local `incusd` over `/var/lib/incus/unix.socket`.

---

## Design decisions

> [!NOTE]
> IncusCubby follows a local-first, single-binary design using custom Precision Instrument Slint components and direct Unix socket REST calls. No background daemon of its own or external database is introduced; Incus itself remains the single source of truth.

- **Direct Unix socket REST over CLI wrapping**: All state queries and lifecycle actions go through `hyperlocal` -> `/var/lib/incus/unix.socket` (`IncusClient`). Only interactive terminal attachment shells out (`incus exec <name> -- su -l root`).
- **`GET` + `PUT InstancePut` over `PATCH`**: Incus rejects `null` device entries in `PATCH /1.0/instances/{name}`, so device removal, disk/proxy attachment, and live limit edits read-modify-write the full `InstancePut` (and similarly `ProfilePut` for profiles).
- **Custom Precision Instrument Slint UI**: Styled components (`ui/theme.slint`, `ui/components.slint`, `ui/simple.slint`, `ui/advanced.slint`) with Geist/Geist Mono fonts, Lucide icons, Simple/Advanced modes, and Dark Graphite / Light Studio themes.
- **ID-mapped bind mounts (`shift: "true"`)**: Host workspace mounts default to `shift: "true"` at `/workspace` so unprivileged containers can read/write host project files without manual UID remapping.

---

## Risk list

| # | Risk | Impact | Mitigation |
|---|---|---|---|
| R1 | User not in `incus-admin` or `incusd` stopped | All API calls fail with `PermissionDenied` or `NotFound` | `classify_transport_error` maps both to `IncusError::SocketUnavailable` and displays the inline `Incus Socket Warning` banner with `sudo usermod -aG incus-admin $USER`. |
| R2 | Incus Go `nil` slices/maps serialize as JSON `null` | `serde_json` deserialization failure on empty `config`, `devices`, `profiles`, `addresses` | `null_to_default` deserializer on all nullable collections; `i64` for signed/negative memory fields. |
| R3 | `PATCH` device removal rejected by Incus | Unable to detach mounts or proxy devices | Always use `GET` + `PUT` with `InstancePut` / `ProfilePut`, preserving unrelated config and devices. |
| R4 | Host filesystem lacks ID-mapped mount (`shift: "true"`) support | Mount attachment fails on older kernels/filesystems | Surface exact `IncusError::Api` message in the status bar; custom storage volumes remain available as an alternative. |
| R5 | Cloud-init ignored on non-`/cloud` images | `cloud-init.user-data` config stored on instance but never executed | Create form explicitly labels cloud-init as `"Optional Cloud-Init User Data (for /cloud images)"`. |
| R6 | Package manager differs or OS metadata absent | Upgrade/install action cannot infer command | `packages.rs` covers Debian/Ubuntu, Alpine, Arch, Fedora/Alma/Rocky/CentOS, and openSUSE; unsupported OS values return a clear error without executing anything. |
| R7 | 3-second polling races with user action | UI refreshes overwrite busy indicator or stale state | Polling updates `AppWindow` via `upgrade_in_event_loop` only when `!ui.get_busy()` and state changed. |
| R8 | Accidental destructive click (delete instance/snapshot/network/volume/profile/image) | Irreversible data loss | All destructive actions gate behind `pending_confirm` and the inline `Confirm Action` banner; deleting the `"default"` profile is blocked in both UI and client. |
| R9 | Remote simplestreams fetch fails offline or times out | Cannot load full upstream alias list | 10-second `reqwest` timeout; 7 offline preset aliases (`debian/12`, `debian/13`, `ubuntu/24.04`, `ubuntu/26.04`, `alpine/3.24`, `archlinux`, `fedora/44`) remain available. |

---

## Phased milestones

```mermaid
flowchart LR
  P0["P0 Scaffold"] --> P1["P1 Core REST & UI"]
  P1 --> P2["P2 Launchers & Mounts"]
  P2 --> P3["P3 DistroShelf/BoxBuddyRS Parity"]
  P3 --> P4["P4 Incus Web UI & Clippy Hardening"]
  P4 --> P5["P5 UI Overhaul, Packaging & CI"]
```

### P0 — Scaffold (Done)
- Rust `2024` Cargo manifest, Slint `build.rs`, GPL-3.0-or-later `LICENSE`, `Cargo.lock` committed.

### P1 — Core REST client & Slint UI (Done)
- `IncusClient` over `/var/lib/incus/unix.socket` (`list_instances`, `create_instance`, `set_instance_state`, `delete_instance`, `create_snapshot`, `restore_snapshot`, `delete_snapshot`).
- Slint `TabWidget` UI + background Tokio worker with `upgrade_in_event_loop`.

### P2 — Launchers, workspace mounts & polling (Done)
- Terminal launcher (`incus exec <name> -- su -l root`) and IDE launchers (`codium`/`zed`).
- 3-second live polling, `shift: "true"` workspace bind-mounts, custom CPU/image creation options, inline delete/restore confirmations.

### P3 — DistroShelf / BoxBuddyRS feature parity (Done)
- CoW clone, `restart`/`freeze`/`unfreeze`, REST `exec` OS-aware package upgrade & install, profiles + `cloud-init.user-data`, multi-mounts + `proxy` port forwards, local image cache + `images.linuxcontainers.org` simplestreams alias fetch, XDG JSON terminal/IDE settings persistence.

### P4 — Incus Web UI local parity & Rust hardening (Done)
- Live telemetry (CPU seconds, root disk, network RX/TX, process count) + live CPU/memory limits editor.
- Storage pools + custom volume CRUD/attach, networks + DHCP leases + NAT bridge creation, profile CRUD, instance logs viewer, file push/pull, container/VM type toggle.
- Strict `clippy::pedantic` + `unsafe_code = "warn"` lints enabled cleanly across the crate.

### P5 — UI Overhaul, Packaging & CI
- Custom Precision Instrument Slint UI (`ui/theme.slint`, `ui/components.slint`, `ui/simple.slint`, `ui/advanced.slint`), Lucide icons, Simple/Advanced modes, and dynamic OS/app logo fetching (`src/logos.rs`) implemented.
- Distro packaging (`.desktop`, AppImage via `build-appimage.sh`, `.deb` via `build-deb.sh`, `.rpm` via `build-rpm.sh`, local `PKGBUILD`), `deny.toml` license gate, `.github/scripts/release-notes.py`, and GitHub Actions CI + tagged release workflows implemented.
- Configurable Incus socket path / remote contexts remains deferred beyond local `/var/lib/incus/unix.socket`.

---

## Test plan

| Scope | Automated | Manual |
|---|---|---|
| Core & Client | `cargo test --locked` (47 passing tests, 3 ignored across `client.rs`, `infra.rs`, `community.rs`, `packages.rs`, `remote_images.rs`, `logos.rs`, `settings.rs` using mock `hyper` Unix sockets and pure unit tests). | — |
| Lint & Format | `cargo fmt --all --check` and `cargo clippy --all-targets --locked -- -D warnings`. | — |
| Live Incus E2E | `cargo test --locked live_incus_full_lifecycle_smoke -- --ignored --nocapture` against `/var/lib/incus/unix.socket`. | Verify UI tabs, inline socket warning (`incusd` stopped or non-`incus-admin` user), terminal/IDE spawning, and VM vs container creation on desktop. |
