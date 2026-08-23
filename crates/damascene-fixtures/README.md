<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-fixtures

![Showcase — Booleans section: switches, checkboxes, radio group](https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/showcase_booleans.png)

Backend-neutral Damascene fixture apps and render trees.

This crate is useful as source material when learning Damascene's app API:
it contains realistic `damascene_core::App` implementations without any
windowing, GPU setup, or browser glue.

Use it when validating a backend or host:

```rust
use damascene_fixtures::Showcase;

let app = Showcase::new();
```

For normal application code, depend on `damascene-core` and import
`damascene_core::prelude::*`. For a native desktop host, add
`damascene-winit-wgpu`.
