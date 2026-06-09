//! Free constructors for common [`El`] tree shapes.
//!
//! Kept separate from the core `El` type so the central node definition
//! stays focused on fields and chainable modifiers.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;
use std::sync::Arc;

use crate::image::Image;
use crate::layout::VirtualItems;
use crate::math::{MathDisplay, MathExpr};

use super::layout_types::{Align, Axis, Size};
use super::node::El;
use super::semantics::Kind;

/// A vertical container — the layout fallback.
///
/// Reach for a named widget first: [`card`] / [`titled_card`] for boxed
/// surfaces; [`sidebar`] for nav rails; [`toolbar`] for page headers;
/// [`item`] for object rows; [`form_item`] / [`field_row`] for stacked
/// fields. `column` is the right answer when no widget shape fits.
///
/// Defaults match CSS flex's `display: flex; flex-direction: column`:
/// `axis = Column`, `align = Stretch`, `width = Hug`, `height = Hug`,
/// `gap = 0`. Children shrink to content on the main axis (height)
/// and stretch to the column's width on the cross axis.
///
/// To claim the parent's extent (the analog of `width: 100%` /
/// `flex: 1`), set `.width(Size::Fill(1.0))` /
/// `.height(Size::Fill(1.0))`. To space children apart, set
/// `.gap(tokens::SPACE_*)` — CSS-style opt-in spacing.
///
/// Switch `align` to `Center` / `Start` / `End` and children shrink
/// to their content width so the alignment can position them — the
/// same as CSS `align-items` non-stretch semantics.
///
/// **Smell:** `column([...]).fill(CARD).stroke(BORDER).radius(...)`
/// reinvents [`card`]; `column([...]).fill(CARD).stroke(BORDER).width(SIDEBAR_WIDTH)`
/// reinvents [`sidebar`]. Use the named widget — same recipe, the right
/// surface role, less to forget.
///
/// [`card`]: crate::widgets::card::card
/// [`titled_card`]: crate::widgets::card::titled_card
/// [`sidebar`]: crate::widgets::sidebar::sidebar
/// [`toolbar`]: crate::widgets::toolbar::toolbar
/// [`item`]: crate::widgets::item::item
/// [`form_item`]: crate::widgets::form::form_item
/// [`field_row`]: crate::widgets::form::field_row
#[track_caller]
pub fn column<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
}

/// A horizontal container — the layout fallback.
///
/// Reach for a named widget first: [`item`] for clickable object rows
/// (recent file, repo, project, person, asset entry — anywhere you'd
/// otherwise build a focusable row with stacked text and trailing
/// buttons); [`toolbar`] for page chrome; [`field_row`] for label +
/// control; [`tabs_list`] for segmented controls; [`breadcrumb_list`] /
/// [`pagination_content`] for navigation rows. `row` is the right
/// answer when no widget shape fits.
///
/// Defaults match CSS flex's `display: flex; flex-direction: row`:
/// `axis = Row`, `align = Stretch`, `width = Hug`, `height = Hug`,
/// `gap = 0`. Children shrink to content on the main axis (width)
/// and stretch to the row's height on the cross axis.
///
/// `Stretch` is the cross-axis default the same way `align-items:
/// stretch` is in CSS. For typical content rows (`[icon, text,
/// button]`) you almost always want `.align(Center)` to vertically
/// center the children — the CSS-Tailwind muscle memory of
/// `flex items-center`. Without it, smaller fixed-size children
/// (badges, icons) sit at the top of the row, just like CSS does.
///
/// To space children apart, set `.gap(tokens::SPACE_*)` — opt-in
/// like CSS.
///
/// **Smell:** a focusable, keyed `row([column([t1, t2]), button, button])`
/// used as a clickable resource entry — that's [`item`], not a hand-rolled
/// row. The named widget gives you hover, press, focus, the rail, and
/// the slots (`item_media`, `item_content`, `item_actions`) for free.
///
/// [`item`]: crate::widgets::item::item
/// [`toolbar`]: crate::widgets::toolbar::toolbar
/// [`field_row`]: crate::widgets::form::field_row
/// [`tabs_list`]: crate::widgets::tabs::tabs_list
/// [`breadcrumb_list`]: crate::widgets::breadcrumb::breadcrumb_list
/// [`pagination_content`]: crate::widgets::pagination::pagination_content
#[track_caller]
pub fn row<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Row)
}

/// An overlay stack; children share the parent's rect.
///
/// For modals, sheets, popovers, and tooltips reach for the named
/// widget instead — [`dialog`], [`sheet`], [`popover`], `.tooltip(...)`.
/// `stack` is the layered-visuals primitive (focus rings, custom
/// badges painted over content) that those widgets compose against.
///
/// [`dialog`]: crate::widgets::dialog::dialog
/// [`sheet`]: crate::widgets::sheet::sheet
/// [`popover`]: crate::widgets::popover::popover
#[track_caller]
pub fn stack<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Overlay)
}

/// A vertical scroll viewport. Children stack as in [`column()`]; the
/// container clips overflow and translates content by the current scroll
/// offset. Wheel events over the viewport update the offset.
///
/// Give it a `.key("...")` so the offset persists by name across
/// rebuilds — without a key, the offset is keyed by sibling index and
/// resets if structure shifts.
#[track_caller]
pub fn scroll<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Scroll)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .clip()
        .scrollable()
        .scrollbar()
}

/// Block whose direct children flow inline (text leaves + embeds +
/// hard breaks). Models HTML's `<p>` shape: heterogeneous children,
/// attributed runs, optional inline embeds. Children are styled via
/// the existing modifier chain (`.bold()`, `.italic()`, `.color(c)`,
/// `.code()`, `.link(url)`, etc.) — there is no parallel
/// `RichText`/`TextRun` type.
///
/// ```ignore
/// text_runs([
///     text("Damascene — "),
///     text("rich text").bold(),
///     text(" composition."),
///     hard_break(),
///     text("Custom shaders, custom layouts, "),
///     text("virtual_list").code(),
///     text(" — and inline runs."),
/// ])
/// ```
#[track_caller]
pub fn text_runs<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Inlines)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Start)
        .width(Size::Fill(1.0))
        .children(children)
}

/// Forced line break inside a [`text_runs`] block. Mirrors HTML's
/// `<br>`. Outside an `Inlines` parent, lays out as a zero-size leaf.
#[track_caller]
pub fn hard_break() -> El {
    El::new(Kind::HardBreak)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Native mathematical notation. Alias for [`math_inline`].
#[track_caller]
pub fn math(expr: impl Into<Arc<MathExpr>>) -> El {
    math_inline(expr)
}

/// Math notation in compact in-line style (TeX text style); the El
/// hugs the rendered expression.
#[track_caller]
pub fn math_inline(expr: impl Into<Arc<MathExpr>>) -> El {
    El::new(Kind::Math)
        .at_loc(Location::caller())
        .math_expr(expr)
        .math_display(MathDisplay::Inline)
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Math notation in display style for standalone equations; fills the
/// available width.
#[track_caller]
pub fn math_block(expr: impl Into<Arc<MathExpr>>) -> El {
    El::new(Kind::Math)
        .at_loc(Location::caller())
        .math_expr(expr)
        .math_display(MathDisplay::Block)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Virtualized vertical list of `count` rows of fixed height
/// `row_height`. The library calls `build_row(i)` only for indices
/// whose rect intersects the visible viewport, then lays them out at
/// the scroll-shifted Y. `.gap(...)` contributes spacing between rows,
/// matching column-style layout. Authors typically key rows with a
/// stable identifier (`button("foo").key("msg-abc")`) so hover/press/
/// focus state survives scrolling.
///
/// The returned El defaults to `Size::Fill(1.0)` on both axes (it's a
/// viewport — its size is decided by the parent). `Size::Hug` would
/// defeat virtualization and panics at layout time.
#[track_caller]
pub fn virtual_list<F>(count: usize, row_height: f32, build_row: F) -> El
where
    F: Fn(usize) -> El + Send + Sync + 'static,
{
    let mut el = El::new(Kind::VirtualList)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .clip()
        .scrollable()
        .scrollbar();
    el.virtual_items = Some(VirtualItems::new(count, row_height, build_row));
    el
}

/// Variable-height variant of [`virtual_list`]. `row_key(i)` must
/// return a stable identity for the logical row at `i`; the dynamic
/// list uses those identities to preserve an in-viewport anchor while
/// rows are inserted, removed, measured, or remeasured at a new width.
/// Each row sizes itself from its own content (`Size::Hug` or
/// `Size::Fixed` on the main axis); `estimated_row_height` is used for
/// unmeasured rows when the library positions the visible window and
/// computes the scrollbar thumb. `.gap(...)` contributes spacing
/// between rows, matching column-style layout.
///
/// Use this when row heights are content-driven (diff hunks, expanded
/// rows, comment threads) and a single `row_height` would either waste
/// space or truncate. For genuinely uniform lists prefer
/// [`virtual_list`] — its O(1) range math is cheaper and free of any
/// estimate/measure jitter.
#[track_caller]
pub fn virtual_list_dyn<K, F>(
    count: usize,
    estimated_row_height: f32,
    row_key: K,
    build_row: F,
) -> El
where
    K: Fn(usize) -> String + Send + Sync + 'static,
    F: Fn(usize) -> El + Send + Sync + 'static,
{
    let mut el = El::new(Kind::VirtualList)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .clip()
        .scrollable()
        .scrollbar();
    el.virtual_items = Some(VirtualItems::new_dyn(
        count,
        estimated_row_height,
        row_key,
        build_row,
    ));
    el
}

/// A `Fill(1)` filler. Inside a `row` it pushes siblings to the right;
/// inside a `column` it pushes siblings to the bottom.
#[track_caller]
pub fn spacer() -> El {
    El::new(Kind::Spacer)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

/// A raster image element. The El hugs the image's natural pixel
/// size by default; set [`El::width`] / [`El::height`] for an
/// explicit box, and [`El::image_fit`] to control projection.
///
/// ```
/// use damascene_core::prelude::*;
/// let pixels = vec![0u8; 4 * 4 * 4];
/// let img = Image::from_rgba8(4, 4, pixels);
/// let _ = image(img).image_fit(ImageFit::Cover).radius(8.0);
/// ```
#[track_caller]
pub fn image(img: impl Into<Image>) -> El {
    El::new(Kind::Image).at_loc(Location::caller()).image(img)
}

/// An app-supplied vector asset. By default Damascene preserves authored
/// fills, strokes, and gradients through the painted vector path; call
/// [`El::vector_mask`] when the asset should be treated as a one-colour
/// coverage mask. Companion to [`crate::icon`] for content that
/// doesn't fit icon conventions: arbitrary-aspect bounding boxes,
/// programmatic construction each frame. Pairs with
/// [`crate::vector::PathBuilder`] for ergonomic path construction.
///
/// # Sizing
///
/// The default size matches the asset's view-box dimensions in logical
/// pixels. Set [`El::width`] / [`El::height`] / [`El::fill_size`] to
/// override. Painted vectors are tessellated into the resolved rect;
/// mask vectors sample the backend MSDF atlas across that rect.
///
/// # Caching
///
/// The asset's [`VectorAsset::content_hash`](crate::vector::VectorAsset::content_hash)
/// is the backend cache key. Apps that build the same shape twice (two
/// commits sharing a merge connector geometry, two flowchart edges with
/// the same arc) can share backend work; per-frame-unique geometry gets
/// one cache entry per unique shape.
///
/// ```ignore
/// use damascene_core::prelude::*;
/// use damascene_core::tree::Color;
///
/// let curve = PathBuilder::new()
///     .move_to(0.0, 0.0)
///     .cubic_to(20.0, 0.0, 0.0, 60.0, 20.0, 60.0)
///     .stroke_solid(Color::srgb_u8(80, 200, 240), 2.0)
///     .stroke_line_cap(VectorLineCap::Round)
///     .build();
/// let asset = VectorAsset::from_paths([0.0, 0.0, 20.0, 60.0], vec![curve]);
/// let _ = vector(asset);
/// ```
#[track_caller]
pub fn vector(asset: crate::vector::VectorAsset) -> El {
    let [_, _, vw, vh] = asset.view_box;
    El::new(Kind::Vector)
        .at_loc(Location::caller())
        .width(Size::Fixed(vw.max(0.0)))
        .height(Size::Fixed(vh.max(0.0)))
        .vector_source(std::sync::Arc::new(asset))
}

/// An app-owned-texture surface. Damascene composites the texture into
/// the paint stream at the El's resolved rect — no upload, no per-frame
/// copy. The default size matches the texture's pixel dimensions; set
/// [`El::width`] / [`El::height`] (or `.fill_size()`) for an explicit
/// box.
///
/// # Sizing, projection, and transforms
///
/// The texture's pixel dimensions are **independent of the rendered
/// size**. By default the widget stretches the texture across the
/// resolved rect ([`crate::image::ImageFit::Fill`]); reach for
/// [`El::surface_fit`] to letterbox-preserve aspect ratio
/// ([`crate::image::ImageFit::Contain`]), crop-cover
/// ([`crate::image::ImageFit::Cover`]), or paint at natural size
/// ([`crate::image::ImageFit::None`]). [`El::surface_transform`]
/// composes an affine on top — rotate, mirror, zoom/pan — applied
/// around the centre of the post-fit rect.
///
/// Picking a sizing strategy:
/// - For pixel-accurate display, size the widget to the texture's
///   pixel dimensions (the default constructor does this for you).
/// - For a 3D viewport or video frame whose source resolution should
///   track the rendered size, the app should re-allocate its texture
///   to match the resolved rect (read it via `UiState::rect_of_key`
///   after `prepare()`).
/// - For an animated image whose natural dimensions are fixed
///   (decoded GIF / WebP / APNG, decoded video frame),
///   `surface_fit(Contain)` letterboxes into any layout rect with
///   no per-resize allocation.
///
/// # z-order, scissor, hit-test
///
/// The widget participates in layout, scissor, scrolling, hit-test,
/// and z-order like any other El: siblings declared before this one
/// paint underneath, siblings after paint on top. The auto-clip
/// scissor clamps painted content to the El's content rect — affines
/// or `Cover` projections that overflow are cropped.
///
/// ```ignore
/// // Pseudocode — the AppTexture comes from a backend constructor.
/// use damascene_core::prelude::*;
/// let tex: AppTexture = /* damascene_wgpu::app_texture(...) */ todo!();
/// let _ = surface(tex)
///     .fill_size()
///     .surface_fit(ImageFit::Contain)
///     .surface_alpha(SurfaceAlpha::Opaque)
///     .surface_transform(Affine2::rotate(0.1));
/// ```
#[track_caller]
pub fn surface(texture: crate::surface::AppTexture) -> El {
    let (w, h) = texture.size_px();
    El::new(Kind::Surface)
        .at_loc(Location::caller())
        .width(Size::Fixed(w as f32))
        .height(Size::Fixed(h as f32))
        .surface_source(crate::surface::SurfaceSource::Texture(texture))
}

/// A small, polished, hardware-accelerated 3D scene — point scatter, lit
/// meshes, and lines — as a first-class element. Unlike [`surface`], the
/// app does not own a renderer or a device: it describes the scene with a
/// [`crate::scene::SceneSpec`] and the backend renders it. Fills its area
/// by default (it has no intrinsic pixel size); size it like any El.
///
/// ```ignore
/// use damascene_core::prelude::*;
/// use damascene_core::scene::{GridPlanes, SceneSpec};
///
/// let scene = SceneSpec::new().points(scatter).mesh(model).grid(GridPlanes::XZ);
/// let _ = chart3d(scene).key("scene");
/// ```
#[track_caller]
pub fn chart3d(scene: crate::scene::SceneSpec) -> El {
    El::new(Kind::Scene3D)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .scene_source(scene)
}

/// A 1-pixel separator line.
#[track_caller]
pub fn divider() -> El {
    El::new(Kind::Divider)
        .at_loc(Location::caller())
        .height(Size::Fixed(1.0))
        .width(Size::Fill(1.0))
        .fill(crate::tokens::BORDER)
}

// ---------- &str → El convenience ----------
//
// Lets `titled_card("Title", ["a body line"])` work without `text(...)`.

impl From<&str> for El {
    fn from(s: &str) -> Self {
        crate::widgets::text::text(s)
    }
}

impl From<String> for El {
    fn from(s: String) -> Self {
        crate::widgets::text::text(s)
    }
}

impl From<&String> for El {
    fn from(s: &String) -> Self {
        crate::widgets::text::text(s.as_str())
    }
}
