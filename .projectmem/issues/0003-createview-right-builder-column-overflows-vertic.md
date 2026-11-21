# #0003 CreateView right builder column overflows vertically and pushes 52px bottom Deploy/Create bar under CubbyResourceBar

- 2026-11-21T13:16:23Z `issue`: CreateView right builder column overflows vertically and pushes 52px bottom Deploy/Create bar under CubbyResourceBar [ui/advanced.slint:1988]
- 2026-11-21T13:18:44Z `fix`: Wrapped CreateView right builder body in ScrollView with min-height: 0px, set explicit CubbyTextArea heights, pinned 52px bottom action bar; cargo test 51 passed, clippy clean [ui/advanced.slint:1992]
