<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-winit

Pure `winit` → damascene input mappers, shared by every winit host.

This crate depends on `damascene-core` and `winit` only — no render
backend — so custom hosts (vulkano, ash, or fully out-of-tree) get the
same total key/pointer/cursor translation tables the in-tree
`damascene-winit-wgpu` host uses, without dragging in a GPU stack:

```rust
use damascene_winit::{key_modifiers, map_key, map_physical, pointer_button};
```

- `map_key` — winit logical `Key` → `LogicalKey` (W3C `key` named set;
  unmapped keys become `Unidentified`, never a stringly fallback)
- `map_physical` — winit `PhysicalKey` → `PhysicalKey` (W3C `code` set,
  including the `SuperLeft`→`MetaLeft`-style spelling bridges)
- `key_modifiers`, `pointer_button`, `touch_pressure`, `winit_cursor`

The tables are guarded by totality tests against `damascene-core`'s
`NamedKey::ALL` / `PhysicalKey::ALL` / `Cursor::ALL`, so a vocabulary
addition in core fails this crate's tests rather than silently degrading
keys in every host.

Part of [Damascene](https://github.com/computer-whisperer/damascene), a
GPU-accelerated UI library designed for LLM authorship.
