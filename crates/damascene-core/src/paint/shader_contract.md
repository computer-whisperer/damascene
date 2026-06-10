# Custom-shader color contract

A custom shader registered via `ShaderBinding::custom(name)` and bound to
an `El` runs against the same vertex/fragment pipeline as the stock
shaders. The contract below documents what arrives in its uniform slots
and what it's expected to write out.

This document is the canonical reference; the shaders under
`crates/damascene-core/src/paint/shaders/` (`rounded_rect.wgsl`,
`text_sdf.wgsl`, `focus_ring.wgsl`) and the cross-tree exemplars
(`crates/damascene-fixtures/.../liquid_glass.wgsl`,
`examples/.../gradient.wgsl`) all conform.

## Color values arrive in the renderer's working color space

Every `Color` set on a `UniformBlock` (via `set_color`,
`UniformValue::Color`, or any of the `slot_a..slot_e` paths) is
converted to the renderer's *working color space* exactly once, at
paint time, before the bytes hit the GPU. The shader sees an
`array<f32, 4>` already in working space — straight (not premultiplied)
alpha, no further OETF / matrix work required.

- Default working space: `damascene_core::paint::DEFAULT_WORKING_COLOR_SPACE`
  = `ColorSpace::SRGB_LINEAR` (sRGB primaries, linear transfer, 100 nit
  reference white).
- Hosts targeting wide-gamut / HDR surfaces (Wayland color-management
  output, macOS Display-P3, etc.) configure their backend to pack via
  `pack_instance_in(rect, shader, uniforms, working)` /
  `rgba_f32_in(c, working)` with a different `ColorSpace` (typically
  `SCRGB_LINEAR` for `Rgba16Float` swapchains, or a BT.2020-linear
  variant). The shader receives values in *that* space without
  modification.

What this means for shader authors:

1. **Don't decode sRGB.** The boundary already converted the input from
   its authored space (sRGB, Display-P3, BT.2020, …) into linear-light
   working space. Multiplying, mixing, and lerping channel values is
   safe linear-light math.
2. **Don't premultiply alpha at upload time.** Slots arrive un-
   premultiplied. The stock pipeline's blend state premultiplies as
   part of `OVER` if needed; if you write your own blend state, follow
   the convention.
3. **Write output in working space.** Whatever the working space is,
   your fragment's `@location(0)` output should be a value in that
   space (linear-light if working space is `*_LINEAR`). The renderer
   handles the encode-to-scanout step (either via the surface texture's
   sRGB view, or via a post-blit on HDR paths).

## Backdrop sampling

Custom shaders bound to a node that opts in to backdrop sampling (e.g.
`liquid_glass.wgsl` via the `BackdropSnapshot` mechanism) receive a
`@group(1) @binding(0) sampler` + `@group(1) @binding(1) texture_2d`
covering the working-space framebuffer at the snapshot point. Samples
from this texture are *also* in working color space — no decode
required. Composite against your own paint in linear-light, then write
the result back out in the same working space.

## Generic vec slots

For non-`RoundedRect` stock or custom shaders, `pack_instance` reads
uniforms named `vec_a` / `vec_b` / `vec_c` / `vec_d` / `vec_e` into
`slot_a..slot_e` (in that order). The conversion rules per `UniformValue`:

| `UniformValue`     | What lands in the slot                                  |
| ------------------ | ------------------------------------------------------- |
| `Color(c)`         | Working-space float4 via `rgba_f32_in(c, working)`      |
| `Vec4([a,b,c,d])`  | The four floats verbatim                                |
| `Vec2([x,y])`      | `[x, y, 0.0, 0.0]`                                      |
| `F32(f)`           | `[f, 0.0, 0.0, 0.0]`                                    |
| `Bool(b)`          | `[1.0 or 0.0, 0.0, 0.0, 0.0]`                           |

If you need to pass a *raw* RGBA float that should *not* be color-space
converted (e.g. a normal-map encoding packed into RGB, or an HDR
extended-range value the shader will tone-map itself), pack it as
`Vec4` not `Color`. Anything declared as `Color` is treated as
visible-light authored content and goes through the working-space
boundary.

## Named slots — let the WGSL field names be the contract

Positional `vec_a..vec_e` keys force you to mirror slot meanings in
comments on both sides — reorder either side and it compiles fine and
renders garbage. Custom shaders registered through any backend's
`register_shader_with` get their instance-attribute names introspected
(`paint::slots`), and uniforms then route **by the WGSL field name**:

```wgsl
struct InstanceInput {
    @location(1) rect:          vec4<f32>,
    @location(2) xyz_to_rgb_c0: vec4<f32>,
    @location(3) xyz_to_rgb_c1: vec4<f32>,
    @location(4) xyz_to_rgb_c2: vec4<f32>,
    @location(6) view_bounds:   vec4<f32>,
}
```

```rust,ignore
ShaderBinding::custom("chroma_field")
    .mat3(["xyz_to_rgb_c0", "xyz_to_rgb_c1", "xyz_to_rgb_c2"], xyz_to_rgb)
    .vec4("view_bounds", bounds)
```

A key that matches neither a declared field name nor a positional alias
warns once at the first draw (`log::warn!`) and lands nowhere — drift
between the Rust packing and the WGSL unpacking surfaces immediately
instead of as wrong pixels. `ShaderBinding::mat3` packs a column-major
3×3 across three named slots (`mat3x3<f32>(inst.c0.xyz, inst.c1.xyz,
inst.c2.xyz)` on the WGSL side). A free-slot attribute that is not
`vec4<f32>` is caught at registration with the field name attached,
before the backend's own (more cryptic) pipeline error. The positional
aliases stay valid forever.

## Why this contract exists

Damascene's `Color` type carries a `ColorSpace` tag end-to-end. The paint
stream is the single boundary where authored space → working space
happens, so shaders don't need to know whether the author wrote
`Color::srgb_hex("#FAFAFA")`, `Color::display_p3(...)`, or
`Color::bt2020_pq(...)` — by the time the bytes reach a uniform slot
they're all in the same linear-light space the shader can compose in.

When step 2 (Wayland color-management surface support, HDR output, etc.)
lands, the only thing that changes is what working space the backend
picks. Existing custom shaders that follow the contract above continue
to work unchanged — their inputs and outputs are still "working-space
linear premultiplied float4", the renderer just signs the surface
differently.
