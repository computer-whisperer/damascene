//! Pure winit → damascene input mappers — re-exported from
//! [`damascene_winit`], the shared render-backend-free home for these
//! tables (#121). Custom hosts on other backends should depend on that
//! crate directly; this module stays so existing
//! `damascene_winit_wgpu::host::input::*` paths keep working.

pub use damascene_winit::*;
