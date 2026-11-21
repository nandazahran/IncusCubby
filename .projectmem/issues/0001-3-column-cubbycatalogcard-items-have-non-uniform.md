# #0001 3-column CubbyCatalogCard items have non-uniform widths due to intrinsic text/tag preferred-width in HorizontalLayout

- 2026-11-21T12:04:37Z `issue`: 3-column CubbyCatalogCard items have non-uniform widths due to intrinsic text/tag preferred-width in HorizontalLayout [ui/components.slint:449]
- 2026-11-21T12:10:28Z `fix`: Set min-width: 0px and preferred-width: 0px on CubbyCatalogCard and grid cell wrappers in CreateView, plus min-width: 0px on card title Text; cargo test 50 passed, 3 ignored [ui/components.slint:451]
