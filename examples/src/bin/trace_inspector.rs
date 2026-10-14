//! Interactive Carbon/workbench demo — the dense-application counterpart
//! to the `hero` binary's shadcn dashboard.
//!
//! Run: `cargo run -p damascene-examples --bin trace_inspector`
//!
//! Worth scrolling the draw-call table: it is 2431 rows built lazily
//! through `virtual_list`, so only the visible window exists at any
//! moment.

use damascene_carbon::TraceInspector;
use damascene_carbon::inspector::INSPECTOR_LOGICAL_SIZE;
use damascene_core::Rect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = INSPECTOR_LOGICAL_SIZE;
    let viewport = Rect::new(0.0, 0.0, w as f32, h as f32);
    damascene_winit_wgpu::run(
        "Damascene — trace inspector (Carbon)",
        viewport,
        TraceInspector,
    )
}
