# #0005 ui.on_open_url rejects all URLs not starting with IncusCubby repo URL, blocking Community Scripts links

- 2026-11-21T14:23:47Z `issue`: ui.on_open_url rejects all URLs not starting with IncusCubby repo URL, blocking Community Scripts links [src/main.rs:2411]
- 2026-11-21T14:25:21Z `fix`: Replaced IncusCubby URL prefix check in ui.on_open_url with is_safe_external_url allowing validated http/https URLs via xdg-open; cargo test 56 passed, clippy clean [src/main.rs:2409]
