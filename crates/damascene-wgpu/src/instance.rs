//! wgpu-side scissor binding. The cross-backend paint primitives —
//! `QuadInstance`, `InstanceRun`, `PaintItem`, `PhysicalScissor`,
//! `physical_scissor`, `pack_instance`, `close_run`, `rgba_f32` —
//! live in [`damascene_core::paint`] so `damascene-wgpu` and `damascene-vulkano`
//! consume them from a single source. This file only carries the
//! wgpu-shaped `set_scissor` wrapper, which has to know about
//! `wgpu::RenderPass` and so cannot live in the backend-agnostic core.

use damascene_core::paint::PhysicalScissor;

pub(crate) fn set_scissor(
    pass: &mut wgpu::RenderPass<'_>,
    scissor: Option<PhysicalScissor>,
    full: PhysicalScissor,
) {
    let s = scissor.unwrap_or(full);
    pass.set_scissor_rect(s.x, s.y, s.w, s.h);
}
