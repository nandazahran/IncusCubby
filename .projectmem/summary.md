# projectmem - IncusCubby

_Last updated: 2026-11-18_

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Recent issues
- No issues logged yet.

## Decisions
- No decisions logged yet.

## Notes
- Initial commit
- New feature: Add Incus sandbox manager GUI and Unix socket client [build.rs]
- gotcha: Incus REST API serializes Go nil slices/maps (config, addresses) as JSON null; use null_to_default deserializer and i64 for memory usage [src/incus/models.rs:4]
- gotcha: Incus PATCH /1.0/instances/{name} rejects null device entries ("type <nil> is not supported in *api.DevicesMap"); use GET + PUT InstancePut to remove devices [src/incus/client.rs:140]
- New feature: Add workspace mounts, live polling, codium launcher, and delete confirmations [src/incus/client.rs]
- New feature: Add DistroShelf/BoxBuddyRS features: clone, freeze/restart, exec packages, mounts/proxies, image cache, and settings [src/incus/client.rs]
- New feature: Add Incus Web UI features, harden Rust idioms, and enable clippy pedantic lints [src/incus/client.rs]
- Added IncusCubby documentation suite modeled on KineAnchor: README.md, CONTRIBUTING.md, docs/ARCHITECTURE.md, docs/INCUS_API.md, docs/ROADMAP.md, and docs/PRIVACY.md
- New feature: Add documentation suite for IncusCubby [CONTRIBUTING.md]
- gotcha: community-scripts/Incus only mirrors stable ProxmoxVE scripts; dev/new apps like surfsense and t3code live in community-scripts/DevScripts and are listed on community-scripts.org [src/incus/community.rs:1]

## Key files
- `.gitattributes`
- `.gitignore`
- `LICENSE`
- `README.md`
- `build.rs`
- `Cargo.toml`
- `src/incus/client.rs`
- `src/incus/mod.rs`
- `src/incus/models.rs`
- `src/launcher.rs`
- `src/main.rs`
- `ui/app.slint`
- `.projectmem/PROJECT_MAP.md`
- `.projectmem/plan.md`
- `/1.0`
- `api.DevicesMap`
- `.projectmem/summary.md`
- `src/incus/packages.rs`
- `src/incus/remote_images.rs`
- `src/settings.rs`

## Open questions
- None logged yet.
