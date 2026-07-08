//! CSS subset parser shared across tier-2A (inline `style="..."`),
//! tier-2B (`<style>` block declarations), and tier-2D (layout
//! reconciliation + lint).
//!
//! Property coverage:
//!
//! - Visual — `color`, `background` / `background-color`, `padding`
//!   (shorthand + per-side), `border` / `border-color` /
//!   `border-width`, `border-radius`, `opacity`, `box-shadow` (blur
//!   extracted, offset / spread / color dropped).
//! - Layout sizing — `width`, `height`, `min/max-width/height`.
//! - Layout reshape — `display: flex` + `flex-direction`,
//!   `align-items`, `justify-content`, `overflow` (`hidden` → clip,
//!   `auto` / `scroll` → wrap in `scroll(...)`).
//! - Parent rhythm — `margin` (shorthand + per-side); not applied
//!   here, projected into parent `gap` / `padding` by
//!   [`crate::transform::walk_block_children`].
//! - Text — `text-align`, `font-size`, `font-weight`, `font-style`,
//!   `text-decoration`, `font-family` (monospace detection only).
//!
//! Outcome reporting:
//!
//! - Malformed values (`padding: not-a-length`) drop silently — the
//!   surrounding declarations still apply.
//! - Unsupported units (`10vw`, `4fr`, `12vh`, …) drop with a
//!   [`crate::FindingKind::DroppedDeclaration`] finding.
//! - Properties whose semantics don't translate (`position: absolute`,
//!   `float: left`, `display: grid`, baseline alignment, …) drop with
//!   the same finding kind.
//! - Unknown properties (vendor prefixes, custom properties, CSS
//!   experiments) drop silently — flooding the lint surface with them
//!   tells the author nothing actionable.

use damascene_core::prelude::*;
use markup5ever_rcdom::{Handle, NodeData};

use crate::lints::{FindingKind, Lints};
use crate::sanitize::is_blocked_attr;

/// Container-layout properties parsed from CSS that change the El's
/// shape (axis, alignment, overflow). Distinct from
/// [`ComputedStyle`]'s visual / sizing fields because they only apply
/// to container-tagged elements (`<div>`, `<section>`, …) — applying
/// them to a `<p>` or `<h1>` would be nonsense, so the dispatcher
/// reads these via [`ComputedStyle::layout_overrides`] only at the
/// container arms.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutOverrides {
    /// `display: flex` → `Some(true)` (use `flex_axis` + alignment).
    /// `display: block` / `inline-block` → `Some(false)` (default
    /// column flow). Unsupported values (`grid`, `table`, …) emit a
    /// lint at parse time and leave this `None`.
    pub display_is_flex: Option<bool>,
    /// CSS `flex-direction` → Damascene [`Axis`]. Only consulted when
    /// `display_is_flex == Some(true)`.
    pub flex_axis: Option<Axis>,
    /// CSS `align-items` → Damascene [`Align`].
    pub align: Option<Align>,
    /// CSS `justify-content` → Damascene [`Justify`].
    pub justify: Option<Justify>,
    /// CSS `overflow` (shorthand) collapsed into one value — `hidden`
    /// → `Some(Hidden)`, `auto` / `scroll` → `Some(Scroll)`, `visible`
    /// → unset.
    pub overflow: Option<Overflow>,
}

/// Reduced CSS `overflow` model: only the values that map onto Damascene
/// primitives (`clip()` and `scroll([...])`). `visible` and `clip`
/// (the CSS keyword, not Damascene's `.clip()` modifier) collapse to
/// "unset" since neither needs container reshape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overflow {
    Hidden,
    Scroll,
}

/// Typed CSS-shaped size value: pixels, percentage-of-parent, or
/// `auto`. Converts to Damascene [`Size`] at apply time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CssSize {
    /// Concrete logical pixels (`200px`, `1.5rem`, `12pt`).
    Px(f32),
    /// Fraction of parent extent (`50%` → 0.5). Maps to `Size::Fill`.
    Percent(f32),
    /// Intrinsic size (`auto`). Maps to `Size::Hug`.
    Auto,
}

impl CssSize {
    fn into_damascene(self) -> Size {
        match self {
            CssSize::Px(v) => Size::Fixed(v),
            CssSize::Percent(frac) => Size::Fill(frac),
            CssSize::Auto => Size::Hug,
        }
    }
}

/// Flattened style for one element. Every field is `Option` so the
/// applier only touches fields the source declared — defaults from
/// the El's constructor stay intact for unspecified properties.
///
/// Authors mutate via the parser ([`parse_inline_style`]); consumers
/// project through [`ComputedStyle::apply_to_block`] (visual / sizing /
/// text) or [`ComputedStyle::apply_container_layout`] +
/// [`ComputedStyle::wrap_with_overflow`] (layout reshape), or fold into
/// `crate::transform::InlineState`.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct ComputedStyle {
    // Visual
    pub text_color: Option<Color>,
    pub background: Option<Color>,
    pub padding: Option<Sides>,
    pub border_color: Option<Color>,
    pub border_width: Option<f32>,
    pub border_radius: Option<f32>,
    pub opacity: Option<f32>,

    // Layout sizing
    pub width: Option<CssSize>,
    pub height: Option<CssSize>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,

    // Text
    pub text_align: Option<TextAlign>,
    pub font_size: Option<f32>,
    pub font_weight: Option<FontWeight>,
    /// `Some(true)` ↔ `font-style: italic` / `oblique`; `Some(false)`
    /// ↔ explicit `font-style: normal`. Unset stays unset so the
    /// applier doesn't clobber an outer `<em>`'s italic flag.
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    /// `font-family` parsed and classified as monospace (`monospace`,
    /// `mono`, `courier`, `consolas`, `menlo`, …). `Some(true)` →
    /// flip the El to mono. Non-mono families parse but lint as
    /// dropped because Damascene doesn't expose per-element font-family
    /// pinning beyond the mono toggle.
    pub font_mono: Option<bool>,

    // Tier-2D — layout reconciliation
    /// CSS `margin` values per side. Reconciled into parent `gap`
    /// (between siblings) and parent `padding` (first / last child's
    /// outer edge) by [`crate::transform::walk_block_children`]. The
    /// `apply_to_block` path never reads margin — it's a parent-side
    /// concern.
    pub margin: Option<Sides>,
    /// CSS `box-shadow` parsed best-effort to a single blur-radius
    /// scalar, fed to Damascene's `shadow(f32)` elevation level.
    /// Offset / spread / color are dropped — the renderer expands the
    /// level through `paint::shadow::shadow_recipe`, whose anchors sit
    /// on Tailwind's blur values, so Tailwind-generated CSS
    /// (`shadow-sm` blur 3, `shadow-md` blur 6, `shadow-lg` blur 15)
    /// lands near its intended tier.
    pub box_shadow: Option<f32>,
    /// Container-layout overrides (display/flex/align/justify/overflow).
    /// Only read by container-tag dispatchers; leaf-tag dispatchers
    /// (`<p>`, `<h1>`, `<img>`) ignore these fields.
    pub layout: LayoutOverrides,
}

impl ComputedStyle {
    /// `true` when no declaration on the source element produced any
    /// value. Block builders use this to skip the wrap-in-column step
    /// for generic containers (`<div>`, `<section>`, …) whose only
    /// purpose was structural grouping.
    pub(crate) fn is_empty(&self) -> bool {
        *self == ComputedStyle::default()
    }

    /// Layer `other` on top of `self`: every field `other` declares
    /// (`Some(...)`) overwrites the corresponding field on `self`;
    /// fields `other` leaves unset stay as they were. The cascade
    /// engine calls this once per matching rule, ordered from lowest
    /// to highest priority, so the highest-priority declaration is
    /// the last to land.
    pub(crate) fn merge(&mut self, other: &ComputedStyle) {
        if other.text_color.is_some() {
            self.text_color = other.text_color;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.padding.is_some() {
            self.padding = other.padding;
        }
        if other.border_color.is_some() {
            self.border_color = other.border_color;
        }
        if other.border_width.is_some() {
            self.border_width = other.border_width;
        }
        if other.border_radius.is_some() {
            self.border_radius = other.border_radius;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        if other.width.is_some() {
            self.width = other.width;
        }
        if other.height.is_some() {
            self.height = other.height;
        }
        if other.min_width.is_some() {
            self.min_width = other.min_width;
        }
        if other.max_width.is_some() {
            self.max_width = other.max_width;
        }
        if other.min_height.is_some() {
            self.min_height = other.min_height;
        }
        if other.max_height.is_some() {
            self.max_height = other.max_height;
        }
        if other.text_align.is_some() {
            self.text_align = other.text_align;
        }
        if other.font_size.is_some() {
            self.font_size = other.font_size;
        }
        if other.font_weight.is_some() {
            self.font_weight = other.font_weight;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strikethrough.is_some() {
            self.strikethrough = other.strikethrough;
        }
        if other.font_mono.is_some() {
            self.font_mono = other.font_mono;
        }
        if other.margin.is_some() {
            self.margin = other.margin;
        }
        if other.box_shadow.is_some() {
            self.box_shadow = other.box_shadow;
        }
        if other.layout.display_is_flex.is_some() {
            self.layout.display_is_flex = other.layout.display_is_flex;
        }
        if other.layout.flex_axis.is_some() {
            self.layout.flex_axis = other.layout.flex_axis;
        }
        if other.layout.align.is_some() {
            self.layout.align = other.layout.align;
        }
        if other.layout.justify.is_some() {
            self.layout.justify = other.layout.justify;
        }
        if other.layout.overflow.is_some() {
            self.layout.overflow = other.layout.overflow;
        }
    }

    /// Apply the visual + layout sizing fields to a block-level El.
    /// Text fields are applied too, for elements (`<p>`, `<h1>`, etc.)
    /// whose body is a single text leaf — they're a no-op on `Inlines`
    /// containers (where text fields ride on the per-run state
    /// instead).
    pub(crate) fn apply_to_block(&self, mut el: El) -> El {
        if let Some(c) = self.text_color {
            el = el.text_color(c);
        }
        if let Some(c) = self.background {
            el = el.fill(c);
        }
        if let Some(p) = self.padding {
            el = el.padding(p);
        }
        if let Some(w) = self.border_width {
            el = el.stroke_width(w);
        }
        if let Some(c) = self.border_color {
            el = el.stroke(c);
        }
        if let Some(r) = self.border_radius {
            el = el.radius(r);
        }
        if let Some(o) = self.opacity {
            el = el.opacity(o.clamp(0.0, 1.0));
        }
        if let Some(w) = self.width {
            el = el.width(w.into_damascene());
        }
        if let Some(h) = self.height {
            el = el.height(h.into_damascene());
        }
        if let Some(v) = self.min_width {
            el = el.min_width(v);
        }
        if let Some(v) = self.max_width {
            el = el.max_width(v);
        }
        if let Some(v) = self.min_height {
            el = el.min_height(v);
        }
        if let Some(v) = self.max_height {
            el = el.max_height(v);
        }
        if let Some(a) = self.text_align {
            el = el.text_align(a);
        }
        if let Some(s) = self.font_size {
            el = el.font_size(s);
        }
        if let Some(w) = self.font_weight {
            el = el.font_weight(w);
        }
        // `Some(false)` is a cancellation override (`font-style:
        // normal`, `text-decoration: none`) — it must clear an
        // inherited/tag-set flag, not no-op.
        match self.italic {
            Some(true) => el = el.italic(),
            Some(false) => el.text_italic = false,
            None => {}
        }
        match self.underline {
            Some(true) => el = el.underline(),
            Some(false) => el.text_underline = false,
            None => {}
        }
        match self.strikethrough {
            Some(true) => el = el.strikethrough(),
            Some(false) => el.text_strikethrough = false,
            None => {}
        }
        if let Some(true) = self.font_mono {
            el = el.mono();
        }
        if let Some(blur) = self.box_shadow {
            el = el.shadow(blur);
        }
        el
    }

    /// Apply the container-layout overrides (axis, alignment,
    /// justify) onto an already-constructed container El. Caller is
    /// responsible for picking the right baseline (`column` /
    /// `row`) — this only mutates the fields the CSS source declared.
    ///
    /// The `overflow` field is **not** applied here: turning a
    /// container into a scroll viewport is a wrap-not-mutate
    /// operation, so the caller projects it through
    /// [`Self::wrap_with_overflow`] separately.
    pub(crate) fn apply_container_layout(&self, mut el: El) -> El {
        if matches!(self.layout.display_is_flex, Some(true))
            && let Some(axis) = self.layout.flex_axis
        {
            el = el.axis(axis);
        }
        if let Some(a) = self.layout.align {
            el = el.align(a);
        }
        if let Some(j) = self.layout.justify {
            el = el.justify(j);
        }
        if matches!(self.layout.overflow, Some(Overflow::Hidden)) {
            el = el.clip();
        }
        el
    }

    /// `overflow: auto` / `scroll` wraps the container in `scroll(...)`
    /// so the viewport semantic comes from the wrapper rather than from
    /// modifying the container itself. Returns the unmodified `el`
    /// when the CSS source didn't declare a scrolling overflow.
    pub(crate) fn wrap_with_overflow(&self, el: El) -> El {
        if matches!(self.layout.overflow, Some(Overflow::Scroll)) {
            scroll([el])
        } else {
            el
        }
    }
}

/// Read an element's `style="..."` attribute and parse it. Returns a
/// default-empty [`ComputedStyle`] when the attribute is absent. The
/// `lints` collector receives a finding per dropped declaration
/// (unsupported property, unsupported unit, malformed value).
///
/// With `sanitize` set the attribute is dropped unparsed — untrusted
/// input doesn't get to style itself — and a [`FindingKind::SanitizedStyle`]
/// finding records the drop.
pub(crate) fn read_inline_style(node: &Handle, lints: &Lints, sanitize: bool) -> ComputedStyle {
    let Some(raw) = element_style_attr(node) else {
        return ComputedStyle::default();
    };
    if sanitize {
        lints.push(
            FindingKind::SanitizedStyle,
            format!("inline style dropped by sanitize_styles: \"{raw}\""),
        );
        return ComputedStyle::default();
    }
    parse_inline_style(&raw, lints)
}

fn element_style_attr(node: &Handle) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    for a in attrs.borrow().iter() {
        let name = a.name.local.as_ref();
        if name.eq_ignore_ascii_case("style") && !is_blocked_attr(name) {
            return Some(a.value.to_string());
        }
    }
    None
}

/// Parse a `style="..."` value into a [`ComputedStyle`]. Unknown
/// properties and malformed values are silently dropped — known
/// declarations on the same element still apply. Dropped declarations
/// (unsupported property, unsupported unit, recognised but
/// unmappable value such as `position: absolute`) push one finding
/// each into `lints`.
pub(crate) fn parse_inline_style(input: &str, lints: &Lints) -> ComputedStyle {
    let mut style = ComputedStyle::default();
    for decl in split_declarations(input) {
        let Some((prop, value)) = split_declaration(decl) else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        apply_declaration(&mut style, &prop, value, lints);
    }
    style
}

/// Split a declaration list on top-level `;`, honouring nested
/// parens (`rgb(0, 0, 0)`) and quoted strings.
fn split_declarations(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut depth: i32 = 0;
    let mut quote: Option<u8> = None;
    let mut start = 0;
    for i in 0..bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                if start < i {
                    out.push(&input[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < bytes.len() {
        out.push(&input[start..]);
    }
    out
}

fn split_declaration(decl: &str) -> Option<(&str, &str)> {
    let colon = decl.find(':')?;
    let (prop, rest) = decl.split_at(colon);
    Some((prop, &rest[1..]))
}

fn apply_declaration(style: &mut ComputedStyle, prop: &str, value: &str, lints: &Lints) {
    match prop {
        "color" => match parse_color(value) {
            Some(c) => style.text_color = Some(c),
            None => lints.push(
                FindingKind::DroppedDeclaration,
                format!("color: {} (unsupported color syntax)", value.trim()),
            ),
        },
        "background" | "background-color" => match parse_color(value) {
            Some(c) => style.background = Some(c),
            None => lints.push(
                FindingKind::DroppedDeclaration,
                format!("{prop}: {} (unsupported color syntax)", value.trim()),
            ),
        },
        "padding" => {
            if let Some(p) = parse_sides_shorthand(value) {
                style.padding = Some(p);
            }
        }
        "padding-top" => with_side(style, value, |s, v| s.top = v),
        "padding-right" => with_side(style, value, |s, v| s.right = v),
        "padding-bottom" => with_side(style, value, |s, v| s.bottom = v),
        "padding-left" => with_side(style, value, |s, v| s.left = v),
        "margin" => {
            if let Some(m) = parse_sides_shorthand(value) {
                style.margin = Some(m);
            }
        }
        "margin-top" => with_margin_side(style, value, |s, v| s.top = v),
        "margin-right" => with_margin_side(style, value, |s, v| s.right = v),
        "margin-bottom" => with_margin_side(style, value, |s, v| s.bottom = v),
        "margin-left" => with_margin_side(style, value, |s, v| s.left = v),
        "border" => {
            let (width, color) = parse_border_shorthand(value);
            if let Some(w) = width {
                style.border_width = Some(w);
            }
            if let Some(c) = color {
                style.border_color = Some(c);
            }
        }
        "border-width" => {
            apply_length_with_lint(value, prop, lints, |px| style.border_width = Some(px));
        }
        "border-color" => {
            if let Some(c) = parse_color(value) {
                style.border_color = Some(c);
            }
        }
        "border-radius" => {
            apply_length_with_lint(value, prop, lints, |px| style.border_radius = Some(px));
        }
        "opacity" => {
            if let Ok(v) = value.parse::<f32>() {
                style.opacity = Some(v);
            }
        }
        "width" => {
            apply_css_size_with_lint(value, prop, lints, |s| style.width = Some(s));
        }
        "height" => {
            apply_css_size_with_lint(value, prop, lints, |s| style.height = Some(s));
        }
        "min-width" => {
            apply_length_with_lint(value, prop, lints, |px| style.min_width = Some(px));
        }
        "max-width" => {
            apply_length_with_lint(value, prop, lints, |px| style.max_width = Some(px));
        }
        "min-height" => {
            apply_length_with_lint(value, prop, lints, |px| style.min_height = Some(px));
        }
        "max-height" => {
            apply_length_with_lint(value, prop, lints, |px| style.max_height = Some(px));
        }
        "text-align" => {
            if let Some(a) = parse_text_align(value) {
                style.text_align = Some(a);
            }
        }
        "font-size" => {
            apply_length_with_lint(value, prop, lints, |px| style.font_size = Some(px));
        }
        "font-weight" => {
            if let Some(w) = parse_font_weight(value) {
                style.font_weight = Some(w);
            }
        }
        "font-style" => match value.to_ascii_lowercase().as_str() {
            "italic" | "oblique" => style.italic = Some(true),
            "normal" => style.italic = Some(false),
            _ => {}
        },
        "font-family" => {
            // Mono families turn the flag on; non-mono families lint
            // as dropped because there's no per-element family pin
            // beyond mono. Unknown garbage is silently skipped.
            match classify_font_family(value) {
                FontFamilyClass::Monospace => style.font_mono = Some(true),
                FontFamilyClass::Other(name) => lints.push(
                    FindingKind::DroppedDeclaration,
                    format!("font-family: {name} (only monospace families map onto Damascene)"),
                ),
                FontFamilyClass::Unrecognised => {}
            }
        }
        "text-decoration" | "text-decoration-line" => {
            for token in value.split_ascii_whitespace() {
                match token.to_ascii_lowercase().as_str() {
                    "underline" => style.underline = Some(true),
                    "line-through" => style.strikethrough = Some(true),
                    "none" => {
                        style.underline = Some(false);
                        style.strikethrough = Some(false);
                    }
                    _ => {}
                }
            }
        }
        "box-shadow" => {
            if let Some(blur) = parse_box_shadow_blur(value) {
                style.box_shadow = Some(blur);
            }
        }
        "display" => {
            match value.to_ascii_lowercase().as_str() {
                "block" | "inline-block" => style.layout.display_is_flex = Some(false),
                "flex" | "inline-flex" => style.layout.display_is_flex = Some(true),
                "none" => {
                    // CSS `display: none` would hide the subtree;
                    // honouring it silently would be surprising in a
                    // renderer that otherwise always shows content.
                    // Lint and ignore so the author sees the input.
                    lints.push(
                        FindingKind::DroppedDeclaration,
                        "display: none (would hide content; element rendered anyway)",
                    );
                }
                other => lints.push(FindingKind::DroppedDeclaration, format!("display: {other}")),
            }
        }
        "flex-direction" => match value.to_ascii_lowercase().as_str() {
            "row" => style.layout.flex_axis = Some(Axis::Row),
            "column" => style.layout.flex_axis = Some(Axis::Column),
            "row-reverse" | "column-reverse" => lints.push(
                FindingKind::DroppedDeclaration,
                format!("flex-direction: {value} (reversed axes not supported)"),
            ),
            _ => {}
        },
        "align-items" => match value.to_ascii_lowercase().as_str() {
            "stretch" => style.layout.align = Some(Align::Stretch),
            "flex-start" | "start" => style.layout.align = Some(Align::Start),
            "center" => style.layout.align = Some(Align::Center),
            "flex-end" | "end" => style.layout.align = Some(Align::End),
            "baseline" => lints.push(
                FindingKind::DroppedDeclaration,
                "align-items: baseline (Damascene has no baseline alignment)",
            ),
            _ => {}
        },
        "justify-content" => match value.to_ascii_lowercase().as_str() {
            "flex-start" | "start" | "left" => style.layout.justify = Some(Justify::Start),
            "center" => style.layout.justify = Some(Justify::Center),
            "flex-end" | "end" | "right" => style.layout.justify = Some(Justify::End),
            "space-between" => style.layout.justify = Some(Justify::SpaceBetween),
            "space-around" | "space-evenly" => lints.push(
                FindingKind::DroppedDeclaration,
                format!("justify-content: {value} (only space-between is supported)"),
            ),
            _ => {}
        },
        "overflow" | "overflow-x" | "overflow-y" => match value.to_ascii_lowercase().as_str() {
            "hidden" | "clip" => style.layout.overflow = Some(Overflow::Hidden),
            "auto" | "scroll" => style.layout.overflow = Some(Overflow::Scroll),
            "visible" => {}
            _ => {}
        },
        "position" => match value.to_ascii_lowercase().as_str() {
            "static" | "relative" => {
                // `static` is the default and `relative` without
                // children consuming the positioning context is a
                // no-op — neither needs a finding.
            }
            other => lints.push(
                FindingKind::DroppedDeclaration,
                format!("position: {other} (use stack/overlay for overlays)"),
            ),
        },
        "float" => match value.to_ascii_lowercase().as_str() {
            "none" => {}
            other => lints.push(
                FindingKind::DroppedDeclaration,
                format!("float: {other} (no float layout in Damascene)"),
            ),
        },
        // Unknown property — silently ignored. We don't lint these
        // because authored CSS routinely carries vendor prefixes /
        // experimental properties / custom properties (`--foo`) that
        // would flood the lint surface without telling the author
        // anything actionable.
        _ => {}
    }
}

/// Apply a length declaration: write the parsed value via `setter`,
/// emit a `DroppedDeclaration` finding for the unsupported-unit case
/// (`vh`, `vw`, `fr`, `ch`, …), stay silent for malformed values.
fn apply_length_with_lint(value: &str, prop: &str, lints: &Lints, mut setter: impl FnMut(f32)) {
    match classify_length(value) {
        LengthParse::Ok(px) => setter(px),
        LengthParse::UnsupportedUnit(unit) => lints.push(
            FindingKind::DroppedDeclaration,
            format!("{prop}: {value} (unit `{unit}` not supported)"),
        ),
        LengthParse::Malformed => {}
    }
}

fn apply_css_size_with_lint(
    value: &str,
    prop: &str,
    lints: &Lints,
    mut setter: impl FnMut(CssSize),
) {
    let s = value.trim();
    if s.eq_ignore_ascii_case("auto") {
        setter(CssSize::Auto);
        return;
    }
    if let Some(rest) = s.strip_suffix('%') {
        if let Ok(n) = rest.trim().parse::<f32>() {
            setter(CssSize::Percent((n / 100.0).max(0.0)));
        }
        return;
    }
    apply_length_with_lint(value, prop, lints, |px| setter(CssSize::Px(px)));
}

fn with_margin_side(style: &mut ComputedStyle, value: &str, mutate: impl FnOnce(&mut Sides, f32)) {
    let Some(px) = parse_length_px(value) else {
        return;
    };
    let mut sides = style.margin.unwrap_or(Sides::zero());
    mutate(&mut sides, px);
    style.margin = Some(sides);
}

fn with_side(style: &mut ComputedStyle, value: &str, mutate: impl FnOnce(&mut Sides, f32)) {
    let Some(px) = parse_length_px(value) else {
        return;
    };
    let mut sides = style.padding.unwrap_or(Sides::zero());
    mutate(&mut sides, px);
    style.padding = Some(sides);
}

// ---------- Value parsers ----------

/// Parse a CSS `<color>` value: hex (`#rgb`, `#rrggbb`, `#rrggbbaa`),
/// functional (`rgb(r,g,b)`, `rgba(r,g,b,a)`), the small named subset
/// in [`named_color`], or `transparent`.
pub(crate) fn parse_color(input: &str) -> Option<Color> {
    let s = input.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return Some(Color::srgb_u8a(0, 0, 0, 0));
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(rest) = s.strip_prefix(|c: char| matches!(c, 'r' | 'R'))
        && let Some(args) = rest
            .strip_prefix(|c: char| matches!(c, 'g' | 'G'))
            .and_then(|r| r.strip_prefix(|c: char| matches!(c, 'b' | 'B')))
    {
        return parse_rgb_function(args);
    }
    if let Some(inner) = strip_function(s, "hsl").or_else(|| strip_function(s, "hsla")) {
        return parse_hsl_function(inner);
    }
    if let Some(inner) = strip_function(s, "hwb") {
        return parse_hwb_function(inner);
    }
    named_color(s)
}

/// Strip a `name(args)` wrapper (ASCII case-insensitive on the name),
/// returning the raw argument text.
fn strip_function<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    if s.len() < name.len() || !s[..name.len()].eq_ignore_ascii_case(name) {
        return None;
    }
    s[name.len()..]
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// Parse `hsl()` / `hsla()` arguments in either the legacy
/// comma-separated or the modern space-separated (`h s% l% / a`) form.
fn parse_hsl_function(inner: &str) -> Option<Color> {
    let parts: Vec<&str> = inner
        .split([',', '/', ' '])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let h = parse_hue(parts[0])?;
    let s = parse_fraction(parts[1])?;
    let l = parse_fraction(parts[2])?;
    let a = if parts.len() > 3 {
        parse_alpha_channel(parts[3])?
    } else {
        255
    };
    let (r, g, b) = hsl_to_rgb(h, s, l);
    Some(Color::srgb_u8a(r, g, b, a))
}

/// Parse `hwb()` arguments: hue, whiteness%, blackness% (+ optional
/// `/ alpha`). When whiteness + blackness ≥ 1 the result is the gray
/// `w / (w + b)`, per CSS Color 4.
fn parse_hwb_function(inner: &str) -> Option<Color> {
    let parts: Vec<&str> = inner
        .split([',', '/', ' '])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let h = parse_hue(parts[0])?;
    let w = parse_fraction(parts[1])?;
    let bk = parse_fraction(parts[2])?;
    let a = if parts.len() > 3 {
        parse_alpha_channel(parts[3])?
    } else {
        255
    };
    if w + bk >= 1.0 {
        let gray = ((w / (w + bk)) * 255.0).round() as u8;
        return Some(Color::srgb_u8a(gray, gray, gray, a));
    }
    let (r, g, b) = hsl_to_rgb(h, 1.0, 0.5);
    let scale = |c: u8| (((c as f32 / 255.0) * (1.0 - w - bk) + w) * 255.0).round() as u8;
    Some(Color::srgb_u8a(scale(r), scale(g), scale(b), a))
}

/// Parse a CSS hue: a bare number or a `deg`-suffixed angle, normalized
/// into `[0, 360)`. Other angle units (`rad`, `grad`, `turn`) are rare
/// enough in authored scraps to stay unsupported (the caller lints).
fn parse_hue(s: &str) -> Option<f32> {
    let num = s.strip_suffix("deg").unwrap_or(s).trim();
    let h: f32 = num.parse().ok()?;
    Some(h.rem_euclid(360.0))
}

/// Parse a percentage (or a bare 0–100 number) into `[0, 1]`.
fn parse_fraction(s: &str) -> Option<f32> {
    let num = s.strip_suffix('%').unwrap_or(s).trim();
    let v: f32 = num.parse().ok()?;
    Some((v / 100.0).clamp(0.0, 1.0))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to_u8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r1 + m), to_u8(g1 + m), to_u8(b1 + m))
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    fn nibble(c: char) -> Option<u8> {
        c.to_digit(16).map(|d| d as u8)
    }
    let chars: Vec<char> = hex.chars().collect();
    match chars.len() {
        3 => {
            let r = nibble(chars[0])?;
            let g = nibble(chars[1])?;
            let b = nibble(chars[2])?;
            Some(Color::srgb_u8(r * 17, g * 17, b * 17))
        }
        4 => {
            let r = nibble(chars[0])?;
            let g = nibble(chars[1])?;
            let b = nibble(chars[2])?;
            let a = nibble(chars[3])?;
            Some(Color::srgb_u8a(r * 17, g * 17, b * 17, a * 17))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::srgb_u8(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::srgb_u8a(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_function(after_rgb: &str) -> Option<Color> {
    // After-rgb form: `a(...)` for rgb(...) or `(...)` for raw — but
    // we already stripped `rgb`/`rgba` letters in `parse_color`, so
    // expect an optional `a` then a paren-wrapped arg list.
    let (has_alpha, rest) =
        if let Some(rest) = after_rgb.strip_prefix(|c: char| matches!(c, 'a' | 'A')) {
            (true, rest)
        } else {
            (false, after_rgb)
        };
    let rest = rest.trim();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split([',', '/']).map(str::trim).collect();
    let need = if has_alpha { 4 } else { 3 };
    if parts.len() < need {
        return None;
    }
    let r = parse_rgb_channel(parts[0])?;
    let g = parse_rgb_channel(parts[1])?;
    let b = parse_rgb_channel(parts[2])?;
    if has_alpha {
        let a = parse_alpha_channel(parts[3])?;
        Some(Color::srgb_u8a(r, g, b, a))
    } else {
        Some(Color::srgb_u8(r, g, b))
    }
}

fn parse_rgb_channel(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        let pct: f32 = num.trim().parse().ok()?;
        return Some(((pct / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let n: f32 = s.parse().ok()?;
    Some(n.clamp(0.0, 255.0) as u8)
}

fn parse_alpha_channel(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        let pct: f32 = num.trim().parse().ok()?;
        return Some(((pct / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let n: f32 = s.parse().ok()?;
    Some((n.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The full CSS named-color table (CSS Color 4 §6.1), plus the
/// gray/grey spelling pairs.
fn named_color(name: &str) -> Option<Color> {
    let n = name.to_ascii_lowercase();
    let (r, g, b) = match n.as_str() {
        "aliceblue" => (240, 248, 255),
        "antiquewhite" => (250, 235, 215),
        "aqua" | "cyan" => (0, 255, 255),
        "aquamarine" => (127, 255, 212),
        "azure" => (240, 255, 255),
        "beige" => (245, 245, 220),
        "bisque" => (255, 228, 196),
        "black" => (0, 0, 0),
        "blanchedalmond" => (255, 235, 205),
        "blue" => (0, 0, 255),
        "blueviolet" => (138, 43, 226),
        "brown" => (165, 42, 42),
        "burlywood" => (222, 184, 135),
        "cadetblue" => (95, 158, 160),
        "chartreuse" => (127, 255, 0),
        "chocolate" => (210, 105, 30),
        "coral" => (255, 127, 80),
        "cornflowerblue" => (100, 149, 237),
        "cornsilk" => (255, 248, 220),
        "crimson" => (220, 20, 60),
        "darkblue" => (0, 0, 139),
        "darkcyan" => (0, 139, 139),
        "darkgoldenrod" => (184, 134, 11),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "darkgreen" => (0, 100, 0),
        "darkkhaki" => (189, 183, 107),
        "darkmagenta" => (139, 0, 139),
        "darkolivegreen" => (85, 107, 47),
        "darkorange" => (255, 140, 0),
        "darkorchid" => (153, 50, 204),
        "darkred" => (139, 0, 0),
        "darksalmon" => (233, 150, 122),
        "darkseagreen" => (143, 188, 143),
        "darkslateblue" => (72, 61, 139),
        "darkslategray" | "darkslategrey" => (47, 79, 79),
        "darkturquoise" => (0, 206, 209),
        "darkviolet" => (148, 0, 211),
        "deeppink" => (255, 20, 147),
        "deepskyblue" => (0, 191, 255),
        "dimgray" | "dimgrey" => (105, 105, 105),
        "dodgerblue" => (30, 144, 255),
        "firebrick" => (178, 34, 34),
        "floralwhite" => (255, 250, 240),
        "forestgreen" => (34, 139, 34),
        "fuchsia" | "magenta" => (255, 0, 255),
        "gainsboro" => (220, 220, 220),
        "ghostwhite" => (248, 248, 255),
        "gold" => (255, 215, 0),
        "goldenrod" => (218, 165, 32),
        "gray" | "grey" => (128, 128, 128),
        "green" => (0, 128, 0),
        "greenyellow" => (173, 255, 47),
        "honeydew" => (240, 255, 240),
        "hotpink" => (255, 105, 180),
        "indianred" => (205, 92, 92),
        "indigo" => (75, 0, 130),
        "ivory" => (255, 255, 240),
        "khaki" => (240, 230, 140),
        "lavender" => (230, 230, 250),
        "lavenderblush" => (255, 240, 245),
        "lawngreen" => (124, 252, 0),
        "lemonchiffon" => (255, 250, 205),
        "lightblue" => (173, 216, 230),
        "lightcoral" => (240, 128, 128),
        "lightcyan" => (224, 255, 255),
        "lightgoldenrodyellow" => (250, 250, 210),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "lightgreen" => (144, 238, 144),
        "lightpink" => (255, 182, 193),
        "lightsalmon" => (255, 160, 122),
        "lightseagreen" => (32, 178, 170),
        "lightskyblue" => (135, 206, 250),
        "lightslategray" | "lightslategrey" => (119, 136, 153),
        "lightsteelblue" => (176, 196, 222),
        "lightyellow" => (255, 255, 224),
        "lime" => (0, 255, 0),
        "limegreen" => (50, 205, 50),
        "linen" => (250, 240, 230),
        "maroon" => (128, 0, 0),
        "mediumaquamarine" => (102, 205, 170),
        "mediumblue" => (0, 0, 205),
        "mediumorchid" => (186, 85, 211),
        "mediumpurple" => (147, 112, 219),
        "mediumseagreen" => (60, 179, 113),
        "mediumslateblue" => (123, 104, 238),
        "mediumspringgreen" => (0, 250, 154),
        "mediumturquoise" => (72, 209, 204),
        "mediumvioletred" => (199, 21, 133),
        "midnightblue" => (25, 25, 112),
        "mintcream" => (245, 255, 250),
        "mistyrose" => (255, 228, 225),
        "moccasin" => (255, 228, 181),
        "navajowhite" => (255, 222, 173),
        "navy" => (0, 0, 128),
        "oldlace" => (253, 245, 230),
        "olive" => (128, 128, 0),
        "olivedrab" => (107, 142, 35),
        "orange" => (255, 165, 0),
        "orangered" => (255, 69, 0),
        "orchid" => (218, 112, 214),
        "palegoldenrod" => (238, 232, 170),
        "palegreen" => (152, 251, 152),
        "paleturquoise" => (175, 238, 238),
        "palevioletred" => (219, 112, 147),
        "papayawhip" => (255, 239, 213),
        "peachpuff" => (255, 218, 185),
        "peru" => (205, 133, 63),
        "pink" => (255, 192, 203),
        "plum" => (221, 160, 221),
        "powderblue" => (176, 224, 230),
        "purple" => (128, 0, 128),
        "rebeccapurple" => (102, 51, 153),
        "red" => (255, 0, 0),
        "rosybrown" => (188, 143, 143),
        "royalblue" => (65, 105, 225),
        "saddlebrown" => (139, 69, 19),
        "salmon" => (250, 128, 114),
        "sandybrown" => (244, 164, 96),
        "seagreen" => (46, 139, 87),
        "seashell" => (255, 245, 238),
        "sienna" => (160, 82, 45),
        "silver" => (192, 192, 192),
        "skyblue" => (135, 206, 235),
        "slateblue" => (106, 90, 205),
        "slategray" | "slategrey" => (112, 128, 144),
        "snow" => (255, 250, 250),
        "springgreen" => (0, 255, 127),
        "steelblue" => (70, 130, 180),
        "tan" => (210, 180, 140),
        "teal" => (0, 128, 128),
        "thistle" => (216, 191, 216),
        "tomato" => (255, 99, 71),
        "turquoise" => (64, 224, 208),
        "violet" => (238, 130, 238),
        "wheat" => (245, 222, 179),
        "white" => (255, 255, 255),
        "whitesmoke" => (245, 245, 245),
        "yellow" => (255, 255, 0),
        "yellowgreen" => (154, 205, 50),
        _ => return None,
    };
    Some(Color::srgb_u8(r, g, b))
}

/// Outcome of [`classify_length`]: either a converted pixel value, an
/// unsupported unit (lintable — author wrote `10vw` and we can't honour
/// it), or malformed (silently dropped — `foo`, `12 px`, empty, …).
pub(crate) enum LengthParse {
    Ok(f32),
    UnsupportedUnit(String),
    Malformed,
}

/// Parse a CSS length and classify the failure modes. Used by the
/// declaration applier so unsupported-unit drops can produce a lint
/// finding while malformed values stay silent.
pub(crate) fn classify_length(input: &str) -> LengthParse {
    let s = input.trim();
    if s.is_empty() {
        return LengthParse::Malformed;
    }
    if s == "0" {
        return LengthParse::Ok(0.0);
    }
    let Some((num, unit)) = split_number_unit(s) else {
        return LengthParse::Malformed;
    };
    let Ok(n) = num.parse::<f32>() else {
        return LengthParse::Malformed;
    };
    let unit_lc = unit.to_ascii_lowercase();
    let px = match unit_lc.as_str() {
        "" => return LengthParse::Malformed, // bare non-zero numbers
        "px" => n,
        "pt" => n * 96.0 / 72.0,
        "rem" => n * 16.0,
        "em" => n * 16.0, // no parent-font-size context yet
        "vh" | "vw" | "vmin" | "vmax" | "fr" | "ch" | "ex" | "lh" | "rlh" | "cm" | "mm" | "in"
        | "pc" => {
            return LengthParse::UnsupportedUnit(unit_lc);
        }
        _ => return LengthParse::Malformed,
    };
    LengthParse::Ok(px)
}

/// Parse a CSS length into logical pixels. Supports `px`, `pt`, `rem`
/// (= 16px), `em` (= 16px in this slice; no parent-context lookup
/// yet), and bare `0`. Returns `None` for both malformed values and
/// unsupported units — call [`classify_length`] directly if you need
/// to distinguish them (e.g. to emit a lint finding).
pub(crate) fn parse_length_px(input: &str) -> Option<f32> {
    match classify_length(input) {
        LengthParse::Ok(px) => Some(px),
        _ => None,
    }
}

/// CSS `font-family` classification: monospace fingerprints flip the
/// El's mono toggle, anything else lints as dropped.
enum FontFamilyClass {
    Monospace,
    Other(String),
    /// Empty / pure garbage — no finding, no effect.
    Unrecognised,
}

fn classify_font_family(value: &str) -> FontFamilyClass {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return FontFamilyClass::Unrecognised;
    }
    // CSS allows a comma-separated fallback list. Walk it and treat
    // the *first* family name as the author's intent; if any family
    // in the list is monospace, treat the declaration as mono.
    let mut first: Option<String> = None;
    for raw in trimmed.split(',') {
        let name = raw
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if is_monospace_family(&name) {
            return FontFamilyClass::Monospace;
        }
        if first.is_none() {
            first = Some(name);
        }
    }
    match first {
        Some(name) => FontFamilyClass::Other(name),
        None => FontFamilyClass::Unrecognised,
    }
}

fn is_monospace_family(name: &str) -> bool {
    // CSS generic `monospace`, common mono faces, plus the `mono`
    // / `code` suffixes that show up in design-system names.
    if name == "monospace" || name == "mono" {
        return true;
    }
    let monos = [
        "courier",
        "courier new",
        "consolas",
        "menlo",
        "monaco",
        "ubuntu mono",
        "fira code",
        "fira mono",
        "source code pro",
        "jetbrains mono",
        "cascadia code",
        "cascadia mono",
        "sf mono",
        "roboto mono",
        "ibm plex mono",
        "anonymous pro",
        "dejavu sans mono",
        "liberation mono",
        "inconsolata",
        "victor mono",
        "iosevka",
    ];
    monos.contains(&name)
}

/// Best-effort `box-shadow` parser: extract the *blur* length (the
/// second length value in the shorthand) and return it as the El's
/// shadow scalar. Offset, spread, and color are dropped because
/// Damascene's `shadow(f32)` modifier doesn't carry them. CSS multi-shadow
/// lists (`box-shadow: 0 1px 2px black, 0 4px 8px gray`) collapse to
/// the largest blur encountered.
fn parse_box_shadow_blur(input: &str) -> Option<f32> {
    let s = input.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    // Comma-separated shadow list — but the value can contain commas
    // inside `rgb(...)` colors. Reuse the paren-aware splitter.
    let mut best: Option<f32> = None;
    for shadow in split_top_level_commas(s) {
        let mut lengths: Vec<f32> = Vec::new();
        for token in shadow.split_ascii_whitespace() {
            if token.eq_ignore_ascii_case("inset") {
                continue;
            }
            if let Some(px) = parse_length_px(token) {
                lengths.push(px);
                if lengths.len() == 4 {
                    break;
                }
            }
        }
        // Conventional ordering: offset-x, offset-y, blur, spread.
        // Take the third (blur) when present; fall back to the second
        // length for `box-shadow: 0 4px black` which omits blur.
        let blur = match lengths.as_slice() {
            [_, _, blur, ..] => Some(*blur),
            [_, second] => Some(*second),
            _ => None,
        };
        if let Some(b) = blur {
            best = Some(best.map_or(b, |prev| prev.max(b)));
        }
    }
    best
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut depth: i32 = 0;
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                if start < i {
                    out.push(input[start..i].trim());
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < bytes.len() {
        let tail = input[start..].trim();
        if !tail.is_empty() {
            out.push(tail);
        }
    }
    out
}

fn split_number_unit(s: &str) -> Option<(&str, &str)> {
    let end = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some((&s[..end], &s[end..]))
}

/// Parse a CSS `padding` / `margin` shorthand: 1, 2, 3, or 4 lengths.
/// Matches CSS order — `top right bottom left`, with the 2/3-value
/// forms mirroring the missing sides.
pub(crate) fn parse_sides_shorthand(input: &str) -> Option<Sides> {
    let parts: Vec<&str> = input.split_ascii_whitespace().collect();
    let px: Vec<f32> = parts
        .iter()
        .map(|p| parse_length_px(p))
        .collect::<Option<Vec<_>>>()?;
    let sides = match px.len() {
        1 => Sides::all(px[0]),
        2 => Sides {
            top: px[0],
            right: px[1],
            bottom: px[0],
            left: px[1],
        },
        3 => Sides {
            top: px[0],
            right: px[1],
            bottom: px[2],
            left: px[1],
        },
        4 => Sides {
            top: px[0],
            right: px[1],
            bottom: px[2],
            left: px[3],
        },
        _ => return None,
    };
    Some(sides)
}

/// Parse a `border` shorthand into `(width, color)`. The spec form is
/// `<width> <style> <color>` in any order; we pick out the first
/// length-shaped token as the width and the first colour-shaped token
/// as the colour, ignoring the `<style>` (Damascene only paints solid
/// borders).
pub(crate) fn parse_border_shorthand(input: &str) -> (Option<f32>, Option<Color>) {
    let mut width = None;
    let mut color = None;
    for token in input.split_ascii_whitespace() {
        if width.is_none()
            && let Some(px) = parse_length_px(token)
        {
            width = Some(px);
            continue;
        }
        if color.is_none()
            && let Some(c) = parse_color(token)
        {
            color = Some(c);
            continue;
        }
    }
    (width, color)
}

fn parse_text_align(value: &str) -> Option<TextAlign> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "start" => Some(TextAlign::Start),
        "center" => Some(TextAlign::Center),
        "right" | "end" => Some(TextAlign::End),
        _ => None,
    }
}

fn parse_font_weight(value: &str) -> Option<FontWeight> {
    match value.to_ascii_lowercase().as_str() {
        "normal" | "400" => Some(FontWeight::Regular),
        "500" => Some(FontWeight::Medium),
        "semibold" | "demibold" | "600" => Some(FontWeight::Semibold),
        "bold" | "700" | "bolder" | "800" | "900" => Some(FontWeight::Bold),
        "100" | "200" | "300" | "lighter" => Some(FontWeight::Regular),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_3_4_6_8_all_parse() {
        assert_eq!(parse_color("#000"), Some(Color::srgb_u8(0, 0, 0)));
        assert_eq!(parse_color("#fff"), Some(Color::srgb_u8(255, 255, 255)));
        assert_eq!(parse_color("#abc"), Some(Color::srgb_u8(170, 187, 204)));
        assert_eq!(parse_color("#1234"), Some(Color::srgb_u8a(17, 34, 51, 68)));
        assert_eq!(parse_color("#ff8800"), Some(Color::srgb_u8(255, 136, 0)));
        assert_eq!(
            parse_color("#ff8800ff"),
            Some(Color::srgb_u8a(255, 136, 0, 255))
        );
    }

    #[test]
    fn rgb_and_rgba_functional_forms() {
        assert_eq!(
            parse_color("rgb(10, 20, 30)"),
            Some(Color::srgb_u8(10, 20, 30))
        );
        assert_eq!(
            parse_color("rgba(10, 20, 30, 0.5)"),
            Some(Color::srgb_u8a(10, 20, 30, 128))
        );
        assert_eq!(
            parse_color("rgb(100%, 0%, 50%)"),
            Some(Color::srgb_u8(255, 0, 128))
        );
    }

    #[test]
    fn named_colors_parse() {
        assert_eq!(parse_color("red"), Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(parse_color("RED"), Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(
            parse_color("transparent"),
            Some(Color::srgb_u8a(0, 0, 0, 0))
        );
        assert_eq!(parse_color("not-a-color"), None);
        // Full CSS named-color table, not just the common subset.
        assert_eq!(
            parse_color("rebeccapurple"),
            Some(Color::srgb_u8(102, 51, 153))
        );
        assert_eq!(
            parse_color("dodgerblue"),
            Some(Color::srgb_u8(30, 144, 255))
        );
        assert_eq!(
            parse_color("DarkSlateGrey"),
            Some(Color::srgb_u8(47, 79, 79))
        );
    }

    #[test]
    fn hsl_colors_parse() {
        // Legacy comma form.
        assert_eq!(
            parse_color("hsl(120, 100%, 25%)"),
            Some(Color::srgb_u8(0, 128, 0))
        );
        // Modern space form with slash alpha.
        assert_eq!(
            parse_color("hsl(0 100% 50% / 0.5)"),
            Some(Color::srgb_u8a(255, 0, 0, 128))
        );
        // hsla + deg suffix + negative hue normalization.
        assert_eq!(
            parse_color("hsla(-240deg, 100%, 50%, 1)"),
            Some(Color::srgb_u8a(0, 255, 0, 255))
        );
        // Achromatic.
        assert_eq!(
            parse_color("hsl(0, 0%, 50%)"),
            Some(Color::srgb_u8(128, 128, 128))
        );
    }

    #[test]
    fn hwb_colors_parse() {
        assert_eq!(parse_color("hwb(0 0% 0%)"), Some(Color::srgb_u8(255, 0, 0)));
        // w + b >= 100% → gray w/(w+b).
        assert_eq!(
            parse_color("hwb(120 60% 60%)"),
            Some(Color::srgb_u8(128, 128, 128))
        );
        // Tinted: red whitened and blackened.
        assert_eq!(
            parse_color("hwb(0 20% 20%)"),
            Some(Color::srgb_u8(204, 51, 51))
        );
    }

    #[test]
    fn unsupported_color_functions_return_none() {
        assert_eq!(parse_color("oklch(0.7 0.1 200)"), None);
        assert_eq!(parse_color("lab(50% 40 59.5)"), None);
        assert_eq!(parse_color("hsl(nope)"), None);
    }

    #[test]
    fn lengths_in_supported_units() {
        assert_eq!(parse_length_px("12px"), Some(12.0));
        assert_eq!(parse_length_px("1rem"), Some(16.0));
        assert_eq!(parse_length_px("2em"), Some(32.0));
        assert_eq!(parse_length_px("0"), Some(0.0));
        assert!((parse_length_px("12pt").unwrap() - 16.0).abs() < 0.01);
        assert_eq!(parse_length_px("12"), None);
        assert_eq!(parse_length_px("12vw"), None);
    }

    #[test]
    fn css_size_handles_px_percent_auto_via_inline_style() {
        let lints = Lints::default();
        let style = parse_inline_style("width: 200px; height: 50%; min-width: auto", &lints);
        assert_eq!(style.width, Some(CssSize::Px(200.0)));
        assert_eq!(style.height, Some(CssSize::Percent(0.5)));
        // min-width / max-width route through apply_length_with_lint
        // (length-only) so `auto` doesn't apply there. width / height
        // route through apply_css_size_with_lint which accepts auto.
    }

    #[test]
    fn padding_shorthand_one_two_three_four() {
        assert_eq!(parse_sides_shorthand("8px"), Some(Sides::all(8.0)));
        assert_eq!(
            parse_sides_shorthand("8px 16px"),
            Some(Sides {
                top: 8.0,
                right: 16.0,
                bottom: 8.0,
                left: 16.0,
            })
        );
        assert_eq!(
            parse_sides_shorthand("8px 16px 24px"),
            Some(Sides {
                top: 8.0,
                right: 16.0,
                bottom: 24.0,
                left: 16.0,
            })
        );
        assert_eq!(
            parse_sides_shorthand("1px 2px 3px 4px"),
            Some(Sides {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            })
        );
    }

    #[test]
    fn border_shorthand_extracts_width_and_color_in_any_order() {
        let (w, c) = parse_border_shorthand("1px solid #f00");
        assert_eq!(w, Some(1.0));
        assert_eq!(c, Some(Color::srgb_u8(255, 0, 0)));

        let (w, c) = parse_border_shorthand("red 2px");
        assert_eq!(w, Some(2.0));
        assert_eq!(c, Some(Color::srgb_u8(255, 0, 0)));
    }

    #[test]
    fn font_weight_named_and_numeric() {
        assert_eq!(parse_font_weight("bold"), Some(FontWeight::Bold));
        assert_eq!(parse_font_weight("700"), Some(FontWeight::Bold));
        assert_eq!(parse_font_weight("500"), Some(FontWeight::Medium));
        assert_eq!(parse_font_weight("normal"), Some(FontWeight::Regular));
        assert_eq!(parse_font_weight("400"), Some(FontWeight::Regular));
        assert_eq!(parse_font_weight("garbage"), None);
    }

    #[test]
    fn inline_style_round_trip() {
        let lints = Lints::default();
        let style = parse_inline_style(
            "color: #ff0000; background: rgb(0, 255, 0); padding: 8px 16px; \
             font-size: 14px; font-weight: bold; text-align: center; \
             border-radius: 4px; opacity: 0.5",
            &lints,
        );
        assert_eq!(style.text_color, Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(style.background, Some(Color::srgb_u8(0, 255, 0)));
        assert_eq!(
            style.padding,
            Some(Sides {
                top: 8.0,
                right: 16.0,
                bottom: 8.0,
                left: 16.0,
            })
        );
        assert_eq!(style.font_size, Some(14.0));
        assert_eq!(style.font_weight, Some(FontWeight::Bold));
        assert_eq!(style.text_align, Some(TextAlign::Center));
        assert_eq!(style.border_radius, Some(4.0));
        assert_eq!(style.opacity, Some(0.5));
    }

    #[test]
    fn inline_style_ignores_unknown_props_and_malformed_values() {
        let lints = Lints::default();
        let style = parse_inline_style(
            "color: red; foo: bar; padding: not-a-length; font-size: 18px",
            &lints,
        );
        assert_eq!(style.text_color, Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(style.padding, None);
        assert_eq!(style.font_size, Some(18.0));
    }

    #[test]
    fn margin_shorthand_parses_into_margin_field() {
        let lints = Lints::default();
        let style = parse_inline_style("margin: 12px 4px", &lints);
        assert_eq!(
            style.margin,
            Some(Sides {
                top: 12.0,
                right: 4.0,
                bottom: 12.0,
                left: 4.0,
            })
        );
    }

    #[test]
    fn margin_per_side_props_fold_together() {
        let lints = Lints::default();
        let style = parse_inline_style("margin-top: 8px; margin-bottom: 16px", &lints);
        let m = style.margin.unwrap();
        assert_eq!(m.top, 8.0);
        assert_eq!(m.bottom, 16.0);
        assert_eq!(m.left, 0.0);
        assert_eq!(m.right, 0.0);
    }

    #[test]
    fn display_flex_with_direction_writes_layout_overrides() {
        let lints = Lints::default();
        let style = parse_inline_style(
            "display: flex; flex-direction: row; align-items: center; justify-content: space-between",
            &lints,
        );
        assert_eq!(style.layout.display_is_flex, Some(true));
        assert_eq!(style.layout.flex_axis, Some(Axis::Row));
        assert_eq!(style.layout.align, Some(Align::Center));
        assert_eq!(style.layout.justify, Some(Justify::SpaceBetween));
    }

    #[test]
    fn overflow_maps_to_hidden_or_scroll() {
        let lints = Lints::default();
        assert_eq!(
            parse_inline_style("overflow: hidden", &lints)
                .layout
                .overflow,
            Some(Overflow::Hidden)
        );
        assert_eq!(
            parse_inline_style("overflow: auto", &lints).layout.overflow,
            Some(Overflow::Scroll)
        );
        assert_eq!(
            parse_inline_style("overflow: scroll", &lints)
                .layout
                .overflow,
            Some(Overflow::Scroll)
        );
        assert_eq!(
            parse_inline_style("overflow: visible", &lints)
                .layout
                .overflow,
            None
        );
    }

    #[test]
    fn font_family_mono_sets_flag_other_lints() {
        let lints = Lints::default();
        let style = parse_inline_style("font-family: 'JetBrains Mono', monospace", &lints);
        assert_eq!(style.font_mono, Some(true));
        let lints = Lints::default();
        let style = parse_inline_style("font-family: 'Helvetica Neue', sans-serif", &lints);
        assert_eq!(style.font_mono, None);
        let findings = lints.into_vec();
        assert!(findings.iter().any(|f| f.detail.contains("font-family")));
    }

    #[test]
    fn box_shadow_extracts_blur_largest_in_list() {
        let lints = Lints::default();
        assert_eq!(
            parse_inline_style("box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2)", &lints).box_shadow,
            Some(8.0)
        );
        assert_eq!(
            parse_inline_style("box-shadow: 0 1px 2px black, 0 4px 16px gray", &lints).box_shadow,
            Some(16.0)
        );
        assert_eq!(
            parse_inline_style("box-shadow: none", &lints).box_shadow,
            Some(0.0)
        );
    }

    #[test]
    fn unsupported_units_lint_as_dropped() {
        let lints = Lints::default();
        let style = parse_inline_style("font-size: 4vw; width: 50vh", &lints);
        assert_eq!(style.font_size, None);
        assert_eq!(style.width, None);
        let findings = lints.into_vec();
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|f| f.detail.contains("not supported")));
    }

    #[test]
    fn position_and_float_lint_as_dropped() {
        let lints = Lints::default();
        let _ = parse_inline_style("position: absolute; float: left", &lints);
        let findings = lints.into_vec();
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().any(|f| f.detail.contains("position")));
        assert!(findings.iter().any(|f| f.detail.contains("float")));
    }

    #[test]
    fn position_relative_and_static_do_not_lint() {
        let lints = Lints::default();
        let _ = parse_inline_style("position: relative", &lints);
        let _ = parse_inline_style("position: static", &lints);
        assert!(lints.into_vec().is_empty());
    }

    #[test]
    fn split_declarations_respects_parens_and_quotes() {
        let decls =
            split_declarations("color: rgb(0, 0, 0); background: \"hi; there\"; padding: 8px");
        assert_eq!(decls.len(), 3);
        assert!(decls[0].contains("rgb(0, 0, 0)"));
        assert!(decls[1].contains("hi; there"));
        assert!(decls[2].contains("padding"));
    }
}
