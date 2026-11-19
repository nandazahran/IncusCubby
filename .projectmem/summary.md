# projectmem - IncusCubby

_Last updated: 2026-11-19_

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Recent issues
- [DONE] #0001 CI Rust 1.100.0 clippy failed on Option::map().unwrap_or_default() and assert_is_empty in tests [src/settings.rs:39] -> Used Option::map_or_default and assert_ne! / Vec::first; CI run 44296315315 passed all checks [src/settings.rs:39] (fixed)

## Decisions
- Use custom-styled Slint Precision Instrument components (Geist/Geist Mono fonts, Dark Graphite & Light Studio themes, Simple & Advanced modes) directly implementing the Doop design system without std-widgets styling lock-in [ui/app.slint]
- Use Linux-only GitHub Actions CI (with non-code paths-ignore and cargo-deny) and x86_64/aarch64 AppImage+tarball release workflow modeled on KineAnchor [.github/workflows/]
- Standardize UI iconography on single-pack Lucide monochrome SVGs (CubbyIcon, CubbyIconButton, CubbyActionButton) across Doop design system and future Slint UI to replace wordy utility and row-action buttons [ui/components.slint]
- Fetch community-scripts app logos dynamically from community-scripts.org logo URLs with lazy ListView loading and XDG disk cache instead of bundling third-party app SVGs [src/logos.rs]

## Notes
- gotcha: community-scripts/Incus only mirrors stable ProxmoxVE scripts; dev/new apps like surfsense and t3code live in community-scripts/DevScripts and are listed on community-scripts.org [src/incus/community.rs:1]
- New feature: Add About & Legal tab and set license to GPL-3.0-or-later [src/main.rs]
- Implemented Doop Precision Instrument UI in Slint across ui/theme.slint, ui/components.slint, ui/advanced.slint, ui/simple.slint, and ui/app.slint with bundled Geist/Geist Mono TTFs and 18 local distro/app SVG icons [ui/theme.slint]
- gotcha: IncusCubby uses let_chains (Rust 1.88+), cmake (aws-lc-sys), libfontconfig1-dev (yeslogic-fontconfig-sys), and Slint GPL-3.0-only in deny.toml [.github/workflows/ci.yml]
- New feature: Add GitHub Actions CI/CD, Linux packaging, and cargo-deny gate [packaging/linux/build-appimage.sh]
- Published v0.1.0 release (run 44298031256) with x86_64/aarch64 AppImage, tar.gz, and SHA256SUMS.txt; updated PKGBUILD sha256sums [.github/workflows/release.yml]
- Added private .deb and .rpm GitHub release packaging scripts (dpkg-deb and rpmbuild) without publishing to external repos or stores [packaging/linux/build-deb.sh]
- New feature: Add .deb and .rpm packaging to GitHub release workflow [packaging/linux/build-deb.sh]
- Ported 56-icon Lucide SVG system (ISC) into Slint desktop app: ui/icons/lucide/*.svg, CubbyTheme.ui_icon, CubbyIcon/CubbyIconButton/CubbyActionButton, and full sweeps of ui/simple.slint and ui/advanced.slint; verified with cargo test, clippy -D warnings, and cargo fmt --check [ui/theme.slint]
- Published v0.1.1 release (run 44426078199) with x86_64/aarch64 AppImage, tar.gz, deb, rpm, and SHA256SUMS.txt; updated PKGBUILD sha256sums in f8df7e4 [.github/workflows/release.yml]

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
