# Project Map - IncusCubby

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Structure
- [Cargo.toml](file:///home/nanda/Dokumen/GitHub/IncusCubby/Cargo.toml) — Rust 2024 manifest (`slint`, `tokio`, `hyper`, `hyperlocal`, `reqwest`, `serde`, `thiserror`, `anyhow`)
- [build.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/build.rs) — compiles Slint UI markup
- [ui/app.slint](file:///home/nanda/Dokumen/GitHub/IncusCubby/ui/app.slint) — standard-widget `TabWidget` GUI (Sandboxes, Create Sandbox, Images, Settings, inline confirmations & socket warning)
- [src/main.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/main.rs) — Slint main thread + background Tokio worker (3s polling, async REST commands, `upgrade_in_event_loop`)
- [src/incus/mod.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/mod.rs) — Incus module exports
- [src/incus/models.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/models.rs) — Incus 1.0 REST API request/response structs and UI summary mapping
- [src/incus/client.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/client.rs) — `hyperlocal` Unix socket client (instances, clone, state, devices/proxies, snapshots, images, profiles, exec)
- [src/incus/remote_images.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/remote_images.rs) — `reqwest` simplestreams alias fetcher for `images.linuxcontainers.org`
- [src/incus/packages.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/packages.rs) — OS-aware package upgrade/install command builders with token validation
- [src/settings.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/settings.rs) — XDG JSON preferences (`preferred_terminal`, `preferred_ide`) and PATH detection
- [src/launcher.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/launcher.rs) — host terminal (`incus exec <name> -- su -l root`) and external IDE (`codium`/`code`, `zed`/`zeditor`) launchers

## Relationships
- `src/main.rs` routes UI actions to the background Tokio thread (`mpsc::unbounded_channel`) and updates Slint strictly via `ui_weak.upgrade_in_event_loop`.
- `src/incus/client.rs` uses `hyperlocal::Uri::new("/var/lib/incus/unix.socket", ...)` exclusively for all Incus state, lifecycle, device, snapshot, image, profile, and exec operations.
