//! vulkano-side scissor binding. The cross-backend paint primitives —
//! `QuadInstance`, `InstanceRun`, `PaintItem`, `PhysicalScissor`,
//! `physical_scissor`, `pack_instance`, `close_run`, `rgba_f32` —
//! live in [`damascene_core::paint`] so `damascene-wgpu` and `damascene-vulkano`
//! consume them from a single source. This file only carries the
//! vulkano-shaped `set_scissor` wrapper, which has to know about
//! `AutoCommandBufferBuilder` and so cannot live in the
//! backend-agnostic core.

use damascene_core::paint::PhysicalScissor;
use smallvec::smallvec;
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};
use vulkano::pipeline::graphics::viewport::Scissor;

/// Apply `scissor` (or `full` when `None`) to the given primary
/// command-buffer builder via vulkano's dynamic scissor state. The
/// pipeline must declare `Scissor` in its `dynamic_state`.
pub(crate) fn set_scissor(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    scissor: Option<PhysicalScissor>,
    full: PhysicalScissor,
) {
    let s = scissor.unwrap_or(full);
    builder
        .set_scissor(
            0,
            smallvec![Scissor {
                offset: [s.x, s.y],
                extent: [s.w.max(1), s.h.max(1)],
            }],
        )
        .expect("set_scissor");
}
