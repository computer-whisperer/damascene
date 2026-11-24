//! The threading contract: which thread owns the tree, what may cross
//! threads, and the one target where the answers differ.
//!
//! # The contract
//!
//! - **The tree belongs to one thread.** An app builds its [`El`] tree,
//!   and the host lays it out and paints it, on the thread that runs the
//!   host's event loop — the UI thread. Damascene never moves a tree to
//!   another thread or calls a builder closure from one, and the
//!   text-measurement caches are per thread.
//! - **Other threads talk to the UI through data handles.** A
//!   [`SeriesHandle`](crate::plot::SeriesHandle), a
//!   [`GeometryHandle`](crate::scene::GeometryHandle) and the app's own
//!   `Arc<Mutex<…>>` row data are plain CPU data: `Send + Sync` on every
//!   target. Another thread writes into the handle and has the host
//!   redraw; the UI thread reads it on the next build.
//! - **The tree is `Send + Sync` wherever GPU handles are.** On every
//!   target but one, [`El`], [`DrawOp`](crate::ir::DrawOp) and
//!   [`AppTexture`](crate::surface::AppTexture) are `Send + Sync`, so a
//!   tree may sit in shared app state or be built ahead of time on
//!   another thread.
//!
//! # The exception: `wasm32` with atomics
//!
//! A browser GPU object is a JavaScript object, and JavaScript objects
//! belong to the thread that created them: a Web Worker has its own heap
//! and cannot reach the page's `GPUTexture` at all. On single-threaded
//! wasm that costs nothing, and the wgpu backend marks its types
//! `Send + Sync` through wgpu's `fragile-send-sync-non-atomic-wasm`
//! feature. Built with `-C target-feature=+atomics` — threads over one
//! shared memory — that feature is inert and wgpu's types are truthfully
//! neither.
//!
//! On that target only:
//!
//! - [`AppTexture`](crate::surface::AppTexture) is `!Send + !Sync`, and
//!   so is everything that can hold one: [`El`] and
//!   [`DrawOp`](crate::ir::DrawOp). The tree stays on the page's thread.
//! - What the library retains for the tree is not required to be
//!   `Send + Sync` either: the row builders of
//!   [`virtual_list`](crate::tree::virtual_list) and its relatives, an
//!   [`El::layout`] function, and a
//!   [`WidgetState`](crate::state::WidgetState). They may hold an
//!   `AppTexture` or a prebuilt `El`.
//! - Data handles are still `Send + Sync`, but a worker cannot yet use
//!   them freely — see the next section.
//!
//! [`WasmNotSendSync`] is the bound that expresses this: `Send + Sync`
//! everywhere except `wasm32` with atomics, where it asks for nothing.
//! The name and the shape are wgpu's. wgpu's own trait also depends on
//! its `fragile-send-sync-non-atomic-wasm` feature, which
//! `damascene-wgpu` always enables on `wasm32`, so the two agree on
//! every target Damascene builds for. The condition is a target `cfg`,
//! not a Cargo feature — it follows the app's own `RUSTFLAGS`, and there
//! is nothing to enable.
//!
//! # Blocking on the page's thread
//!
//! A browser does not let the page's thread block. On `wasm32` with
//! atomics a *contended* `std::sync::Mutex::lock` spins for a moment and
//! then waits, and a wait on the page's thread traps. So on that target
//! a lock shared between the page and a worker is a hazard however
//! briefly the worker holds it:
//!
//! - **Damascene's data handles are not yet safe to write from a
//!   worker.** [`SeriesHandle`](crate::plot::SeriesHandle) and
//!   [`GeometryHandle`](crate::scene::GeometryHandle) take a blocking
//!   lock when the UI thread reads them during paint, so a worker
//!   writing at that moment can trap the page. Until that changes, a
//!   worker hands its finished data to the page's thread, and the page
//!   calls `set` / `append`.
//! - **The app's own shared row data** — the `Arc<Mutex<…>>` a virtual
//!   list's row builder reads — must be taken with `try_lock` on the
//!   page's side when a worker also locks it.
//! - **Waking the host** from a worker goes through the page as well:
//!   the web host's handle is not `Send`, so the worker posts a message
//!   and the page requests the redraw.
//!
//! None of this applies to native targets, where a contended lock simply
//! waits.
//!
//! [`El`]: crate::tree::El
//! [`El::layout`]: crate::tree::El::layout

use crate::layout::LayoutCtx;
use crate::tree::{El, Rect};

/// `Send + Sync` on every target except `wasm32` with atomics, where it
/// is satisfied by every type. See the [module docs](self).
///
/// Implemented for every eligible type; there is nothing to implement
/// by hand. Use it wherever a bound exists only to keep the tree
/// `Send + Sync` — a wrapper around
/// [`virtual_list`](crate::tree::virtual_list) writes
/// `F: Fn(usize) -> El + WasmNotSendSync + 'static`.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub trait WasmNotSendSync: Send + Sync {}

#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
impl<T: Send + Sync + ?Sized> WasmNotSendSync for T {}

/// The `wasm32` + atomics side of the trait above: satisfied by every
/// type. See the [module docs](self).
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub trait WasmNotSendSync {}

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
impl<T: ?Sized> WasmNotSendSync for T {}

// A trait object can only name auto traits beside its principal trait,
// so the retained closure types cannot say `+ WasmNotSendSync`; each is
// declared once per side of the same condition instead.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
mod retained {
    use super::{El, LayoutCtx, Rect};

    /// The stored form of a virtual list's row builder: `Send + Sync`
    /// wherever [`WasmNotSendSync`](super::WasmNotSendSync) is.
    pub type RowBuilderFn = dyn Fn(usize) -> El + Send + Sync;
    /// The stored form of a virtual list's row-key function: `Send +
    /// Sync` wherever [`WasmNotSendSync`](super::WasmNotSendSync) is.
    pub type RowKeyFn = dyn Fn(usize) -> String + Send + Sync;
    /// The stored form of a custom layout function: `Send + Sync`
    /// wherever [`WasmNotSendSync`](super::WasmNotSendSync) is.
    pub type CustomLayoutFn = dyn Fn(LayoutCtx) -> Vec<Rect> + Send + Sync;
}

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
mod retained {
    use super::{El, LayoutCtx, Rect};

    /// The stored form of a virtual list's row builder: `Send + Sync`
    /// wherever [`WasmNotSendSync`](super::WasmNotSendSync) is.
    pub type RowBuilderFn = dyn Fn(usize) -> El;
    /// The stored form of a virtual list's row-key function: `Send +
    /// Sync` wherever [`WasmNotSendSync`](super::WasmNotSendSync) is.
    pub type RowKeyFn = dyn Fn(usize) -> String;
    /// The stored form of a custom layout function: `Send + Sync`
    /// wherever [`WasmNotSendSync`](super::WasmNotSendSync) is.
    pub type CustomLayoutFn = dyn Fn(LayoutCtx) -> Vec<Rect>;
}

pub use retained::{CustomLayoutFn, RowBuilderFn, RowKeyFn};

// The contract, held at compile time. `tools/check_wasm_atomics.sh`
// builds the `wasm32` + atomics side, which no stable toolchain reaches.
const fn assert_send_sync<T: Send + Sync + ?Sized>() {}

// Data handles cross threads on every target.
const _: () = {
    assert_send_sync::<crate::plot::SeriesHandle>();
    assert_send_sync::<crate::scene::MeshHandle>();
    assert_send_sync::<crate::scene::PointsHandle>();
    assert_send_sync::<crate::scene::LinesHandle>();
};

// The tree is `Send + Sync` wherever GPU handles are.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
const _: () = {
    assert_send_sync::<El>();
    assert_send_sync::<crate::ir::DrawOp>();
    assert_send_sync::<crate::surface::AppTexture>();
    assert_send_sync::<crate::surface::SurfaceSource>();
};

// With atomics the retained closures may capture what the page's thread
// owns: here an `Rc`, standing in for an `AppTexture` over a browser GPU
// texture.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
#[allow(dead_code)]
fn thread_bound_captures_are_accepted() {
    use crate::tree::{spacer, virtual_grid, virtual_list, virtual_list_dyn};
    use crate::widgets::data_table::data_table_virtual;

    let owned = std::rc::Rc::new(0usize);
    let (a, b, c, d) = (owned.clone(), owned.clone(), owned.clone(), owned);
    let (e, f, g) = (a.clone(), a.clone(), a.clone());
    let _ = virtual_grid(1, 1, 1.0, 0.0, move |i| spacer().key(format!("{}", *e + i)));
    let _ = data_table_virtual(
        "probe",
        &[],
        1,
        move |i| format!("{}", *f + i),
        move |i| spacer().key(format!("{}", *g + i)),
    );
    let _ = virtual_list(1, 1.0, move |i| spacer().key(format!("{}", *a + i)));
    let _ = virtual_list_dyn(
        1,
        1.0,
        move |i| format!("{}", *b + i),
        move |i| spacer().key(format!("{}", *c + i)),
    );
    let _ = spacer().layout(move |cx| vec![cx.container; cx.children.len().min(*d)]);

    #[derive(Debug)]
    struct PageOwned(#[allow(dead_code)] std::rc::Rc<usize>);
    impl crate::state::WidgetState for PageOwned {}
}
