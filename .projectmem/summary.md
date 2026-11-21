# projectmem - tilefish

_Last updated: 2026-11-21_

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Recent issues
- [DONE] #0005 ui.on_open_url rejects all URLs not starting with IncusCubby repo URL, blocking Community Scripts links [src/main.rs:2411] -> Replaced IncusCubby URL prefix check in ui.on_open_url with is_safe_external_url allowing validated http/https URLs via xdg-open; cargo test 56 passed, clippy clean [src/main.rs:2409] (fixed)
- [DONE] #0004 parse_ct_metadata captures $(msg_menu ...) subshell expression into var_os when ct script offers Debian/Alpine prompt [src/incus/community.rs:872] -> Validated var_os/var_version tokens in parse_ct_metadata, captured alpine branch defaults, normalized install profile aliases, added clickable OS profile selection; cargo test 55 passed, clippy clean [src/incus/community.rs:942] (fixed)
- [DONE] #0003 CreateView right builder column overflows vertically and pushes 52px bottom Deploy/Create bar under CubbyResourceBar [ui/advanced.slint:1988] -> Wrapped CreateView right builder body in ScrollView with min-height: 0px, set explicit CubbyTextArea heights, pinned 52px bottom action bar; cargo test 51 passed, clippy clean [ui/advanced.slint:1992] (fixed)
- [DONE] #0002 CubbyStatusDot in CubbyResourceBar and popup header shifted down ~7px by y: (parent.height - self.preferred-height) / 2 inside stretched HorizontalLayout [ui/components.slint:1419] -> Removed y: (parent.height - self.preferred-height) / 2 from CubbyStatusDot in CubbyResourceBar and popup header; cargo test 51 passed, clippy clean [ui/components.slint:1419] (fixed)
- [DONE] #0001 3-column CubbyCatalogCard items have non-uniform widths due to intrinsic text/tag preferred-width in HorizontalLayout [ui/components.slint:449] -> Set min-width: 0px and preferred-width: 0px on CubbyCatalogCard and grid cell wrappers in CreateView, plus min-width: 0px on card title Text; cargo test 50 passed, 3 ignored [ui/components.slint:451] (fixed)

## Decisions
- No decisions logged yet.

## Notes
- gotcha: Slint 1.18 lowers `visible` to a Clip wrapper; layout still reserves space for hidden elements, including repeated `for` children. Never use `for` + `visible` to render one-of-N content inside a layout; bind a single resolved value instead (see official-image detail overflow). [ui/advanced.slint:2051]
- New feature: feat(create): unify catalog cards and fix selected-alias overflow [src/main.rs]
- New feature: feat(create): add Community Scripts notes, details, profiles, sliders, and builder scroll [src/incus/community.rs]
- New feature: feat(create): add storage limit, base OS switcher, reactive profiles, and unslop UI copy [src/incus/client.rs]

## Key files
- `1.18`
- `src/main.rs`
- `ui/advanced.slint`
- `ui/app.slint`
- `ui/components.slint`
- `parent.height`
- `self.preferred`
- `ui.on`
- `src/incus/community.rs`
- `ui/theme.slint`
- `src/incus/client.rs`
- `src/incus/models.rs`
- `ui/simple.slint`

## Open questions
- None logged yet.
