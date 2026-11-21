# #0002 CubbyStatusDot in CubbyResourceBar and popup header shifted down ~7px by y: (parent.height - self.preferred-height) / 2 inside stretched HorizontalLayout

- 2026-11-21T13:16:03Z `issue`: CubbyStatusDot in CubbyResourceBar and popup header shifted down ~7px by y: (parent.height - self.preferred-height) / 2 inside stretched HorizontalLayout [ui/components.slint:1419]
- 2026-11-21T13:18:36Z `fix`: Removed y: (parent.height - self.preferred-height) / 2 from CubbyStatusDot in CubbyResourceBar and popup header; cargo test 51 passed, clippy clean [ui/components.slint:1419]
