# #0004 parse_ct_metadata captures $(msg_menu ...) subshell expression into var_os when ct script offers Debian/Alpine prompt

- 2026-11-21T14:11:31Z `issue`: parse_ct_metadata captures $(msg_menu ...) subshell expression into var_os when ct script offers Debian/Alpine prompt [src/incus/community.rs:872]
- 2026-11-21T14:14:56Z `fix`: Validated var_os/var_version tokens in parse_ct_metadata, captured alpine branch defaults, normalized install profile aliases, added clickable OS profile selection; cargo test 55 passed, clippy clean [src/incus/community.rs:942]
