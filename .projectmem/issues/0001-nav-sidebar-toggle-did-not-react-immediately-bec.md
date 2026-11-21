# #0001 Nav sidebar toggle did not react immediately because clicked callback only invoked persistence round-trip

- 2026-11-21T02:52:07Z `issue`: Nav sidebar toggle did not react immediately because clicked callback only invoked persistence round-trip [ui/advanced.slint:65]
- 2026-11-21T02:52:12Z `fix`: Direct sidebar_collapsed assignment in CubbyTopBar.clicked before persistence callback; verified synthetic Slint click collapsed=true, capture rail 70px physical = 56 logical vs expanded 260 = 208, settings seed true relaunch collapsed on startup. [ui/advanced.slint:65]
