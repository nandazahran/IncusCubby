# projectmem - Collapsible-sidebar

_Last updated: 2026-11-21_

## Project purpose
Local native Linux desktop GUI (Rust + Slint) to manage local Incus/LXC developer sandboxes over `/var/lib/incus/unix.socket`.

## Recent issues
- [DONE] #0001 Nav sidebar toggle did not react immediately because clicked callback only invoked persistence round-trip [ui/advanced.slint:65] -> Direct sidebar_collapsed assignment in CubbyTopBar.clicked before persistence callback; verified synthetic Slint click collapsed=true, capture rail 70px physical = 56 logical vs expanded 260 = 208, settings seed true relaunch collapsed on startup. [ui/advanced.slint:65] (fixed)

## Decisions
- Nav rail drag resizes between 56px (icon-only floor, not hidden) and 320px; labels hide under 160px. Top-bar button remains separate and toggles full hide (0px) while preserving the dragged width. [ui/advanced.slint:3948]

## Notes
- gotcha: Xwayland/KDE session rejects XTest motion + XSendEvent clicks; /dev/uinput not writable, no sudo. For GUI verification force X11 (WAYLAND_DISPLAY=) for capture, and drive Slint clicks via Window::dispatch_event before ui.run(); capture scale is 1.25. [ui/advanced.slint:65]

## Key files
- `ui.run`
- `1.25`
- `CubbyTopBar.clicked`

## Open questions
- None logged yet.
