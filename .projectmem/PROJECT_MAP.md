# Project Map - IncusCubby

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Structure
- [Cargo.toml](file:///home/nanda/Dokumen/GitHub/IncusCubby/Cargo.toml) — Rust 2024 manifest (`slint`, `tokio`, `hyper`, `hyperlocal`, `serde`, `thiserror`, `anyhow`)
- [build.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/build.rs) — compiles Slint UI markup
- [ui/app.slint](file:///home/nanda/Dokumen/GitHub/IncusCubby/ui/app.slint) — standard-widget GUI (`AppWindow`, instance list, details, create form, snapshots)
- [src/main.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/main.rs) — Slint main thread + background Tokio worker + `upgrade_in_event_loop` state updates
- [src/incus/mod.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/mod.rs) — Incus client module exports
- [src/incus/models.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/models.rs) — Incus 1.0 REST API request/response structs and UI summary mapping
- [src/incus/client.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/incus/client.rs) — `hyperlocal` Unix socket client, async operation waiter, `PermissionDenied`/`NotFound` error classification
- [src/launcher.rs](file:///home/nanda/Dokumen/GitHub/IncusCubby/src/launcher.rs) — host terminal (`incus exec <name> -- su -l root`) and external IDE (`code` / `zed`) launchers

## Relationships
- `src/main.rs` sends commands over a Tokio unbounded channel to the background Tokio thread, which calls `IncusClient` and pushes UI updates back via `ui_weak.upgrade_in_event_loop`.
- `src/incus/client.rs` uses `hyperlocal::Uri::new("/var/lib/incus/unix.socket", ...)` exclusively for all Incus state and lifecycle operations.
