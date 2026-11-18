# #0001 CI Rust 1.100.0 clippy failed on Option::map().unwrap_or_default() and assert_is_empty in tests

- 2026-11-18T11:36:35Z `issue`: CI Rust 1.100.0 clippy failed on Option::map().unwrap_or_default() and assert_is_empty in tests [src/settings.rs:39]
- 2026-11-18T11:46:52Z `fix`: Used Option::map_or_default and assert_ne! / Vec::first; CI run 44296315315 passed all checks [src/settings.rs:39]
