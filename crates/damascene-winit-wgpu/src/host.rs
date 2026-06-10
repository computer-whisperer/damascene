//! Building blocks for writing a custom winit host.
//!
//! This crate's [`run`](crate::run) family owns the whole event loop and
//! is the right entry point for almost every app. A few integrations
//! can't hand over loop ownership — a resident multi-window process
//! spinning windows off one warm instance, portal dialogs, embedding in
//! an existing `ApplicationHandler` — and have to translate winit
//! events and drive `damascene_wgpu::Runner` themselves.
//!
//! The submodules here expose the host's reusable layers so such a
//! custom host doesn't fork-and-drift this crate:
//!
//! - [`input`] — the pure winit → damascene event mappers.
//!
//! The built-in run loop calls through these same functions, so the
//! public surface is the tested path.

pub mod input;
