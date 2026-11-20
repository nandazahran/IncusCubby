# IncusCubby: Architecture

Native Linux desktop GUI for managing local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`. License: GPL-3.0-or-later (`LICENSE`).

---

## 1. Tech validation

### 1.1 Incus control path: Direct Unix socket REST vs CLI wrapping

| Criterion | Direct REST (`hyperlocal` + `hyper`) | Shelling out to `incus` CLI |
|---|---|---|
| Structured data & telemetry | Typed JSON structs from `/1.0/instances?recursion=2`, `/1.0/networks/*/leases`, `/1.0/storage-pools/*/resources`. | Requires parsing JSON/table CLI output per subcommand and spawning subprocesses every 3 s. |
| Error handling & diagnostics | Inspects `io::ErrorKind` (`PermissionDenied` / `NotFound`) and Incus API error codes directly. | Relies on exit codes and localized stderr text. |
| Interactive root shell | Not applicable over simple JSON REST without websocket PTY piping. | `incus exec <name> -- su -l root` inside the user's terminal emulator works natively. |

Decision:
- Use **`IncusClient`** (`hyper` 1.x + `hyper-util` + `hyperlocal` 0.9) against `/var/lib/incus/unix.socket` for every state query, lifecycle mutation, device/limit update, snapshot, file transfer, log fetch, non-interactive `exec`, image, profile, storage, and network operation.
- Shell out to `incus` **only** in `launcher::open_shell` when launching an interactive terminal window (`incus exec <name> -- su -l root`).

### 1.2 UI toolkit: Custom Precision Instrument Slint UI

- Slint `1.18` compiles ahead of time through `build.rs` (`slint_build::compile("ui/app.slint")`).
- Uses custom Precision Instrument components (`ui/theme.slint`, `ui/components.slint`, `ui/simple.slint`, `ui/advanced.slint`, `ui/app.slint`) with bundled Geist/Geist Mono TTF fonts (`ui/fonts/`), unified Lucide SVG icon set (`ui/icons/lucide/`), Simple/Advanced modes, and Dark Graphite / Light Studio themes.
- Slint runs its event loop on the main thread; all async Incus I/O runs on a dedicated OS thread hosting a multi-threaded Tokio runtime, pushing updates strictly through `slint::Weak<AppWindow>::upgrade_in_event_loop`.

---

## 2. System architecture

```mermaid
flowchart LR
  subgraph UI["Main Thread (Slint AppWindow)"]
    W["ui/app.slint + simple/advanced"]
    CB["wire_ui_callbacks + pending_confirm"]
    L["launcher.rs (Terminal / IDE)"]
    S["settings.rs (settings.json)"]
    W --> CB
    CB --> L
    CB --> S
  end

  subgraph Worker["Background Thread (Tokio Runtime)"]
    MPSC["unbounded_channel<BackendCommand>"]
    POLL["3 s Interval Poll (list_instances)"]
    CMD["handle_command"]
    C["IncusClient (hyperlocal)"]
    RI["remote_images (reqwest)"]
    CS["community (reqwest)"]
    LG["logos (reqwest + XDG cache)"]
    MPSC --> CMD
    POLL --> C
    CMD --> C
    CMD --> RI
    CMD --> CS
  end

  CB -->|send BackendCommand| MPSC
  CMD -->|upgrade_in_event_loop| W
  POLL -->|upgrade_in_event_loop when !busy| W
  LG -->|upgrade_in_event_loop| W
  C <-->|HTTP/1.1 Unix Socket| SOCK["/var/lib/incus/unix.socket"]
  RI -->|HTTPS GET| SS["images.linuxcontainers.org"]
  CS -->|HTTPS GET| GH["community-scripts / GitHub"]
  LG -->|HTTPS GET| CDN["cdn.jsdelivr.net / catalog logos"]
```

### 2.1 Repository layout

```
Cargo.toml                    # single crate ("incus-cubby"), edition 2024, pedantic clippy lints
build.rs                      # slint_build::compile("ui/app.slint")
deny.toml                     # cargo-deny GPLv3-compatible license, ban, and source gate
.github/
  workflows/{ci,release}.yml  # Linux CI and tagged x86_64/aarch64 GitHub release pipelines
  scripts/release-notes.py    # commit-categorized GitHub release notes generator
packaging/linux/              # .desktop, 512px PNG icon, AppImage/.deb/.rpm builders, PKGBUILD
ui/
  app.slint                   # AppWindow shell, data structs, confirmation & warning banners
  theme.slint                 # CubbyTheme tokens, Dark/Light palettes, fonts & icon helpers
  components.slint            # shared Precision Instrument widgets
  simple.slint                # Simple mode hub & quick-create flow
  advanced.slint              # Advanced mode views (sandboxes, create, infra, profiles, images, settings/about/legal)
  fonts/                      # bundled Geist & Geist Mono TTFs + OFL.txt
  icons/                      # CubbyMark, app icon, fallback SVGs, and lucide/ SVG set
src/
  main.rs                     # entrypoint, Tokio worker, polling loop, command dispatch, UI state sync
  launcher.rs                 # terminal emulator detection (incus exec) & brand-agnostic IDE launch
  logos.rs                    # async logo worker, XDG disk cache, SVG/raster decode, accent extraction
  settings.rs                 # XDG JSON config (terminal, IDE, simple_mode, light_theme) & PATH scanning
  incus/
    mod.rs                    # re-exports
    client.rs                 # IncusClient, HTTP/Unix transport, instances, devices, snapshots, exec
    community.rs              # community-scripts catalog fetcher, ct/ parser & container runner builder
    infra.rs                  # storage pools/volumes, networks/leases, profiles, logs, file push/pull
    models.rs                 # Incus 1.0 REST serde types + InstanceSummary/ImageSummary mappings
    packages.rs               # distro package upgrade/install argv builders + token validation
    remote_images.rs          # preset aliases + simplestreams JSON index fetcher/parser
```

### 2.2 Incus 1.0 REST client & gotchas

Incus returns `"config": null`, `"devices": null`, `"profiles": null`, or `"addresses": null` for Go nil maps and slices. `models.rs` applies `null_to_default` through `#[serde(default, deserialize_with = "null_to_default")]` so `null` decodes to an empty `Vec` or `HashMap`. Memory fields use `i64` before clamping to `u64`.

`PATCH /1.0/instances/{name}` rejects `null` entries in `devices` (`"type <nil> is not supported in *api.DevicesMap"`). `update_instance_put` and `infra.rs::update_profile` therefore `GET` the full `InstancePut` or `ProfilePut`, mutate it in memory while preserving unrelated `config`, `devices`, `profiles`, `description`, `ephemeral`, and `stateful`, then `PUT` the whole object.

Endpoints that return `type: "async"` with an `operation` path are awaited through `GET {operation}/wait` in `execute_async_with_metadata`, which raises `IncusError::Api` when `metadata.err` is set or `metadata.status_code >= 400`.

`build_disk_device` expands `~/...` against `$HOME`, checks that the host path is an existing absolute directory, and builds a `"type": "disk"` device with `"shift": "true"` so unprivileged containers map UIDs.

### 2.3 UI & background worker model

`spawn_backend_worker` runs `tokio::select!` between `rx.recv()` (`BackendCommand`) and a three-second `tokio::time::interval` with `MissedTickBehavior::Delay`.

Poll results are compared against `last_instances` and `last_socket_err`, and reach the UI through `upgrade_in_event_loop` only when `!ui.get_busy()`, so a poll cannot clobber an in-flight action.

`DeleteInstance`, `RestoreSnapshot`, `DeleteSnapshot`, `DeleteStorageVolume`, `DeleteNetwork`, `DeleteProfile`, and `DeleteImage` park the pending `BackendCommand` in `Rc<RefCell<Option<BackendCommand>>>` and show the inline `Confirm action` banner instead of a modal dialog.

`classify_transport_error` walks the `StdError::source()` chain for `io::ErrorKind::PermissionDenied` or `NotFound`, maps the result to `IncusError::SocketUnavailable`, and shows the `Socket unavailable` banner with the `sudo usermod -aG incus-admin $USER` hint.

### 2.4 Security & validation boundaries

`validate_name` restricts instance, snapshot, device, profile, pool, volume, and network names to non-empty ASCII `[A-Za-z0-9_-]`. `delete_image` requires a non-empty hex fingerprint, and `delete_profile` refuses to delete `"default"`.

`read_instance_log` rejects `.`, `..`, and any character outside `[A-Za-z0-9._-]`.

`send_request` rejects paths that do not start with `/` or that contain control or whitespace characters. File transfer paths are percent-encoded by `percent_encode` in `infra.rs`.

`packages::validate_package_token` rejects empty tokens, leading `-` flags, and any character outside `[A-Za-z0-9_.-+]`. Package names reach the process as direct argv entries, or as `"$@"` positional arguments in `sh -lc`, so no shell interpolation occurs.

---

## 3. Incus REST API & Simplestreams

All endpoints, headers, and simplestreams parsing rules are documented in [INCUS_API.md](INCUS_API.md).

---

## 4. Linux integration

The Incus socket path is hardcoded to `/var/lib/incus/unix.socket` (`DEFAULT_INCUS_SOCKET`). It requires `incusd` running and your user in `incus-admin` or root.

Preferences are stored as pretty JSON in `$XDG_CONFIG_HOME/incus-cubby/settings.json` (fallback `$HOME/.config/incus-cubby/settings.json`) with `preferred_terminal`, `preferred_ide`, `simple_mode`, and `light_theme`. Dynamic OS and app logos are cached in `$XDG_CACHE_HOME/incus-cubby/logos/` (fallback `$HOME/.cache/incus-cubby/logos/`).

Terminal resolution tries `preferred_terminal` unless it is `"auto"`, then `$TERMINAL`, then the first of `x-terminal-emulator`, `gnome-terminal`, `konsole`, `ghostty`, `alacritty`, `kitty`, `foot`, `wezterm`, `xfce4-terminal`, and `xterm` found in `PATH`. Argument flags adapt per binary: `gnome-terminal --`, `wezterm start --`, `foot` with a bare command, and `-e` for the rest.

IDE resolution tries `preferred_ide` unless it is `"auto"` (any `PATH` command or absolute path), then the first of `codium`, `code`, `zed`, `zeditor`, `kate`, `subl`, `hx`, `nvim`, and `emacs` found in `PATH`. It passes the instance's `workspace` mount source path when one exists.

---

## 5. Licensing

- Application: GNU General Public License v3.0 or later (`GPL-3.0-or-later`, `LICENSE`).
- Strict Rust lint posture enforced in `Cargo.toml`: `unsafe_code = "warn"`, `clippy::all = "warn"`, `clippy::pedantic = "warn"`.
