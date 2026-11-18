# projectmem - IncusCubby

_Last updated: 2026-11-18_

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Recent issues
- [DONE] #0001 CI Rust 1.100.0 clippy failed on Option::map().unwrap_or_default() and assert_is_empty in tests [src/settings.rs:39] -> Used Option::map_or_default and assert_ne! / Vec::first; CI run 44296315315 passed all checks [src/settings.rs:39] (fixed)

## Decisions
- Use custom-styled Slint Precision Instrument components (Geist/Geist Mono fonts, Dark Graphite & Light Studio themes, Simple & Advanced modes) directly implementing the Doop design system without std-widgets styling lock-in [ui/app.slint]
- Use Linux-only GitHub Actions CI (with non-code paths-ignore and cargo-deny) and x86_64/aarch64 AppImage+tarball release workflow modeled on KineAnchor [.github/workflows/]

## Notes
- New feature: Add workspace mounts, live polling, codium launcher, and delete confirmations [src/incus/client.rs]
- New feature: Add DistroShelf/BoxBuddyRS features: clone, freeze/restart, exec packages, mounts/proxies, image cache, and settings [src/incus/client.rs]
- New feature: Add Incus Web UI features, harden Rust idioms, and enable clippy pedantic lints [src/incus/client.rs]
- Added IncusCubby documentation suite modeled on KineAnchor: README.md, CONTRIBUTING.md, docs/ARCHITECTURE.md, docs/INCUS_API.md, docs/ROADMAP.md, and docs/PRIVACY.md
- New feature: Add documentation suite for IncusCubby [CONTRIBUTING.md]
- gotcha: community-scripts/Incus only mirrors stable ProxmoxVE scripts; dev/new apps like surfsense and t3code live in community-scripts/DevScripts and are listed on community-scripts.org [src/incus/community.rs:1]
- New feature: Add About & Legal tab and set license to GPL-3.0-or-later [src/main.rs]
- Implemented Doop Precision Instrument UI in Slint across ui/theme.slint, ui/components.slint, ui/advanced.slint, ui/simple.slint, and ui/app.slint with bundled Geist/Geist Mono TTFs and 18 local distro/app SVG icons [ui/theme.slint]
- gotcha: IncusCubby uses let_chains (Rust 1.88+), cmake (aws-lc-sys), libfontconfig1-dev (yeslogic-fontconfig-sys), and Slint GPL-3.0-only in deny.toml [.github/workflows/ci.yml]
- New feature: Add GitHub Actions CI/CD, Linux packaging, and cargo-deny gate [packaging/linux/build-appimage.sh]

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
