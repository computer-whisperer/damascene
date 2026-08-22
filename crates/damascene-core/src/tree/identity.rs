//! Identity, source, and interaction-flag modifiers for [`El`].

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use super::geometry::Sides;
use super::node::El;
use super::semantics::{Kind, Source};

/// Configuration for [`El::hover_alpha`] — the rest and peak alpha
/// endpoints for a node whose opacity binds to the **subtree
/// interaction envelope** (max of hover, focus, and press over the
/// subtree rooted at this node).
///
/// `rest` is the drawn alpha when no descendant of this node is
/// currently the active hover, focus, or press target. `peak` is the
/// drawn alpha at full envelope. Linear interpolation between the two
/// follows the eased subtree envelope (0..1).
///
/// Both fields are clamped to `[0.0, 1.0]` by [`El::hover_alpha`].
/// Typical use is `rest < peak` ("reveal on interaction"), but the
/// representation accepts `rest > peak` ("fade out on interaction") and
/// sub-1.0 peaks for subtle affordances.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverAlpha {
    /// Drawn alpha when the subtree interaction envelope is 0 (no
    /// hover, focus, or press on this node or any descendant).
    pub rest: f32,
    /// Drawn alpha at full interaction envelope (1.0).
    pub peak: f32,
}

impl El {
    /// Construct a bare element of the given [`Kind`] with all
    /// modifiers at their defaults. App code usually reaches for the
    /// catalog constructors (`column`, `button`, `card`, …) instead.
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            ..Default::default()
        }
    }

    // ---- Identity / source ----
    /// Give this node a stable identity across rebuilds. The key
    /// becomes part of the node's `computed_id` (`role[key]` instead of
    /// `role.index`), so focus, hover, scroll offsets, hit-testing, and
    /// animation survive sibling reordering. Required for `.tooltip()`,
    /// `.selectable()`, and anything else looked up by identity.
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }

    /// Make this node opaque to pointer hit-testing: pointer events
    /// over its rect stop here instead of falling through to whatever
    /// is painted beneath (scrims, modal surfaces).
    pub fn block_pointer(mut self) -> Self {
        self.block_pointer = true;
        self
    }

    /// Expand this node's pointer hit target without changing layout
    /// or paint. Hover, press, cursor, tooltip, and click routing all
    /// use the expanded target; [`UiEvent::target_rect`][crate::UiEvent::target_rect]
    /// still reports the node's transformed visual rect from layout.
    ///
    /// Keep this conservative. It is for controls whose effective
    /// interaction region is intentionally larger than their drawn
    /// chrome, not for making unrelated gutters activate nearby UI.
    pub fn hit_overflow(mut self, outset: impl Into<Sides>) -> Self {
        self.hit_overflow = outset.into();
        self
    }

    /// Include this node in keyboard focus traversal (Tab order
    /// follows tree order). Focused nodes receive activation keys and
    /// paint the stock focus ring; pair with `.key(...)` so focus
    /// survives rebuilds.
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// Whether this node anchors an *interaction region*: a focusable
    /// node whose subtree hover / press / focus envelopes descendants
    /// read for `hover_alpha` reveals (a tab revealing its close ×).
    /// Every focusable node is one except a `viewport()`: it is
    /// focusable only for keyboard navigation (#144), and its
    /// background is the hover target for the whole canvas, so
    /// treating it as a region would reveal every descendant's
    /// affordance on any pointer transit — the #110 flicker in a new
    /// coat. The cascade passes through it instead.
    pub(crate) fn is_interaction_region(&self) -> bool {
        self.focusable && !matches!(self.kind, Kind::Viewport)
    }

    /// Prefer this node when the floating layer containing it
    /// auto-focuses on open — HTML's `autofocus` attribute. See
    /// [`El::autofocus`] (the field) for the full rule; the node must
    /// also be focusable (most stock widgets already are) to be
    /// picked.
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }

    /// Suppress this keyed node's interaction-state visuals — the
    /// hover-lighten, press-darken, and focus ring that a keyed node's
    /// fill otherwise picks up from the pointer. Use it on a
    /// keyed-but-decorative surface: one keyed purely for identity,
    /// routing, or persistent state (a pan/zoom canvas background, a
    /// graph node keyed only for click routing, a keyed layout anchor)
    /// whose fill should stay static under the cursor.
    ///
    /// The node still hit-tests and routes clicks/events normally — only
    /// the visual state response is dropped (the underlying envelope is
    /// never tracked, so it reads back at rest). [`Kind::Scrim`] and
    /// [`crate::tree::viewport`] get this behavior automatically; this is
    /// the opt-in for everything else.
    pub fn no_hover(mut self) -> Self {
        self.no_hover = true;
        self
    }

    /// Show the focus ring on this node even when focus arrived via
    /// pointer click. Default focus-ring behavior follows the web
    /// platform's `:focus-visible` rule — ring on Tab, no ring on
    /// click. Widgets where the ring is meaningful regardless of
    /// source — text input, text area — opt in here so clicking into
    /// the field still raises the "now active" affordance. Implies
    /// nothing about focusability; pair with `.focusable()`.
    pub fn always_show_focus_ring(mut self) -> Self {
        self.always_show_focus_ring = true;
        self
    }

    /// Opt this node into the library's text-selection system. The
    /// node must also carry an explicit `.key(...)`; selection requires
    /// stable identity across rebuilds the same way focus does.
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// Opt this node into consuming touch drag. A touch contact that
    /// starts on this node (or any descendant — the flag inherits
    /// down the tree) is treated as a drag rather than a pan/scroll
    /// gesture, suppressing the runner's touch-scroll synthesis.
    /// Use on widgets whose primary interaction is dragging:
    /// sliders, scrubbers, resize handles, draggable cards. No
    /// effect on mouse / pen pointers.
    pub fn consumes_touch_drag(mut self) -> Self {
        self.consumes_touch_drag = true;
        self
    }

    /// Attach source-backed copy/hit-test text for this selectable
    /// node. The node still needs `.selectable().key(...)`; this only
    /// changes how selection offsets map to copied text.
    pub fn selection_source(mut self, source: crate::selection::SelectionSource) -> Self {
        self.selection_source = Some(Box::new(source));
        self
    }

    /// Opt this node into raw key capture when focused. While this
    /// node is the focused target, the library's traversal/activation
    /// defaults are bypassed and raw `KeyDown` events are delivered for
    /// the widget to interpret. Escape is still treated as "exit
    /// editing": the raw `KeyDown` is delivered first, then focus is
    /// cleared. Implies `focusable`.
    pub fn capture_keys(mut self) -> Self {
        self.capture_keys = true;
        self.focusable = true;
        self
    }

    // ---- Accessibility (ARIA-shaped; see crate::a11y) ----

    /// Lazily allocate and borrow this node's accessibility props.
    pub(crate) fn a11y_mut(&mut self) -> &mut crate::a11y::A11yProps {
        self.a11y.get_or_insert_with(Box::default)
    }

    /// Set the semantic role assistive technology announces this
    /// element as — the ARIA `role` attribute. Stock widgets set their
    /// own; custom widgets pick the [`crate::a11y::Role`] matching the
    /// ARIA pattern they implement.
    pub fn role(mut self, role: crate::a11y::Role) -> Self {
        self.a11y_mut().role = Some(role);
        self
    }

    /// Override the accessible name — ARIA `aria-label`. Without it,
    /// assistive technology derives the name from visible text
    /// content, which is right for `button("Save")` and wrong for an
    /// icon-only button; label those:
    /// `icon_button("x").aria_label("Close")`.
    pub fn aria_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_mut().label = Some(label.into());
        self
    }

    /// Supplementary description read after the name — the ARIA
    /// `aria-describedby` content. A `.tooltip(...)` already doubles
    /// as the description when this is unset.
    pub fn aria_description(mut self, description: impl Into<String>) -> Self {
        self.a11y_mut().description = Some(description.into());
        self
    }

    /// Alternative text for image content — HTML `alt`. Same field as
    /// [`Self::aria_label`], named for the idiom:
    /// `image(img).alt("Boarding pass QR code")`.
    pub fn alt(mut self, alt: impl Into<String>) -> Self {
        self.a11y_mut().label = Some(alt.into());
        self
    }

    /// Hide this node and its whole subtree from assistive technology
    /// — ARIA `aria-hidden="true"`. For decorative or duplicated
    /// content; the node still lays out and paints normally.
    pub fn aria_hidden(mut self) -> Self {
        self.a11y_mut().hidden = true;
        self
    }

    /// Announce content changes inside this node — ARIA `aria-live`.
    pub fn aria_live(mut self, live: crate::a11y::LiveRegion) -> Self {
        self.a11y_mut().live = Some(live);
        self
    }

    /// Checked state for checkbox / switch / radio roles — ARIA
    /// `aria-checked`.
    pub fn aria_checked(mut self, checked: bool) -> Self {
        self.a11y_mut().checked = Some(checked);
        self
    }

    /// Expanded/collapsed state for disclosure-style controls — ARIA
    /// `aria-expanded` (accordions, comboboxes, menus).
    pub fn aria_expanded(mut self, expanded: bool) -> Self {
        self.a11y_mut().expanded = Some(expanded);
        self
    }

    /// Selected state for tabs / options / rows — ARIA `aria-selected`.
    pub fn aria_selected(mut self, selected: bool) -> Self {
        self.a11y_mut().selected = Some(selected);
        self
    }

    /// Pressed state for toggle buttons — ARIA `aria-pressed`.
    pub fn aria_pressed(mut self, pressed: bool) -> Self {
        self.a11y_mut().pressed = Some(pressed);
        self
    }

    /// Declare this node's editable-text state for assistive
    /// technology — the input to the AccessKit text protocol
    /// (per-character reading, caret/selection reporting, AT-driven
    /// caret moves). For [`Role::Textbox`](crate::a11y::Role::Textbox)
    /// nodes; stock text widgets stamp it themselves. See
    /// [`EditableText`](crate::a11y::EditableText).
    pub fn editable_text(mut self, text: crate::a11y::EditableText) -> Self {
        self.a11y_mut().text_edit = Some(Box::new(text));
        self
    }

    /// Report this element disabled to assistive technology — ARIA
    /// `aria-disabled`. The stock [`Self::disabled`] style modifier
    /// already sets this alongside its visual/behavioral treatment;
    /// reach for this directly only in custom disabled treatments.
    pub fn aria_disabled(mut self, disabled: bool) -> Self {
        self.a11y_mut().disabled = disabled;
        self
    }

    /// Numeric value, minimum, and maximum for slider / spinbutton /
    /// progressbar roles — ARIA `aria-valuenow` / `-valuemin` /
    /// `-valuemax`.
    pub fn aria_value(mut self, now: f64, min: f64, max: f64) -> Self {
        self.a11y_mut().value = Some((now, min, max));
        self
    }

    /// Human-readable value override — ARIA `aria-valuetext` (e.g.
    /// `"52%"`, `"March"`) for when the bare number is meaningless.
    pub fn aria_value_text(mut self, text: impl Into<String>) -> Self {
        self.a11y_mut().value_text = Some(text.into());
        self
    }

    /// Heading level 1–6 for [`crate::a11y::Role::Heading`] — ARIA
    /// `aria-level`.
    pub fn aria_level(mut self, level: u8) -> Self {
        self.a11y_mut().level = Some(level);
        self
    }

    /// Declare this surface modal to assistive technology — ARIA
    /// `aria-modal="true"`. The focus-trap layer system already
    /// enforces the behavior; this makes screen readers treat content
    /// behind it as inert too.
    pub fn aria_modal(mut self) -> Self {
        self.a11y_mut().modal = true;
        self
    }

    /// Multiply this element's paint opacity by the nearest focusable
    /// ancestor's focus envelope.
    pub fn alpha_follows_focused_ancestor(mut self) -> Self {
        self.alpha_follows_focused_ancestor = true;
        self
    }

    /// Multiply this node's paint opacity by the runtime's caret blink
    /// alpha.
    pub fn blink_when_focused(mut self) -> Self {
        self.blink_when_focused = true;
        self
    }

    /// Borrow hover and press visual envelopes from the nearest
    /// focusable ancestor.
    pub fn state_follows_interactive_ancestor(mut self) -> Self {
        self.state_follows_interactive_ancestor = true;
        self
    }

    /// Bind this element's paint opacity to the subtree interaction
    /// envelope — the `max` of hover, focus, and press for the subtree
    /// rooted at this element.
    ///
    /// At rest (no descendant is the active hover, focus, or press
    /// target) the element paints at `rest`. At full envelope it paints
    /// at `peak`. Both are clamped to `[0.0, 1.0]`, with linear
    /// interpolation in between following the eased envelope.
    ///
    /// "Subtree" matches CSS `:hover` semantics: hovering, focusing, or
    /// pressing *any descendant* keeps the element revealed. A
    /// hover-revealed close icon stays visible while the cursor moves
    /// across the tab body or while the tab is keyboard-focused; an
    /// action pill stays visible while the cursor moves between
    /// focusable buttons inside it. The trigger isn't strictly
    /// "hover" — focus and press also count — but `hover` is the
    /// dominant case and the name reflects it.
    ///
    /// Layout-neutral — the element keeps its computed rect at all
    /// times. Use for hover-revealed close buttons, secondary actions
    /// on list rows, hover-only validation icons, and other
    /// "show on interaction" patterns where the surrounding layout
    /// shouldn't shift.
    ///
    /// # Beyond alpha
    ///
    /// For the other common hover affordances — Material-style lift
    /// (`translate_y`), button-pop (`scale`), tint shift (`fill`) —
    /// drive the prop from app code using
    /// [`crate::BuildCx::is_hovering_within`] plus
    /// [`Self::animate`]:
    ///
    /// ```ignore
    /// fn build(&self, cx: &BuildCx) -> El {
    ///     let lifted = cx.is_hovering_within("card");
    ///     card([...])
    ///         .key("card")
    ///         .focusable()
    ///         .translate(0.0, if lifted { -2.0 } else { 0.0 })
    ///         .scale(if lifted { 1.02 } else { 1.0 })
    ///         .animate(Timing::SPRING_QUICK)
    /// }
    /// ```
    ///
    /// `is_hovering_within` reads the same subtree predicate
    /// `hover_alpha` consumes (CSS `:hover`-style cascade). `animate`
    /// eases the prop between the two build values across frames, so
    /// the transition is smooth without per-channel declarative API.
    /// `hover_alpha` itself is the alpha-channel shorthand — it skips
    /// the boolean-to-value conversion and the per-node `animate`
    /// allocation, since alpha is the dominant hover affordance.
    pub fn hover_alpha(mut self, rest: f32, peak: f32) -> Self {
        self.hover_alpha = Some(HoverAlpha {
            rest: rest.clamp(0.0, 1.0),
            peak: peak.clamp(0.0, 1.0),
        });
        self
    }

    /// Set the source attribution (file + line) reported for this node
    /// by lint findings and inspection dumps, marking it as user code.
    /// Catalog constructors set this automatically via `#[track_caller]`;
    /// see [`Self::at_loc`].
    pub fn at(mut self, file: &'static str, line: u32) -> Self {
        self.source = Source {
            file,
            line,
            from_library: false,
        };
        self
    }

    /// Set source from a `Location` (used internally by
    /// `#[track_caller]` constructors).
    pub fn at_loc(mut self, loc: &'static Location<'static>) -> Self {
        self.source = Source::from_caller(loc);
        self
    }

    /// Mark this El as constructed inside an damascene library closure
    /// where `#[track_caller]` doesn't reach user code (e.g. the
    /// `.map(|item| ...)` body inside `tabs_list`, `radio_group`,
    /// etc.). The lint pass uses this flag to walk blame attribution
    /// upward to the nearest user-source ancestor instead of pointing
    /// findings at damascene-core internals. User code never needs to call
    /// this.
    pub fn from_library(mut self) -> Self {
        self.source.from_library = true;
        self
    }

    /// Suppress a single [`crate::bundle::lint::FindingKind`] on this
    /// node. The bundle's lint pass will skip findings of that kind
    /// whose attribution target is this exact node — siblings,
    /// descendants, and ancestors are unaffected, so a stray
    /// suppression cannot silently swallow real bugs elsewhere in the
    /// tree. Chain to silence multiple kinds:
    /// `el.allow_lint(FindingKind::RawColor).allow_lint(FindingKind::MissingSurfaceFill)`.
    ///
    /// Reach for this when a finding is *genuinely intentional* in your
    /// app — a hand-rolled custom-shader surface where the raw color is
    /// the point, a deliberately bare `Panel` you'll fill later, a
    /// hover-reveal action whose hit-overflow collision is by design.
    /// If you find yourself sprinkling it widely, the lint is probably
    /// catching a real shape worth fixing.
    ///
    /// Whole-class suppression (e.g. silencing every
    /// [`crate::bundle::lint::FindingKind::DuplicateId`] at the bundle
    /// boundary) lives on the [`crate::bundle::lint::LintReport`]
    /// itself — see [`crate::bundle::lint::LintReport::retain`].
    ///
    /// **Dogfood:** stock widgets and the damascene showcase fixture do
    /// not call this — every finding raised inside damascene's own code
    /// gets fixed at the source.
    pub fn allow_lint(mut self, kind: crate::bundle::lint::FindingKind) -> Self {
        let list = self.allow_lint.get_or_insert_default();
        if !list.contains(&kind) {
            list.push(kind);
        }
        self
    }
}
