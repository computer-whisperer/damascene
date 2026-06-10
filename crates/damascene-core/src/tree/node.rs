//! Core [`El`] node data shape.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::anim::Timing;
use crate::image::{Image, ImageFit};
use crate::layout::{LayoutFn, VirtualItems};
use crate::math::{MathDisplay, MathExpr};
use crate::metrics::{ComponentSize, MetricsRole};
use crate::shader::ShaderBinding;
use crate::style::StyleProfile;

use super::geometry::Sides;
use super::identity::HoverAlpha;
use super::layout_types::{Align, Axis, Justify, Size};
use super::semantics::{Kind, Source, SurfaceRole};
use super::text_types::{FontFamily, FontWeight, TextAlign, TextOverflow, TextRole, TextWrap};
use crate::color::Color;

/// Where the stock focus ring is drawn relative to the focusable node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FocusRingPlacement {
    /// Draw the ring outside the layout rect, using the paint-overflow band.
    #[default]
    Outside,
    /// Draw the ring just inside the layout rect. Use for tightly-stacked
    /// focusable rows where adjacent siblings intentionally share edges.
    Inside,
}

/// The core tree node.
///
/// Construct via the component builders (`text`, `button`, `card`,
/// `column`, …) and chain modifiers (`.padding`, `.gap`, `.fill`, …).
/// Avoid building `El` directly — the builders set polished defaults.
///
/// `#[non_exhaustive]` — `El` is meant to be built through the
/// component constructors, not by struct-literal syntax. Direct
/// construction from outside this crate is intentionally disabled
/// so adding new layout/style fields stays a non-breaking change.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct El {
    /// Semantic identity of the element — roughly an HTML tag. See [`Kind`].
    pub kind: Kind,
    /// How the color / status modifiers (`.primary()`, `.destructive()`, …)
    /// restyle this node. Set once by the component constructor. See
    /// [`StyleProfile`].
    pub style_profile: StyleProfile,
    /// Optional stable identity across tree rebuilds. Focusable and
    /// selectable widgets must set one (via `.key(...)`) so their
    /// per-node state survives rebuilds; the layout pass folds it into
    /// the path-based [`Self::computed_id`].
    pub key: Option<String>,
    /// Claim every pointer event inside this node's painted rect so
    /// clicks (and scroll routing) don't fall through to lower layers.
    /// Set on popover / dialog panels via [`Self::block_pointer`].
    pub block_pointer: bool,
    /// Expand this element's pointer hit target beyond its transformed
    /// layout rect. Layout-neutral and paint-neutral: siblings don't
    /// move, the element doesn't draw larger, and focus rings / shadows
    /// still use [`Self::paint_overflow`].
    ///
    /// Use sparingly for controls with deliberately small visuals but
    /// larger intended targets (resize handles, compact icon affordances,
    /// row chrome). Hover, press, cursor, tooltip, and click routing all
    /// share this expanded target, so the invisible area behaves like
    /// the visible control. Ancestor clips still bound hit-testing.
    pub hit_overflow: Sides,
    /// Participate in Tab traversal and receive focus, hover, and press
    /// state. Requires a stable [`Self::key`]. Focused nodes draw the
    /// stock focus ring per [`Self::focus_ring_placement`].
    pub focusable: bool,
    /// Whether the stock focus ring draws outside (default) or inside
    /// the layout rect. See [`FocusRingPlacement`].
    pub focus_ring_placement: FocusRingPlacement,
    /// Show the focus ring on this node even when focus arrived via
    /// pointer (i.e. the runtime's `focus_visible` is `false`). Default
    /// behavior matches the web platform's `:focus-visible` heuristic
    /// — ring on Tab, no ring on click. Widgets like text inputs and
    /// text areas opt in here because the ring is a meaningful
    /// "this surface is now the active editing target" affordance even
    /// when activated by mouse, beyond what the caret alone shows.
    pub always_show_focus_ring: bool,
    /// When true, this node is a pointer target for the library's
    /// text-selection manager: pointer-down inside its rect starts (or
    /// extends) the global [`crate::selection::Selection`] anchored at
    /// this node's `key`. The leaf must also carry an explicit
    /// `.key(...)` — same convention as focusable widgets — so the
    /// selection survives tree rebuilds.
    ///
    /// Set via [`Self::selectable`]. Coordinates with focus on a
    /// per-pointer-event basis: pointer-down on a focusable widget
    /// transfers focus and clears selection; pointer-down on a
    /// selectable-only leaf moves selection without disturbing focus.
    pub selectable: bool,
    /// When true, a touch contact starting on this node (or any
    /// descendant) is treated as a drag rather than a pan/scroll
    /// gesture. The runner's touch-scroll synthesis defaults to
    /// "scroll wins" on touch — every interactive widget loses
    /// touch drag to the nearest enclosing scrollable, matching
    /// platform behavior where you can pan over a button. Widgets
    /// that legitimately need a touch drag — sliders, scrubbers,
    /// resize handles — opt in here so the runner commits to drag
    /// instead of cancelling the press.
    ///
    /// Inherits along the ancestor path: a press on a slider's
    /// thumb child consumes touch drag if the slider's outer
    /// surface set the flag. Has no effect on mouse / pen pointers
    /// — those follow the historical "press + move = drag" model.
    pub consumes_touch_drag: bool,
    /// Optional source-backed selection payload. Plain text leaves
    /// select/copy their rendered [`Self::text`]. Rich text systems can
    /// attach a [`crate::selection::SelectionSource`] so pointer
    /// positions resolve through rendered text but copy returns the
    /// original driving syntax (for example Markdown or TeX).
    pub selection_source: Option<crate::selection::SelectionSource>,
    /// When true, all key events (other than registered hotkeys) route
    /// to this node as raw `KeyDown` instead of being interpreted by
    /// the library's defaults (Tab traversal, Enter/Space activation,
    /// Escape escape). Used by text-input widgets that need to consume
    /// Tab/Enter/etc. as text or editing actions. Implies `focusable`
    /// at the runner — the flag only takes effect when the node is
    /// also the focused target.
    pub capture_keys: bool,
    /// When true, this node's paint opacity is multiplied by the
    /// nearest focusable ancestor's focus envelope (0..1). The library
    /// already animates that envelope on focus / blur; flagged nodes
    /// fade in and out with the same easing without any app-side
    /// focus tracking.
    ///
    /// Used by `text_input`'s caret bar — the caret only paints when
    /// the input is focused, fading via the standard focus animation.
    /// Documented in `widget_kit.md` as part of the public surface.
    pub alpha_follows_focused_ancestor: bool,
    /// When true, this node's paint opacity is also multiplied by the
    /// runtime's caret blink alpha. Combine with
    /// `alpha_follows_focused_ancestor` (the caret should blink only
    /// while the input is focused) — the two compose multiplicatively.
    /// Used by `text_input` / `text_area`'s caret bar.
    pub blink_when_focused: bool,
    /// When true, this node's hover and press visual envelopes are
    /// borrowed from its nearest focusable ancestor instead of being
    /// driven by its own (always-zero) envelope.
    ///
    /// The hit-test only ever resolves to a focusable target, so a
    /// child of an interactive container — a slider thumb, a select
    /// trigger's chevron, the dot inside a radio — never receives
    /// hover or press envelopes of its own. Flagged children pick up
    /// the ancestor's envelopes so they can lighten / darken / ring
    /// out alongside the surface that captured the input.
    ///
    /// Used by `slider`'s thumb so grabbing the slider visibly
    /// reacts on the thumb itself, mirroring shadcn's
    /// `hover:ring-4 hover:ring-ring/50`.
    pub state_follows_interactive_ancestor: bool,
    /// When `Some`, this node's paint opacity is bound to the
    /// **subtree interaction envelope** — `max` of the hover, focus,
    /// and press envelopes for the subtree rooted here. The drawn
    /// alpha interpolates from `rest` (no interaction anywhere in the
    /// subtree) to `peak` (full interaction), then composes
    /// multiplicatively with the existing [`Self::opacity`] /
    /// inherited opacity stack.
    ///
    /// "Interaction" includes hovering, pressing, or keyboard-focusing
    /// any descendant — so a hover-revealed close icon stays visible
    /// when its tab is keyboard-focused, and an action pill stays
    /// visible when the cursor moves to one of its focusable buttons.
    /// Mirrors CSS's "this element OR any descendant is hot."
    ///
    /// Layout-neutral — the element's geometry stays fixed regardless
    /// of interaction state. Use for hover-revealed close buttons,
    /// secondary actions on list rows, hover-only validation icons,
    /// and other "show on interaction" patterns whose visibility
    /// shouldn't shift the surrounding layout.
    pub hover_alpha: Option<HoverAlpha>,
    /// Call-site attribution recorded by the `#[track_caller]`
    /// constructors; the bundle lint pass uses it to blame findings on
    /// user code rather than library internals. See [`Source`].
    pub source: Source,

    /// Lint kinds the bundle's lint pass should treat as intentional on
    /// this node. Each entry suppresses one
    /// [`crate::bundle::lint::FindingKind`] on this specific node only —
    /// it does **not** propagate to descendants, and it only matches
    /// findings whose attribution target *is* this node. Authored via
    /// [`Self::allow_lint`]. Whole-class or pattern-based suppression
    /// (e.g. `DuplicateId`, which has no per-node target) is the
    /// province of [`crate::bundle::lint::LintReport::retain`].
    ///
    /// Stock widgets do not call [`Self::allow_lint`]; the showcase
    /// fixture does not call it either (it is a dogfood standard that
    /// every showcase lint be addressed in widgets / layout rather than
    /// silenced). User apps may opt out per-node when a finding is
    /// genuinely intentional in their domain.
    pub allow_lint: Vec<crate::bundle::lint::FindingKind>,

    // Layout
    /// Direction children are laid out: column, row, or overlay
    /// (children share this node's rect). Defaults to [`Axis::Overlay`];
    /// the `column()` / `row()` constructors set it.
    pub axis: Axis,
    /// Space between consecutive children along [`Self::axis`], in
    /// logical pixels (CSS flexbox `gap`). Default `0.0`.
    pub gap: f32,
    /// Inner padding between this node's edges and its content, in
    /// logical pixels (CSS `padding`).
    pub padding: Sides,
    /// Cross-axis alignment of children (CSS `align-items`). Default
    /// [`Align::Stretch`].
    pub align: Align,
    /// Main-axis distribution of children (CSS `justify-content`).
    /// Default [`Justify::Start`].
    pub justify: Justify,
    /// Width sizing policy — hug content (default), fill the parent,
    /// fixed logical pixels, or aspect-derived. See [`Size`].
    pub width: Size,
    /// Height sizing policy — hug content (default), fill the parent,
    /// fixed logical pixels, or aspect-derived. See [`Size`].
    pub height: Size,
    /// Optional lower bound on the resolved width in logical pixels.
    /// Layout clamps the final width up to this value after `width`
    /// resolves (Hug, Fill, or even Fixed) — useful for "this card
    /// shrinks to fit content but never below 200px." A `Hug` parent
    /// of a min-clamped child reflects the clamped intrinsic so the
    /// parent grows accordingly. Composes additively with
    /// [`Self::max_width`] and conflict-resolves to the lower bound
    /// when both apply.
    pub min_width: Option<f32>,
    /// Optional upper bound on the resolved width in logical pixels.
    /// Layout clamps the final width down to this value after `width`
    /// resolves. Useful for capping a `Fill` child so it doesn't grow
    /// past a readable column width. Space a clamped `Fill` frees (or
    /// steals, for `min_width`) is re-distributed among its sibling
    /// fills; with none left flexible it goes to the parent's
    /// `justify`, matching CSS flex. Conflict-resolves with
    /// [`Self::min_width`] in favour of the lower bound when both
    /// apply (matches CSS `min-width` over `max-width` precedence).
    pub max_width: Option<f32>,
    /// Optional lower bound on the resolved height in logical pixels.
    /// See [`Self::min_width`] for the semantic.
    pub min_height: Option<f32>,
    /// Optional upper bound on the resolved height in logical pixels.
    /// See [`Self::max_width`] for the semantic.
    pub max_height: Option<f32>,
    /// Optional t-shirt size for stock widgets. `None` means the active
    /// theme supplies the component-class default.
    pub component_size: Option<ComponentSize>,
    /// Optional theme-facing metrics role. Stock widgets set this so
    /// the theme can resolve default height/padding/radius before
    /// layout; app-defined widgets can set the same role to opt into
    /// identical sizing behavior.
    pub metrics_role: Option<MetricsRole>,
    /// Author-overrode layout metrics. Stock constructors set defaults
    /// without these flags; public modifiers flip them so theme metrics
    /// do not clobber explicit app choices.
    pub explicit_width: bool,
    /// Author explicitly set [`Self::height`]; theme metrics leave it alone.
    pub explicit_height: bool,
    /// Author explicitly set [`Self::padding`]; theme metrics leave it alone.
    pub explicit_padding: bool,
    /// Author explicitly set [`Self::gap`]; theme metrics leave it alone.
    pub explicit_gap: bool,
    /// Author explicitly set [`Self::radius`]; theme metrics leave it alone.
    pub explicit_radius: bool,
    /// Author explicitly set [`Self::font_family`]; theme application
    /// leaves it alone.
    pub explicit_font_family: bool,
    /// Author overrode the monospace font face for this node — theme
    /// application leaves [`Self::mono_font_family`] alone when set.
    pub explicit_mono_font_family: bool,
    /// Author opted this node into the monospace family via
    /// [`Self::mono`]. Role modifiers ([`Self::caption`], [`Self::label`],
    /// [`Self::body`], [`Self::title`], [`Self::heading`],
    /// [`Self::display`]) leave [`Self::font_mono`] alone when this flag
    /// is set, so the natural reading order `text(s).mono().caption()`
    /// keeps the mono family. Without this guard, role application
    /// silently resets `font_mono = false`. The [`Self::code`] role
    /// always forces `font_mono = true` regardless.
    pub explicit_mono: bool,

    // Visual style — these still live on `El` because the modifier API
    // (`.fill(c)`, `.radius(r)`, `.shadow(s)`) is what users type. The
    // renderer translates them into a [`ShaderBinding`] for
    // `stock::rounded_rect` (or whatever `shader_override` specifies)
    // when emitting [`crate::ir::DrawOp`]s.
    /// Background fill color. `None` paints no fill of its own — though
    /// a fill-providing [`Self::surface_role`] may default one from the
    /// palette.
    pub fill: Option<Color>,
    /// Alternate fill used when the nearest focusable ancestor's focus
    /// envelope is below 1.0; the painter linearly interpolates from
    /// `dim_fill` toward `fill` as the envelope approaches 1.0. Used by
    /// `text_input` / `text_area` selection bands so the highlight
    /// remains visible (in a muted color) even when the input loses
    /// focus, matching the macOS convention.
    pub dim_fill: Option<Color>,
    /// Border color, painted at [`Self::stroke_width`]. `None` means no
    /// border.
    pub stroke: Option<Color>,
    /// Border width in logical pixels; `0.0` (the default) paints no
    /// border.
    pub stroke_width: f32,
    /// Corner radii in logical pixels. Authored as a scalar in the
    /// common case (`.radius(tokens::RADIUS_MD)` works via
    /// [`super::geometry::Corners::from`]); per-corner shapes use
    /// [`super::geometry::Corners::top`],
    /// [`super::geometry::Corners::bottom`], etc. The painter clamps each corner to
    /// half the shorter side.
    pub radius: super::geometry::Corners,
    /// Drop-shadow size in logical pixels (`tokens::SHADOW_SM` /
    /// `SHADOW_MD` / `SHADOW_LG` = 4 / 12 / 24). `0.0` (the default)
    /// paints no shadow; positive values feed the surface shader's
    /// `shadow` uniform and position the visual in the
    /// [`Self::paint_overflow`] band.
    pub shadow: f32,
    /// Semantic paint role for this surface — the theme applies a
    /// stroke / shadow / fill recipe per role at paint time. See
    /// [`SurfaceRole`].
    pub surface_role: SurfaceRole,
    /// Permit this element to paint outside its layout bounds. The
    /// outset enlarges the quad geometry handed to the shader (and
    /// any focus / shadow / glow visuals are positioned in the
    /// overflow band) while leaving the layout rect — and therefore
    /// sibling positions and hit-testing — unchanged. Subject to
    /// ancestor clip rects: a focused widget inside a `clip()`ped
    /// parent has its overflow clipped, same as any other paint.
    pub paint_overflow: Sides,
    /// Clip this element's own paint and descendants to its computed rect.
    /// Used by scroll panes, host-painted regions, overlays, and any region
    /// where overflow should not leak visually or receive events.
    pub clip: bool,
    /// This element is a vertical scroll viewport. The layout pass reads
    /// the offset from `UiState`'s scroll-offset side map keyed by
    /// `computed_id`, clamps it to `[0, content_h - viewport_h]`, and
    /// writes the clamped value back. Set automatically by [`crate::scroll()`].
    pub scrollable: bool,
    /// Which edge of a [`Kind::Scroll`] container the offset sticks to
    /// when engaged. `None` (default) means the stored offset is the
    /// only source of truth — content changes do not shift it. `End`
    /// matches egui's `ScrollArea::stick_to_bottom(true)`: the offset
    /// stays glued to the tail across content growth (chat-log idiom).
    /// `Start` is the symmetric "stick to head" — useful for virtual
    /// lists that grow at the top (commit logs / activity-feed reverse
    /// order) so newly added rows stay visible. No effect on
    /// non-scrollable nodes. Opt in with [`Self::pin_start`] or
    /// [`Self::pin_end`].
    ///
    /// The "is the pin currently engaged" bit lives in
    /// [`crate::state::UiState`]'s scroll subsystem, keyed by
    /// `computed_id`; layout reads it each frame to decide whether to
    /// snap the stored offset to the pinned edge before clamping.
    pub pin_policy: crate::tree::PinPolicy,
    /// Treat this element's focusable children as a single arrow-navigable
    /// group: while a focused element is one of the direct children
    /// (focusable *descendants* for [`ArrowNav::Grid`]), the arrows the
    /// mode covers plus `Home` / `End` move focus within the group
    /// instead of being routed as a `KeyDown`. Tab traversal is
    /// unchanged.
    ///
    /// Used by `popover_panel` / `dropdown_menu_content` /
    /// `menubar_content` (`Vertical`), `tabs_list` / `toggle_group`
    /// (`Horizontal`), `radio_group` (`Both`), and `calendar_month`'s
    /// day grid (`Grid`); available to any user widget that wants the
    /// same semantics. Set via [`Self::arrow_nav`] or the no-arg
    /// vertical shorthand [`Self::arrow_nav_siblings`].
    ///
    /// [`ArrowNav::Grid`]: crate::tree::ArrowNav::Grid
    pub arrow_nav: Option<crate::tree::ArrowNav>,
    /// Tooltip text. When set, the runtime synthesizes a hover-driven
    /// tooltip layer anchored to this node — appearing after the
    /// hover delay elapses, fading in with the standard envelope, and
    /// dismissed when the pointer leaves or presses the node. The
    /// trigger doesn't have to be focusable or keyed; the runtime
    /// anchors the tooltip via the trigger's `computed_id`.
    pub tooltip: Option<String>,
    /// Pointer cursor declared for this element. `None` falls through
    /// to whatever an ancestor declared, else [`crate::cursor::Cursor::Default`].
    /// Resolution lives in [`crate::state::UiState::cursor`]: if a
    /// press is captured, the cursor follows the press target;
    /// otherwise the hovered node is walked root-ward for the first
    /// explicit declaration. The `.disabled()` chainable declares
    /// [`crate::cursor::Cursor::NotAllowed`] here as part of its
    /// treatment.
    pub cursor: Option<crate::cursor::Cursor>,
    /// Cursor to show *only while a press is captured at this exact
    /// node*. Powers the natural Grab → Grabbing transition: the
    /// slider sets `cursor=Grab` + `cursor_pressed=Grabbing`, and the
    /// resolver picks the latter while the press anchors here. Unlike
    /// [`Self::cursor`], this does **not** walk up: an ancestor's
    /// `cursor_pressed` doesn't apply to a descendant press target.
    /// The press target's own `cursor` is the fallback when this is
    /// `None`.
    pub cursor_pressed: Option<crate::cursor::Cursor>,
    /// Override the implicit `stock::rounded_rect` binding for this
    /// node's surface. The escape hatch a user crate uses to bind a
    /// custom shader (e.g. `liquid_glass`).
    pub shader_override: Option<ShaderBinding>,
    /// Second escape hatch: author-supplied layout function that
    /// positions this node's direct children. When set, the layout
    /// pass calls the function instead of running its column/row/
    /// overlay distribution. The library still recurses into each
    /// child and still drives hit-test / focus / animation / scroll
    /// off the rects the function returns. See [`LayoutFn`] for the
    /// contract.
    pub layout_override: Option<LayoutFn>,
    /// Virtualized list state. Set by [`crate::virtual_list`] (and only
    /// on `Kind::VirtualList` nodes). The layout pass uses this to
    /// realize only the rows whose rect intersects the viewport. The
    /// node is automatically `scrollable` + `clip`.
    pub virtual_items: Option<VirtualItems>,
    /// Show a draggable vertical scrollbar thumb when this node is
    /// scrollable and its content overflows the viewport. The thumb
    /// overlays the right edge of the viewport — it does not reflow
    /// children. No effect on non-scrollable nodes. Defaults to
    /// `false`; the [`crate::scroll()`] and [`crate::virtual_list()`]
    /// constructors flip it on by default. Authors disable with
    /// [`Self::no_scrollbar`].
    pub scrollbar: bool,
    /// Reserve a right-edge gutter for the scrollbar thumb so content
    /// (focus rings included) never sits underneath it — the CSS
    /// `scrollbar-gutter: stable` shape. Resolved by the metrics pass
    /// into extra right padding of [`crate::tokens::SCROLLBAR_GUTTER`]
    /// on top of whatever padding the node has, so it composes with
    /// `.padding(...)` in any call order. Set via
    /// [`Self::scrollbar_gutter`]; default `false`.
    pub scrollbar_gutter: bool,

    // Text
    /// Text run rendered by this node, if any. Set by the `text(...)` /
    /// `button(...)` / `heading(...)` constructors and the
    /// [`Self::text`] modifier.
    pub text: Option<String>,
    /// Text color override. `None` falls through to the theme-resolved
    /// default for the node's role / surface.
    pub text_color: Option<Color>,
    /// Horizontal alignment of the text run within the resolved rect.
    /// Default [`TextAlign::Start`].
    pub text_align: TextAlign,
    /// Line-wrapping policy. Default [`TextWrap::NoWrap`]; opt in via
    /// `.wrap_text()`.
    pub text_wrap: TextWrap,
    /// What happens to text exceeding the rect: hard clip (default) or
    /// `…` ellipsis. See [`TextOverflow`].
    pub text_overflow: TextOverflow,
    /// Semantic typography role. The role modifiers (`.caption()`,
    /// `.title()`, `.code()`, …) set it and stamp the matching size /
    /// weight. See [`TextRole`].
    pub text_role: TextRole,
    /// Optional cap on wrapped line count (effective with
    /// [`TextWrap::Wrap`]); the final kept line is ellipsized. Set via
    /// [`Self::max_lines`], which clamps to at least 1.
    pub text_max_lines: Option<usize>,
    /// Font size in logical pixels. Default `tokens::TEXT_SM.size` (14).
    pub font_size: f32,
    /// Line height in logical pixels. Default
    /// `tokens::TEXT_SM.line_height` (20); the [`Self::font_size`]
    /// modifier re-derives it from the size scale.
    pub line_height: f32,
    /// Proportional font family for this node's text. Theme-stamped
    /// unless [`Self::explicit_font_family`] is set.
    pub font_family: FontFamily,
    /// Monospace face used when [`Self::font_mono`] is set (or when the
    /// node carries [`TextRole::Code`]). Stamped by theme application
    /// from [`crate::Theme::mono_font_family`] unless the author set it
    /// explicitly via [`Self::mono_font_family`].
    pub mono_font_family: FontFamily,
    /// Font weight (regular / medium / semibold / bold). Default
    /// [`FontWeight::Regular`].
    pub font_weight: FontWeight,
    /// Render the text in the monospace family
    /// ([`Self::mono_font_family`]). Set via [`Self::mono`];
    /// [`TextRole::Code`] forces it on.
    pub font_mono: bool,
    /// Italic styling. Author-set via [`Self::italic`]; honoured when
    /// this El is a styled text leaf inside an [`Kind::Inlines`] parent
    /// and (best-effort) on standalone text Els.
    pub text_italic: bool,
    /// Underline styling. Author-set via [`Self::underline`].
    pub text_underline: bool,
    /// Strikethrough styling. Author-set via [`Self::strikethrough`].
    pub text_strikethrough: bool,
    /// Link target URL. When set on a text leaf inside [`Kind::Inlines`],
    /// the run renders as a link (themed) and runs sharing a URL group
    /// together for hit-test. Author-set via [`Self::link`].
    pub text_link: Option<String>,
    /// Inline-run background. When set on a text leaf inside
    /// [`Kind::Inlines`], the shaped span paints a solid quad behind
    /// its glyphs (one rect per line if the span wraps). No effect on
    /// standalone text Els — author wraps in a styled `row()` for
    /// chip-shaped surfaces. Author-set via [`Self::background`].
    pub text_bg: Option<Color>,

    // Math
    /// Native math expression rendered through Damascene's math box layout.
    /// Set by [`crate::tree::math`], [`crate::tree::math_inline`], and
    /// [`crate::tree::math_block`].
    pub math: Option<std::sync::Arc<MathExpr>>,
    /// Inline vs block layout for [`Self::math`]. Default
    /// [`MathDisplay::Inline`]; [`crate::tree::math_block`] sets `Block`.
    pub math_display: MathDisplay,

    // Icon
    /// Icon to render — a built-in [`crate::IconName`] or an
    /// app-supplied [`crate::SvgIcon`]. Set via [`Self::icon_source`].
    pub icon: Option<crate::icons::svg::IconSource>,
    /// Stroke width applied to [`Self::icon`]'s paths, in the icon's
    /// own viewBox units (lucide convention). Default `2.0`; the
    /// [`Self::icon_stroke_width`] modifier clamps to at least `0.25`.
    pub icon_stroke_width: f32,

    /// Raster image. When set together with [`Kind::Image`] (or any
    /// kind, though [`crate::image`] is the idiomatic builder) the
    /// `draw_ops` pass emits a [`crate::ir::DrawOp::Image`] projected
    /// per [`Self::image_fit`] and tinted by [`Self::image_tint`].
    /// Layout intrinsic is the image's natural pixel size when both
    /// `width` and `height` are `Hug`.
    pub image: Option<Image>,
    /// Multiply each sampled pixel by this colour (RGBA `[0..1]`). Most
    /// raster art wants `None` (no tint); set it for monochrome assets
    /// (icon-style PNGs) the app wants to recolour.
    pub image_tint: Option<Color>,
    /// How the image projects into the resolved rect. Defaults to
    /// `ImageFit::Contain` — preserves aspect ratio and letterboxes.
    pub image_fit: ImageFit,
    /// How much of the output's HDR headroom the image may use
    /// (mirrors CSS `dynamic-range-limit`). Defaults to
    /// [`crate::image::DynamicRangeLimit::NoLimit`] — full headroom,
    /// remastered to fit the panel.
    pub image_range_limit: crate::image::DynamicRangeLimit,

    /// App-owned GPU texture source for [`Kind::Surface`] elements.
    /// Set via [`Self::surface_source`] (typically through the
    /// [`crate::tree::surface`] builder).
    pub surface_source: Option<crate::surface::SurfaceSource>,
    /// How the surface texture composes with widgets painted below it.
    /// Defaults to [`crate::surface::SurfaceAlpha::Premultiplied`].
    pub surface_alpha: crate::surface::SurfaceAlpha,
    /// How the surface texture projects into the resolved rect.
    /// Defaults to [`ImageFit::Fill`] — stretch to the rect, ignoring
    /// aspect ratio. `Contain` / `Cover` / `None` mirror the
    /// corresponding modes on [`crate::tree::image`].
    pub surface_fit: ImageFit,
    /// Affine applied to the texture quad in destination space, around
    /// the centre of the post-fit rect. Defaults to identity.
    /// Composes after [`Self::surface_fit`]: the fit projection picks
    /// the destination rect, then this matrix transforms it (rotate,
    /// scale, translate, shear). The auto-clip scissor still clamps
    /// to the El's content rect, so transforms that move the texture
    /// outside that rect are cropped.
    pub surface_transform: crate::affine::Affine2,

    /// Scene specification for [`Kind::Scene3D`] elements. Set via the
    /// [`crate::tree::chart3d`] builder. `draw_ops` resolves it (camera
    /// auto-framed against the marks' bounds) into a `DrawOp::Scene3D`.
    /// Boxed to keep `El` small — most Els carry no scene.
    pub scene_source: Option<Box<crate::scene::SceneSpec>>,

    /// Vector asset for [`Kind::Vector`] elements. Set via
    /// [`Self::vector_source`] (typically through the
    /// [`crate::tree::vector`] builder). The asset's view box determines
    /// the natural aspect ratio.
    pub vector_source: Option<std::sync::Arc<crate::vector::VectorAsset>>,
    /// Render policy for [`Self::vector_source`]. Defaults to
    /// [`crate::vector::VectorRenderMode::Painted`] so authored vector
    /// paint is preserved unless the caller explicitly opts into mask
    /// rendering.
    pub vector_render_mode: crate::vector::VectorRenderMode,

    /// Child elements, laid out along [`Self::axis`].
    pub children: Vec<El>,

    /// Paint-time alpha multiplier in `[0, 1]`. Default `1.0`. Multiplies
    /// the alpha channel of `fill`, `stroke`, and text colour at draw
    /// time. Layout-neutral. App-driven changes are eased when
    /// [`Self::animate`] is set.
    pub opacity: f32,
    /// Paint-time offset in logical pixels. Default `(0.0, 0.0)`.
    /// **Subtree-inheriting**: descendants paint at their computed rect
    /// plus all ancestor `translate` accumulated through the paint
    /// recursion. Use this to slide a sidebar / drawer / list-item
    /// without re-running layout. App-driven changes are eased when
    /// [`Self::animate`] is set.
    pub translate: (f32, f32),
    /// Per-node uniform scale around the computed-rect centre. Default
    /// `1.0`. Scales this node's surface quad and (if it carries text)
    /// its glyph run together. **Not** subtree-inheriting — descendants
    /// keep their own scale. Use this for tap-bounce on a button. App-
    /// driven changes are eased when [`Self::animate`] is set.
    pub scale: f32,
    /// Opt-in app-driven prop interpolation. When `Some(timing)`, the
    /// animation tracker eases `fill` / `text_color` / `stroke` /
    /// `opacity` / `translate` / `scale` between rebuilds — the value
    /// the build closure produces becomes the spring/tween target;
    /// `current` carries over from last frame. State visuals (hover /
    /// press / focus ring) keep their own library defaults regardless.
    pub animate: Option<Timing>,

    /// Inside-out redraw deadline: when `Some(d)` and this El is
    /// visible (rect intersects the viewport), Damascene asks the host to
    /// schedule the next frame within `d`. Aggregated across the tree
    /// via `min` and surfaced as
    /// [`crate::runtime::PrepareResult::next_redraw_in`]; the host
    /// drives the loop, Damascene mediates by visibility.
    ///
    /// Use this for any widget whose paint depends on time (animated
    /// images, video frames written via `surface()`, custom shaders
    /// that don't go through the `samples_time` registration path,
    /// hover-and-fade effects implemented outside the built-in
    /// animation tracker). `Duration::ZERO` means "next frame ASAP";
    /// non-zero values let the host pace at lower-than-display
    /// cadence.
    pub redraw_within: Option<std::time::Duration>,

    /// Stable path-based ID, filled by the layout pass. Used as the
    /// key for every side map that holds per-node bookkeeping in
    /// [`crate::state::UiState`] — computed rects, interaction state,
    /// state-envelope amounts, scroll offsets, in-flight animations.
    pub computed_id: String,
}
