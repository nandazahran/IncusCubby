# Contributing to IncusCubby

IncusCubby is a single-crate Rust + Slint desktop application for Linux. This file covers building from source, running tests against the mock and live Incus sockets, and the checks expected before opening a pull request.

## Prerequisites

- **Rust 1.85 or newer** (`edition = "2024"`), with `clippy` and `rustfmt`.
- **Linux desktop libraries** required by Slint's default backend (`winit` / `skia` or `femtovg` / fontconfig, X11/Wayland development headers depending on your distro).
- **Incus** (runtime and optional live integration test):
  - `incusd` running with `/var/lib/incus/unix.socket`.
  - User added to `incus-admin`: `sudo usermod -aG incus-admin "$USER"`.
  - `incus` CLI in `PATH` if testing the interactive *Shell* launcher.

## Build and run

```bash
# Debug build and run
cargo run

# Release build and run
cargo run --release
```

`build.rs` compiles `ui/app.slint` automatically during `cargo build` / `cargo run`.

## Repository Layout

- [`Cargo.toml`](Cargo.toml): Crate manifest (`incus-cubby`), dependencies, and strict `clippy::pedantic` + `unsafe_code = "warn"` lint configuration.
- [`build.rs`](build.rs): Compiles `ui/app.slint` via `slint_build::compile`.
- [`ui/app.slint`](ui/app.slint): Slint `std-widgets` GUI (`AppWindow` with 6 top-level tabs, inline socket warning, and inline destructive-action confirmation banner).
- [`src/main.rs`](src/main.rs): Slint main event loop, `BackendCommand` mpsc dispatch, 3-second polling Tokio worker, and `upgrade_in_event_loop` UI state synchronization.
- [`src/incus/mod.rs`](src/incus/mod.rs): Module exports for the Incus client, models, infra, packages, and remote image fetcher.
- [`src/incus/client.rs`](src/incus/client.rs): `IncusClient` (`hyper` + `hyperlocal` over `/var/lib/incus/unix.socket`), instance lifecycle, CoW clone, limits, disk/proxy devices, snapshots, images, profiles, exec, and mock-socket unit tests.
- [`src/incus/infra.rs`](src/incus/infra.rs): Storage pools, custom volumes, networks & DHCP leases, profile CRUD, instance log listing/reading, and file push/pull over `/1.0/instances/{name}/files`.
- [`src/incus/models.rs`](src/incus/models.rs): Incus 1.0 REST API request/response structs, `null_to_default` deserialization helper, and UI summary mapping (`InstanceSummary`, `ImageSummary`, `ProfileSummary`).
- [`src/incus/packages.rs`](src/incus/packages.rs): OS-aware upgrade/install argv builders (`apt-get`, `apk`, `pacman`, `dnf`, `zypper`) with strict package token validation.
- [`src/incus/remote_images.rs`](src/incus/remote_images.rs): Preset aliases and `reqwest` simplestreams index parser for `https://images.linuxcontainers.org/streams/v1/images.json`.
- [`src/settings.rs`](src/settings.rs): XDG JSON persistence (`$XDG_CONFIG_HOME/incus-cubby/settings.json` or `~/.config/incus-cubby/settings.json`) and `PATH` discovery for terminals and IDEs.
- [`src/launcher.rs`](src/launcher.rs): External terminal (`incus exec <name> -- su -l root`) and IDE (`codium`/`code`, `zed`/`zeditor`) process launchers.

## Checks

Run formatting, clippy, and the unit/mock-socket test suite before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

`Cargo.toml` enables `clippy::all` and `clippy::pedantic` as warnings and sets `unsafe_code = "warn"`, so `-D warnings` treats any lint regression as a hard failure.

### Live Incus smoke test

`src/incus/client.rs` includes an `#[ignore]` end-to-end test (`live_incus_full_lifecycle_smoke`) that exercises instance creation, exec, devices, limits, file transfer, logs, storage volumes, networks, profiles, freeze/restart, snapshots, clone, and cleanup against `/var/lib/incus/unix.socket`:

```bash
cargo test --locked live_incus_full_lifecycle_smoke -- --ignored --nocapture
```

Requires a working local `incusd` with internet access to pull the base image.

## Documentation

- [Architecture Specification](docs/ARCHITECTURE.md)
- [Incus REST API & Simplestreams Reference](docs/INCUS_API.md)
- [Roadmap & Risk Matrix](docs/ROADMAP.md)
- [Privacy Policy](docs/PRIVACY.md)
