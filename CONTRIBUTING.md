# Contributing to IncusCubby

IncusCubby is a single-crate Rust + Slint desktop application for Linux. This file covers building from source, running tests against the mock and live Incus sockets, and the checks expected before opening a pull request.

## Prerequisites

- **Rust 1.91 or newer** (`edition = "2024"`), with `clippy` and `rustfmt`.
- **Linux desktop build dependencies** (Debian/Ubuntu): `pkg-config`, `cmake`, `libfontconfig1-dev`, `libwayland-dev`, `libxkbcommon-dev`, `libx11-dev`, `libx11-xcb-dev`, `libxcursor-dev`, `libxrandr-dev`, `libxi-dev`, `libgl1-mesa-dev`.
- **AppImage packaging**: `appimagetool` (run with `APPIMAGE_EXTRACT_AND_RUN=1`), `desktop-file-utils`, `file`.
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
- [`ui/app.slint`](ui/app.slint): Precision Instrument Slint `AppWindow` shell dispatching between [`ui/simple.slint`](ui/simple.slint) and [`ui/advanced.slint`](ui/advanced.slint) using [`ui/theme.slint`](ui/theme.slint) and [`ui/components.slint`](ui/components.slint).
- [`src/main.rs`](src/main.rs): Slint main event loop, `BackendCommand` mpsc dispatch, 3-second polling Tokio worker, and `upgrade_in_event_loop` UI state synchronization.
- [`src/incus/mod.rs`](src/incus/mod.rs): Module exports for the Incus client, models, infra, packages, community scripts, and remote image fetcher.
- [`src/incus/client.rs`](src/incus/client.rs): `IncusClient` (`hyper` + `hyperlocal` over `/var/lib/incus/unix.socket`), instance lifecycle, CoW clone, limits, disk/proxy devices, snapshots, images, profiles, exec, and mock-socket unit tests.
- [`src/incus/community.rs`](src/incus/community.rs): `community-scripts/Incus` catalog fetcher, `ct/` metadata parser, and REST-API container runner builder.
- [`src/incus/infra.rs`](src/incus/infra.rs): Storage pools, custom volumes, networks & DHCP leases, profile CRUD, instance log listing/reading, and file push/pull over `/1.0/instances/{name}/files`.
- [`src/incus/models.rs`](src/incus/models.rs): Incus 1.0 REST API request/response structs, `null_to_default` deserialization helper, and UI summary mapping (`InstanceSummary`, `ImageSummary`, `ProfileSummary`).
- [`src/incus/packages.rs`](src/incus/packages.rs): OS-aware upgrade/install argv builders (`apt-get`, `apk`, `pacman`, `dnf`, `zypper`) with strict package token validation.
- [`src/incus/remote_images.rs`](src/incus/remote_images.rs): Preset aliases and `reqwest` simplestreams index parser for `https://images.linuxcontainers.org/streams/v1/images.json`.
- [`src/logos.rs`](src/logos.rs): Async distro and community-app logo fetcher, `$XDG_CACHE_HOME/incus-cubby/logos/` cache, image decoder, and badge accent extractor.
- [`src/settings.rs`](src/settings.rs): XDG JSON preferences (`preferred_terminal`, `preferred_ide`, `simple_mode`, `light_theme`) in `$XDG_CONFIG_HOME/incus-cubby/settings.json` (or `~/.config/incus-cubby/settings.json`) and `PATH` discovery.
- [`src/launcher.rs`](src/launcher.rs): External terminal (`incus exec <name> -- su -l root`) and brand-agnostic IDE launcher (auto-detected editors or user-configured command).
- [`packaging/linux/`](packaging/linux): `.desktop` entry, 512x512 PNG icon, AppImage/`.deb`/`.rpm` builders (`build-appimage.sh`, `build-deb.sh`, `build-rpm.sh`), and local Arch/CachyOS `PKGBUILD`.
- [`.github/scripts/release-notes.py`](.github/scripts/release-notes.py): Commit-categorized GitHub release notes generator.
- [`deny.toml`](deny.toml): `cargo-deny` GPLv3-compatible dependency license, ban, and source policy.

## Checks

CI runs formatting, clippy, unit/mock-socket tests, packaging validation, and `cargo-deny` license checks on Linux (non-code commits are skipped via `paths-ignore`):

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check licenses bans sources
desktop-file-validate packaging/linux/io.github.nandazahran.incuscubby.desktop
bash -n packaging/linux/build-appimage.sh
bash -n packaging/linux/build-deb.sh
bash -n packaging/linux/build-rpm.sh
python3 .github/scripts/release-notes.py --self-test
python3 .github/scripts/release-notes.py --tag HEAD --repo nandazahran/IncusCubby >/dev/null
```

`Cargo.toml` enables `clippy::all` and `clippy::pedantic` as warnings and sets `unsafe_code = "warn"`, so `-D warnings` treats any lint regression as a hard failure.

## Packaging

```bash
# After cargo build --locked --release
./packaging/linux/build-appimage.sh x86_64 target/release/incus-cubby \
  incus-cubby-<version>-x86_64.AppImage
./packaging/linux/build-deb.sh x86_64 target/release/incus-cubby \
  incus-cubby_<version>_amd64.deb
./packaging/linux/build-rpm.sh x86_64 target/release/incus-cubby \
  incus-cubby-<version>-1.x86_64.rpm
```

Local Arch/CachyOS packaging is defined in [`packaging/linux/PKGBUILD`](packaging/linux/PKGBUILD). Tagged `v*` releases run [`.github/workflows/release.yml`](.github/workflows/release.yml) across `x86_64` and `aarch64`, verifying that the tag matches `Cargo.toml` before attaching `.tar.gz`, `.AppImage`, `.deb`, `.rpm`, and `SHA256SUMS.txt` to the GitHub release.

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
