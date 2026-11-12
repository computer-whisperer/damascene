//! Style modifier methods on [`El`] — kind-aware via [`StyleProfile`].
//!
//! Each component declares its [`StyleProfile`] in its constructor.
//! Style modifiers (`.primary`, `.success`, `.muted`, etc.) dispatch
//! on the profile, not on `Kind`. That means adding a new component
//! is a self-contained file change: declare a profile, the existing
//! modifier vocabulary just works.
//!
//! Profile semantics:
//!
//! - [`StyleProfile::Solid`] — color modifiers produce solid fills
//!   (Button, Toggle thumb, …).
//! - [`StyleProfile::Tinted`] — color modifiers produce tinted alpha
//!   fills with status-colored text (Badge, highlighted Card, …).
//! - [`StyleProfile::Surface`] — color modifiers tint a subtle bg;
//!   `.muted` swaps to a neutral surface (Card, TextField, Select, …).
//! - [`StyleProfile::TextOnly`] — color modifiers only change text color
//!   (Text, Heading, …).
//! - [`StyleProfile::Indicator`] — color modifiers recolour a marker
//!   child (the dot of a dot-plus-text `badge`) and leave the surface
//!   alone; the surface variants box it first.
//! - [`StyleProfile::Composite`] — the node is a frame around controls
//!   that carry profiles of their own (`button_group`, `split_button`);
//!   colour, surface and disabled modifiers apply to each child.
//!
//! Every colour and surface modifier also marks the node
//! `explicit_variant`, which is how a restyling context (a `toolbar`
//! ghosting its buttons) tells an author's choice from a recipe's
//! default. A surface variant that chose no fill (`ghost`, `outline`)
//! is *quiet*: a status modifier chained after it recolours its text
//! (and border) instead of refilling it, so `.ghost().destructive()`
//! is the red-text action and `.destructive()` alone the solid one.
//! A menu row is quiet by nature: a status on `menu_item` or
//! `dropdown_menu_item` colours its label and glyph and never fills
//! the row.
//!
//! Modifier groups in this file:
//!
//! - **Color/status:** `primary`, `success`, `warning`, `destructive`, `info`
//! - **Surface variants:** `secondary`, `ghost`, `outline`, `muted`, `boxed`
//! - **Semantic states:** `selected`, `current`, `disabled`, `invalid`, `loading`
//! - **Typography roles:** `caption`, `label`, `body`, `title`, `heading`, `display`, `code`
//! - **Text shape:** `bold`, `semibold`, `small`, `xsmall`, `color`

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::metrics::ComponentSize;
use crate::tokens;
use crate::tree::*;

/// How a component reacts to style/color modifiers.
///
/// Set once in the component's constructor; the modifier methods dispatch
/// on this rather than on [`Kind`], so adding a new component never
/// requires editing this file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StyleProfile {
    /// Color modifiers produce solid fills with contrasting text
    /// (Button, Toggle thumb, …).
    Solid,
    /// Color modifiers produce tinted low-alpha fills with
    /// status-colored text (Badge, highlighted Card, …).
    Tinted,
    /// Color modifiers tint a subtle background; `.muted()` swaps to a
    /// neutral surface (Card, TextField, Select, …).
    Surface,
    /// Color modifiers only change the text color (Text, Heading, …).
    /// The default profile.
    #[default]
    TextOnly,
    /// Color modifiers recolour the node's **indicator** — its
    /// `status_dot` children — and leave fill, stroke and text alone.
    /// The dot-plus-text [`badge`](crate::widgets::badge::badge) is
    /// this profile: `.success()` turns the dot green, the label stays
    /// the row's caption. A surface variant (`.boxed()`, `.outline()`,
    /// `.secondary()`, `.ghost()`) first converts the node to a
    /// [`Tinted`](Self::Tinted) chip, taking the dot's colour as the
    /// chip's status.
    Indicator,
    /// The node is a frame around controls with profiles of their own
    /// — [`button_group`](crate::widgets::button_group::button_group),
    /// [`split_button`](crate::widgets::button_group::split_button).
    /// Colour modifiers, surface variants and `.disabled()` apply to
    /// each child; the frame itself stays unpainted, so
    /// `split_button("pull", "Pull").primary()` tints both halves
    /// instead of colouring a label the frame does not have.
    Composite,
}

impl El {
    // ===== Color / status (profile-aware) =====

    /// Primary/brand treatment — applies [`tokens::PRIMARY`] per the
    /// element's [`StyleProfile`].
    pub fn primary(self) -> Self {
        // PRIMARY is a neutral (near-white / near-black), already
        // text-grade on its own tint — no separate text tone needed.
        tint(self, tokens::PRIMARY, tokens::PRIMARY)
    }
    /// Positive status treatment — applies [`tokens::SUCCESS`] per the
    /// element's [`StyleProfile`].
    pub fn success(self) -> Self {
        tint(self, tokens::SUCCESS, tokens::SUCCESS_TINT_FOREGROUND)
    }
    /// Cautionary status treatment — applies [`tokens::WARNING`] per
    /// the element's [`StyleProfile`].
    pub fn warning(self) -> Self {
        tint(self, tokens::WARNING, tokens::WARNING_TINT_FOREGROUND)
    }
    /// Destructive-action treatment — applies [`tokens::DESTRUCTIVE`]
    /// per the element's [`StyleProfile`].
    pub fn destructive(self) -> Self {
        tint(
            self,
            tokens::DESTRUCTIVE,
            tokens::DESTRUCTIVE_TINT_FOREGROUND,
        )
    }
    /// Informational status treatment — applies [`tokens::INFO`] per
    /// the element's [`StyleProfile`].
    pub fn info(self) -> Self {
        tint(self, tokens::INFO, tokens::INFO_TINT_FOREGROUND)
    }

    // ===== Surface variants =====

    /// Default-styled secondary surface. This is the default look for
    /// `button(...)`; calling `.secondary()` makes intent explicit.
    pub fn secondary(self) -> Self {
        if self.style_profile == StyleProfile::Composite {
            return map_children(self, El::secondary);
        }
        let mut this = self.boxed();
        this.explicit_variant = true;
        this.fill = Some(tokens::SECONDARY);
        this.stroke = Some(tokens::BORDER);
        this.stroke_width = 1.0;
        set_content_color(&mut this, tokens::SECONDARY_FOREGROUND);
        this.font_weight = FontWeight::Medium;
        keep_disabled(this)
    }

    /// Box an [`Indicator`](StyleProfile::Indicator) node into its
    /// [`Tinted`](StyleProfile::Tinted) chip form: the dot is removed
    /// and its colour becomes the chip's tint (`badge("Online")
    /// .success().boxed()` is the green pill; `.boxed().success()`
    /// is the same pill). On any other profile this is a no-op, so a
    /// chip can be boxed twice without changing. The surface variants
    /// call it themselves; reach for it directly when a status must
    /// read as a filled chip — the one failure that has to shout —
    /// rather than as a dot beside its label. A box means clickable to
    /// most readers (`docs/POLISH_REVIEW_2026_10_OPUS.md` §3), which is
    /// why the dot is the default.
    ///
    /// The chip is a **text leaf**, as shadcn's `Badge` is one element:
    /// when the dot leaves and a single text child remains, that child
    /// collapses into the node's own text (same role, weight and
    /// alignment), so `badge("3").secondary()` inspects, exports and
    /// stamps exactly as the pre-2026-10 badge did. An indicator with
    /// any other child set keeps its row.
    pub fn boxed(mut self) -> Self {
        if self.style_profile != StyleProfile::Indicator {
            return self;
        }
        let status = take_indicator_color(&mut self).unwrap_or(tokens::INFO);
        self.style_profile = StyleProfile::Tinted;
        if !self.explicit_padding {
            self.padding = Sides::xy(tokens::SPACE_2, 0.0);
        }
        if self.children.len() == 1 && self.children[0].text.is_some() {
            let label = self.children.remove(0);
            self.text = label.text;
            self.text_align = TextAlign::Center;
            self.font_weight = label.font_weight;
            self.font_mono = label.font_mono;
            self.explicit_mono = label.explicit_mono;
            self.text_letter_spacing = label.text_letter_spacing;
            self.text_color = label.text_color;
            self.text_role = label.text_role;
            self.font_size = label.font_size;
            self.line_height = label.line_height;
        }
        if status.token == Some("muted-foreground") {
            return self.muted();
        }
        tint_filled(self, status, text_on_tint(status))
    }

    /// No fill, no border. Low-emphasis actions like "Cancel" alongside
    /// a primary "Save".
    ///
    /// A status modifier chained after it recolours the text and
    /// nothing else: `button("Delete").ghost().destructive()` is the
    /// quiet red-text action (shadcn's `text-destructive` ghost), not
    /// a solid red button. Shape first, then status — the last
    /// modifier wins, so `.destructive().ghost()` ends muted.
    pub fn ghost(self) -> Self {
        if self.style_profile == StyleProfile::Composite {
            return map_children(self, El::ghost);
        }
        let mut this = self.boxed();
        this.explicit_variant = true;
        this.fill = None;
        this.stroke = None;
        this.stroke_width = 0.0;
        set_content_color(&mut this, tokens::MUTED_FOREGROUND);
        keep_disabled(this)
    }

    /// Outline-only style: no fill, prominent border, plain
    /// foreground content.
    ///
    /// The border token is profile-dependent, matching shadcn:
    /// controls (`Solid` / `Surface` — Button, Select, …) stroke with
    /// [`tokens::INPUT`], shadcn's `dark:border-input`; chips
    /// (`Tinted` — Badge) stroke with [`tokens::BORDER`], shadcn's
    /// `Badge` `variant="outline"` (`border-border text-foreground`).
    ///
    /// A status modifier chained after it keeps the outline shape and
    /// colours the border and the text with the status:
    /// `.outline().destructive()` is a red-bordered, red-text button
    /// with no fill.
    pub fn outline(self) -> Self {
        if self.style_profile == StyleProfile::Composite {
            return map_children(self, El::outline);
        }
        let mut this = self.boxed();
        this.explicit_variant = true;
        this.fill = None;
        this.stroke = Some(match this.style_profile {
            StyleProfile::Tinted => tokens::BORDER,
            _ => tokens::INPUT,
        });
        this.stroke_width = 1.0;
        set_content_color(&mut this, tokens::FOREGROUND);
        keep_disabled(this)
    }

    /// Muted/neutral emphasis. On surface profiles this swaps to a
    /// neutral background; on text-only profiles it switches the text
    /// color to muted-foreground; on an indicator it greys the dot
    /// (the "offline" state of a dot-plus-text badge).
    pub fn muted(mut self) -> Self {
        self.explicit_variant = true;
        match self.style_profile {
            StyleProfile::Solid | StyleProfile::Tinted | StyleProfile::Surface => {
                self.fill = Some(tokens::MUTED);
                self.stroke = Some(tokens::BORDER);
                self.stroke_width = 1.0;
                set_content_color(&mut self, tokens::MUTED_FOREGROUND);
            }
            StyleProfile::TextOnly => {
                set_content_color(&mut self, tokens::MUTED_FOREGROUND);
            }
            StyleProfile::Indicator => {
                set_indicator_color(&mut self, tokens::MUTED_FOREGROUND);
            }
            StyleProfile::Composite => {
                return map_children(self, El::muted);
            }
        }
        self
    }

    // ===== Semantic states =====

    /// Selected row/item treatment. Use for the item that is selected
    /// inside a collection, not for transient keyboard focus.
    ///
    /// Selected is a **neutral** raised fill — shadcn's selected
    /// `TableRow` is `data-[state=selected]:bg-muted` — with no
    /// outline and the plain foreground, on every surface (ruled
    /// 2026-10-14, `docs/POLISH_REVIEW_2026_10.md`). It never borrows
    /// the primary tint: a selected row has to read as *where you
    /// are*, and the one primary-coloured thing on a page has to stay
    /// the primary action. The sole accent is an [`item`]'s rail,
    /// which is a single marker rather than a filled surface.
    ///
    /// [`item`]: crate::widgets::item::item
    pub fn selected(mut self) -> Self {
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        } else {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Selected;
            self.fill = Some(tokens::MUTED);
            self.stroke = None;
            self.stroke_width = 0.0;
            set_content_color(&mut self, tokens::FOREGROUND);
            if matches!(self.kind, Kind::Custom("item")) {
                set_item_rail(&mut self, tokens::PRIMARY);
            }
        }
        // Selection is a semantic fact too — declare it to assistive
        // technology alongside the visual treatment.
        keep_disabled(self.aria_selected(true))
    }

    /// Current navigation/page treatment. Slightly quieter than
    /// [`Self::selected`] so nav chrome does not compete with content.
    pub fn current(mut self) -> Self {
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        } else if matches!(self.kind, Kind::Custom("item")) {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Current;
            self.fill = Some(tokens::ACCENT.with_alpha_u8(24));
            self.stroke = Some(tokens::BORDER);
            self.stroke_width = 1.0;
            set_content_color(&mut self, tokens::FOREGROUND);
            set_item_rail(&mut self, tokens::PRIMARY);
        } else {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Current;
            self.fill = Some(tokens::ACCENT);
            self.stroke = Some(tokens::BORDER);
            self.stroke_width = 1.0;
            set_content_color(&mut self, tokens::ACCENT_FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        }
        keep_disabled(self)
    }

    /// Disabled treatment for controls and rows. Also removes the node
    /// from focus order, blocks pointer hits on this element, and
    /// declares [`Cursor::NotAllowed`](crate::cursor::Cursor::NotAllowed)
    /// so hovering a disabled control reads as inert.
    ///
    /// The look is **neutral, then faded**. Every disabled node takes
    /// `MUTED_FOREGROUND` content and is drawn at
    /// [`tokens::DISABLED_ALPHA`]; a painted surface — anything
    /// carrying a fill or a stroke — first goes neutral, `MUTED` fill
    /// and `BORDER` stroke.
    ///
    /// Neutral, because shadcn's `opacity-50` on the variant itself
    /// leaves a dim primary-coloured block that reads as *selected*,
    /// not as *unavailable* (ruled 2026-10-14,
    /// `docs/POLISH_REVIEW_2026_10.md`). Faded as well, because
    /// neutral alone is not enough: `muted` is the `secondary` fill in
    /// every stock palette and `muted-foreground` is a ghost button's
    /// label, so a neutral disabled button at full strength is an
    /// enabled secondary one with a ghost's label (ruled 2026-11-11).
    ///
    /// The fade never raises an opacity: a node already below
    /// `DISABLED_ALPHA` (a hidden indicator, say) stays where it is.
    pub fn disabled(mut self) -> Self {
        self.focusable = false;
        self.block_pointer = true;
        self.cursor = Some(crate::cursor::Cursor::NotAllowed);
        if self.style_profile == StyleProfile::Composite {
            // The frame goes inert; each control inside neutralises
            // on its own terms.
            return map_children(self, El::disabled).aria_disabled(true);
        }
        if !text_only_leaf(&self) {
            if self.fill.is_some() {
                self.fill = Some(tokens::MUTED);
            }
            if self.stroke.is_some() {
                self.stroke = Some(tokens::BORDER);
            }
        }
        self.opacity = self.opacity.min(tokens::DISABLED_ALPHA);
        set_content_color(&mut self, tokens::MUTED_FOREGROUND);
        // Disabled is a semantic fact, not just a visual: declare it
        // to assistive technology alongside the treatment.
        self.aria_disabled(true)
    }

    /// Invalid/error treatment for inputs, rows, and validation badges.
    pub fn invalid(mut self) -> Self {
        if !text_only_leaf(&self) {
            self.style_profile = StyleProfile::Surface;
            // A role that supplies the node's fill stays: the stock
            // inputs author no fill of their own, so swapping their
            // `Input` role for `Danger` left an invalid field a red
            // outline around nothing. The destructive stroke below is
            // authored, and an authored stroke wins over the role's.
            if !self.surface_role.provides_fill() {
                self.surface_role = SurfaceRole::Danger;
            }
        }
        self.stroke = Some(tokens::DESTRUCTIVE);
        self.stroke_width = 1.0;
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::DESTRUCTIVE);
        }
        keep_disabled(self)
    }

    /// Loading treatment for a direct text-bearing node. Container
    /// widgets can still use this for opacity even when they do not
    /// have their own label text.
    pub fn loading(mut self) -> Self {
        self.opacity = self.opacity.min(0.78);
        if let Some(label) = &mut self.text {
            label.push_str("...");
        }
        self
    }

    // ===== Typography roles =====

    /// Set the typography role and apply its size/weight/color defaults.
    /// The named shorthands ([`Self::caption`], [`Self::body`], …) are
    /// the usual entry points.
    pub fn text_role(mut self, role: TextRole) -> Self {
        self.text_role = role;
        apply_text_role(&mut self);
        self
    }

    /// Caption role — [`tokens::TEXT_XS`], regular weight, muted-foreground text.
    pub fn caption(self) -> Self {
        self.text_role(TextRole::Caption)
    }

    /// Label role — [`tokens::TEXT_SM`], medium weight.
    pub fn label(self) -> Self {
        self.text_role(TextRole::Label)
    }

    /// Body role — [`tokens::TEXT_SM`], regular weight.
    pub fn body(self) -> Self {
        self.text_role(TextRole::Body)
    }

    /// Title role — [`tokens::TEXT_BASE`], semibold.
    pub fn title(self) -> Self {
        self.text_role(TextRole::Title)
    }

    /// Heading role — [`tokens::TEXT_2XL`], semibold.
    pub fn heading(self) -> Self {
        self.text_role(TextRole::Heading)
    }

    /// Display role — [`tokens::TEXT_3XL`], bold. The largest stock text role.
    pub fn display(self) -> Self {
        self.text_role(TextRole::Display)
    }

    // ===== Text shape =====

    /// Set the font weight to bold.
    pub fn bold(mut self) -> Self {
        self.font_weight = FontWeight::Bold;
        self
    }
    /// Set the font weight to semibold.
    pub fn semibold(mut self) -> Self {
        self.font_weight = FontWeight::Semibold;
        self
    }
    /// Compact sizing: text leaves drop to [`tokens::TEXT_SM`];
    /// components select their small (`Sm`) size variant.
    pub fn small(mut self) -> Self {
        if text_only_leaf(&self) {
            apply_type_token(&mut self, tokens::TEXT_SM);
        } else {
            self.component_size = Some(ComponentSize::Sm);
        }
        self
    }
    /// Extra-compact sizing: text leaves drop to [`tokens::TEXT_XS`];
    /// components select their extra-small (`Xs`) size variant.
    ///
    /// The end of this ladder: there is no `.xxsmall()`. The text branch
    /// has no rung below [`tokens::TEXT_XS`], and
    /// [`ComponentSize::Xxs`] is a *chrome* rung rather than a general
    /// step down — ask for it explicitly with
    /// `.size(ComponentSize::Xxs)` on the bars that want it.
    pub fn xsmall(mut self) -> Self {
        if text_only_leaf(&self) {
            apply_type_token(&mut self, tokens::TEXT_XS);
        } else {
            self.component_size = Some(ComponentSize::Xs);
        }
        self
    }
    /// Set an explicit text color.
    pub fn color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }
}

fn text_only_leaf(el: &El) -> bool {
    matches!(el.style_profile, StyleProfile::TextOnly) && el.text.is_some()
}

fn apply_type_token(el: &mut El, token: tokens::TypeToken) {
    el.font_size = token.size;
    el.line_height = token.line_height;
}

fn apply_text_role(el: &mut El) {
    // Non-Code roles default to the proportional face; explicit
    // `.mono()` (which sets `explicit_mono`) wins so the natural
    // reading order `text(s).mono().caption()` keeps the mono family.
    // The Code role intentionally forces mono regardless — that's its
    // whole purpose, and the explicit override would only be set true,
    // never false, so there's no conflict to resolve.
    let clear_mono = |el: &mut El| {
        if !el.explicit_mono {
            el.font_mono = false;
        }
    };
    match el.text_role {
        TextRole::Body => {
            apply_type_token(el, tokens::TEXT_SM);
            el.font_weight = FontWeight::Regular;
            el.text_letter_spacing = 0.0;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Caption => {
            apply_type_token(el, tokens::TEXT_XS);
            el.font_weight = FontWeight::Regular;
            el.text_letter_spacing = 0.0;
            clear_mono(el);
            el.text_color = Some(tokens::MUTED_FOREGROUND);
        }
        TextRole::Label => {
            apply_type_token(el, tokens::TEXT_SM);
            el.font_weight = FontWeight::Medium;
            el.text_letter_spacing = 0.0;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Title => {
            apply_type_token(el, tokens::TEXT_BASE);
            el.font_weight = FontWeight::Semibold;
            el.text_letter_spacing = tokens::TRACKING_TIGHT_EM * tokens::TEXT_BASE.size;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Heading => {
            apply_type_token(el, tokens::TEXT_2XL);
            el.font_weight = FontWeight::Semibold;
            el.text_letter_spacing = tokens::TRACKING_TIGHT_EM * tokens::TEXT_2XL.size;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Display => {
            apply_type_token(el, tokens::TEXT_3XL);
            el.font_weight = FontWeight::Bold;
            el.text_letter_spacing = tokens::TRACKING_TIGHT_EM * tokens::TEXT_3XL.size;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Code => {
            apply_type_token(el, tokens::TEXT_XS);
            el.font_weight = FontWeight::Regular;
            el.text_letter_spacing = 0.0;
            el.font_mono = true;
            el.text_color = Some(tokens::FOREGROUND);
        }
    }
}

/// Apply the status color `c` per the element's [`StyleProfile`].
/// `text` is the *text-grade* counterpart (`*_TINT_FOREGROUND`): the
/// base status colors are fill-grade and fail WCAG AA when used as
/// text over their own 15%-alpha tints or the page, so every profile
/// that paints status-colored *text* on an app surface uses `text`
/// instead. Solid fills keep the dedicated on-solid foreground.
fn tint(el: El, c: Color, text: Color) -> El {
    let tinted = if is_menu_row(&el) {
        tint_menu_row(el, text)
    } else if is_quiet_variant(&el)
        && matches!(
            el.style_profile,
            StyleProfile::Solid | StyleProfile::Tinted | StyleProfile::Surface
        )
    {
        tint_quiet(el, c, text)
    } else {
        tint_filled(el, c, text)
    };
    keep_disabled(tinted)
}

/// Disabled is not a variant the next one replaces: a colour or
/// surface modifier chained *after* `.disabled()` would otherwise
/// repaint the node in its own colours at the disabled opacity —
/// `button("Save").disabled().primary()` a dim primary block, the look
/// the neutral rule exists to prevent. Every such modifier ends here,
/// so both orders give the same node.
fn keep_disabled(el: El) -> El {
    if el.is_disabled() { el.disabled() } else { el }
}

/// The status on a quiet variant: `.ghost()` / `.outline()` chose a
/// shape without a fill, so the status recolours the text (and an
/// outline's border) and leaves the shape alone — `ghost()
/// .destructive()` is the red-text quiet action, not a solid red
/// button.
fn tint_quiet(mut el: El, c: Color, text: Color) -> El {
    el.explicit_variant = true;
    if el.stroke.is_some() {
        el.stroke = Some(c);
    }
    set_content_color(&mut el, text);
    el
}

/// The status on a menu row: the label and the glyph take the colour
/// and the row keeps its shape and its hover, so `menu_item("Delete")
/// .destructive()` is the red-text entry of a menu, never a solid red
/// bar — shadcn's `DropdownMenuItem variant="destructive"`. A muted
/// text child (the trailing shortcut hint) stays muted, as it does
/// there.
fn tint_menu_row(mut el: El, text: Color) -> El {
    el.explicit_variant = true;
    el.text_color = Some(text);
    let muted = tokens::MUTED_FOREGROUND.token;
    for child in &mut el.children {
        let is_hint = child.icon.is_none() && child.text_color.is_some_and(|c| c.token == muted);
        if child.icon.is_some() || (child.text.is_some() && !is_hint) {
            child.text_color = Some(text);
        }
    }
    el
}

/// A row of a menu ([`menu_item`](crate::widgets::popover::menu_item),
/// `dropdown_menu_item`, `menubar_item`, `command_item`) that no
/// surface variant has been applied to. A fill alone does not make a
/// variant: `command_item`'s recipe carries one of its own.
fn is_menu_row(el: &El) -> bool {
    el.metrics_role == Some(crate::metrics::MetricsRole::MenuItem)
        && (el.fill.is_none() || !el.explicit_variant)
}

/// The status on a filled profile — the surface takes the colour.
/// [`El::boxed`] calls this directly: the dot badge it is boxing has
/// no fill yet and may already carry a status, which is not a quiet
/// variant but a chip in the making.
fn tint_filled(mut el: El, c: Color, text: Color) -> El {
    el.explicit_variant = true;
    match el.style_profile {
        StyleProfile::Solid => {
            el.fill = Some(c);
            el.stroke = Some(c);
            el.stroke_width = 1.0;
            set_content_color(&mut el, text_on_solid(c));
            el.font_weight = FontWeight::Semibold;
        }
        StyleProfile::Tinted => {
            el.fill = Some(c.with_alpha_u8(38));
            el.stroke = Some(c.with_alpha_u8(120));
            el.stroke_width = 1.0;
            set_content_color(&mut el, text);
        }
        StyleProfile::Surface => {
            el.fill = Some(c.with_alpha_u8(38));
            el.stroke = Some(c.with_alpha_u8(120));
            el.stroke_width = 1.0;
            set_content_color(&mut el, text);
        }
        StyleProfile::TextOnly => {
            set_content_color(&mut el, text);
        }
        StyleProfile::Indicator => {
            set_indicator_color(&mut el, c);
        }
        StyleProfile::Composite => {
            return map_children(el, |child| tint(child, c, text));
        }
    }
    el
}

/// A surface variant that chose no fill — `.ghost()` or `.outline()`,
/// the only variant modifiers that leave `fill` empty. Status
/// modifiers recolour these rather than refill them. The
/// `explicit_variant` half keeps a constructor that simply has not
/// stamped a fill yet (a text area) on the filled path.
fn is_quiet_variant(el: &El) -> bool {
    el.explicit_variant && el.fill.is_none()
}

/// Apply a modifier to each child of a [`StyleProfile::Composite`]
/// frame, leaving the frame itself as it is.
fn map_children(mut el: El, f: impl Fn(El) -> El) -> El {
    for child in &mut el.children {
        *child = f(std::mem::take(child));
    }
    el
}

/// Recolour the indicator children (`status_dot`s) of an
/// [`StyleProfile::Indicator`] node; everything else is untouched.
fn set_indicator_color(el: &mut El, color: Color) {
    for child in &mut el.children {
        if matches!(child.kind, Kind::Custom("status_dot")) {
            child.fill = Some(color);
        }
    }
}

/// Remove the indicator children and report the colour the first one
/// carried — the status a boxed chip inherits.
fn take_indicator_color(el: &mut El) -> Option<Color> {
    let mut status = None;
    el.children.retain(|child| {
        if matches!(child.kind, Kind::Custom("status_dot")) {
            if status.is_none() {
                status = child.fill;
            }
            false
        } else {
            true
        }
    });
    status
}

/// The text-grade sibling of a status fill, for a chip's label — the
/// same pairing the status modifiers hand to [`tint`].
fn text_on_tint(c: Color) -> Color {
    match c.token {
        Some("primary") => tokens::PRIMARY,
        Some("success") => tokens::SUCCESS_TINT_FOREGROUND,
        Some("warning") => tokens::WARNING_TINT_FOREGROUND,
        Some("destructive") => tokens::DESTRUCTIVE_TINT_FOREGROUND,
        Some("info") => tokens::INFO_TINT_FOREGROUND,
        _ => c,
    }
}

fn set_content_color(el: &mut El, color: Color) {
    el.text_color = Some(color);
    for child in &mut el.children {
        if child.text.is_some() || child.icon.is_some() {
            child.text_color = Some(color);
        }
    }
}

fn set_item_rail(el: &mut El, color: Color) {
    for child in &mut el.children {
        if matches!(child.kind, Kind::Custom("item_rail")) {
            child.fill = Some(color);
            child.opacity = 1.0;
        }
    }
}

/// Pick a contrasting text color for a solid background fill.
///
/// Rec. 601 luminance threshold tuned so light/saturated fills (accent
/// blue, success green, warning yellow) get dark text, and darker
/// saturated fills (destructive red) get light text.
fn text_on_solid(c: Color) -> Color {
    match c.token {
        Some("primary") => return tokens::PRIMARY_FOREGROUND,
        Some("secondary") => return tokens::SECONDARY_FOREGROUND,
        Some("accent") => return tokens::ACCENT_FOREGROUND,
        Some("destructive") => return tokens::DESTRUCTIVE_FOREGROUND,
        Some("success") => return tokens::SUCCESS_FOREGROUND,
        Some("warning") => return tokens::WARNING_FOREGROUND,
        Some("info") => return tokens::INFO_FOREGROUND,
        _ => {}
    }

    let srgb = c.convert_to(crate::color::ColorSpace::SRGB);
    let lum = 0.299 * srgb.r + 0.587 * srgb.g + 0.114 * srgb.b;
    if lum > 150.0 / 255.0 {
        Color::srgb_u8a(8, 16, 25, 255)
    } else {
        Color::srgb_u8a(250, 250, 252, 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, button_with_icon, icon_button, row, text};

    #[test]
    fn text_on_solid_contrast_follows_fill_luminance() {
        // Regression: the threshold once compared 0..1 float luminance
        // against 150.0 (a u8-scale constant), so *every* untokened
        // fill got light text — including near-white ones.
        let on_light = text_on_solid(Color::srgb_u8(240, 240, 240));
        let on_dark = text_on_solid(Color::srgb_u8(30, 30, 30));
        assert!(
            on_light.g < 0.5,
            "light fill must get dark text, got g={}",
            on_light.g
        );
        assert!(
            on_dark.g > 0.5,
            "dark fill must get light text, got g={}",
            on_dark.g
        );
    }

    #[test]
    fn selected_is_a_neutral_fill_with_no_outline() {
        // Ruled 2026-10-14: selected never borrows the primary tint.
        let el = row([text("Selected")]).selected();
        assert_eq!(el.fill, Some(tokens::MUTED));
        assert!(el.stroke.is_none());
        assert_eq!(el.stroke_width, 0.0);
        assert_eq!(el.surface_role, SurfaceRole::Selected);
        assert_eq!(el.text_color, Some(tokens::FOREGROUND));
        assert_eq!(el.children[0].text_color, Some(tokens::FOREGROUND));

        // A bare text leaf marks selection by weight, not by colour.
        let leaf = text("Selected").selected();
        assert_eq!(leaf.text_color, Some(tokens::FOREGROUND));
        assert_eq!(leaf.font_weight, FontWeight::Semibold);
    }

    #[test]
    fn current_marks_container_as_selected_surface_role() {
        let el = row([text("Current")]).current();
        assert_eq!(el.fill, Some(tokens::ACCENT));
        assert_eq!(el.stroke, Some(tokens::BORDER));
        assert_eq!(el.surface_role, SurfaceRole::Current);
        assert_eq!(el.style_profile, StyleProfile::Surface);
    }

    #[test]
    fn disabled_removes_focus_and_neutralises_a_painted_control() {
        // Ruled 2026-10-14: a filled control goes MUTED / BORDER /
        // MUTED_FOREGROUND, not the variant at 50%. Ruled 2026-11-11:
        // and is then faded like every other disabled node, so it
        // cannot be taken for an enabled secondary button.
        for el in [
            button("Disabled").disabled(),
            button("Disabled").primary().disabled(),
            button("Disabled").destructive().disabled(),
            button_with_icon("upload", "Disabled").primary().disabled(),
        ] {
            assert!(!el.focusable);
            assert!(el.block_pointer);
            assert_eq!(el.opacity, tokens::DISABLED_ALPHA);
            assert_eq!(el.fill, Some(tokens::MUTED));
            assert_eq!(el.stroke, Some(tokens::BORDER));
            assert_eq!(el.text_color, Some(tokens::MUTED_FOREGROUND));
            for child in el
                .children
                .iter()
                .filter(|c| c.text.is_some() || c.icon.is_some())
            {
                assert_eq!(child.text_color, Some(tokens::MUTED_FOREGROUND));
            }
        }
    }

    #[test]
    fn a_status_after_ghost_or_outline_recolours_instead_of_refilling() {
        let quiet = button("Delete").ghost().destructive();
        assert!(quiet.fill.is_none(), "ghost keeps no fill");
        assert!(quiet.stroke.is_none(), "ghost keeps no border");
        assert_eq!(
            quiet.text_color,
            Some(tokens::DESTRUCTIVE_TINT_FOREGROUND),
            "the text takes the text-grade status colour"
        );

        let outlined = button("Delete").outline().destructive();
        assert!(outlined.fill.is_none(), "outline keeps no fill");
        assert_eq!(
            outlined.stroke,
            Some(tokens::DESTRUCTIVE),
            "the border takes the status"
        );
        assert_eq!(
            outlined.text_color,
            Some(tokens::DESTRUCTIVE_TINT_FOREGROUND)
        );

        // The filled path is untouched, and the order matters: a
        // ghost chained after the status ends muted (last wins).
        assert_eq!(
            button("Delete").destructive().fill,
            Some(tokens::DESTRUCTIVE)
        );
        assert_eq!(
            button("Delete").destructive().ghost().text_color,
            Some(tokens::MUTED_FOREGROUND)
        );
    }

    #[test]
    fn a_ghost_destructive_button_passes_the_contrast_lint() {
        use crate::bundle::lint::FindingKind;
        let mut root = crate::column([button("Delete").key("del").ghost().destructive()])
            .padding(Sides::all(tokens::SPACE_4))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        let bundle = crate::bundle::artifact::render_bundle_themed(
            &mut root,
            Rect::new(0.0, 0.0, 320.0, 120.0),
            &crate::Theme::default(),
        );
        assert!(
            !bundle
                .lint
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::LowContrastText),
            "{}",
            bundle.lint.text()
        );
    }

    #[test]
    fn a_status_on_a_menu_row_colours_the_label_and_glyph_only() {
        use crate::bundle::lint::FindingKind;
        use crate::widgets::dropdown_menu::dropdown_menu_item_with_icon_and_shortcut;
        use crate::widgets::popover::{menu_item, popover_panel};

        let plain = menu_item("Delete").destructive();
        assert!(plain.fill.is_none(), "a menu row never fills solid");
        assert!(plain.stroke.is_none());
        assert_eq!(
            plain.children[0].text_color,
            Some(tokens::DESTRUCTIVE_TINT_FOREGROUND)
        );
        assert_eq!(plain.children[0].font_weight, FontWeight::Regular);

        let rich = dropdown_menu_item_with_icon_and_shortcut("trash-2", "Delete", "Del")
            .key("delete")
            .destructive();
        assert!(rich.fill.is_none());
        let [glyph, label, hint] = &rich.children[..] else {
            panic!("icon, label, shortcut");
        };
        assert_eq!(glyph.text_color, Some(tokens::DESTRUCTIVE_TINT_FOREGROUND));
        assert_eq!(label.text_color, Some(tokens::DESTRUCTIVE_TINT_FOREGROUND));
        assert_eq!(
            hint.text_color,
            Some(tokens::MUTED_FOREGROUND),
            "the shortcut hint stays muted"
        );

        // Text-grade on the menu's own surface, in the default theme.
        let mut root = crate::column([popover_panel([menu_item("Rename").key("rename"), rich])])
            .padding(Sides::all(tokens::SPACE_4))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        let bundle = crate::bundle::artifact::render_bundle_themed(
            &mut root,
            Rect::new(0.0, 0.0, 320.0, 160.0),
            &crate::Theme::default(),
        );
        assert!(
            !bundle
                .lint
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::LowContrastText),
            "{}",
            bundle.lint.text()
        );
    }

    #[test]
    fn disabled_dims_an_unpainted_node() {
        // Nothing to neutralise on a ghost button or a text leaf: the
        // opacity dim is what reads as unavailable there.
        let ghost = button("Cancel").ghost().disabled();
        assert!(ghost.fill.is_none());
        assert_eq!(ghost.opacity, tokens::DISABLED_ALPHA);
        assert_eq!(ghost.text_color, Some(tokens::MUTED_FOREGROUND));

        let leaf = text("n/a").disabled();
        assert_eq!(leaf.opacity, tokens::DISABLED_ALPHA);
        assert_eq!(leaf.text_color, Some(tokens::MUTED_FOREGROUND));
        assert!(!leaf.focusable);
    }

    #[test]
    fn disabled_reads_the_same_whichever_side_of_a_variant_it_is_on() {
        // `.disabled()` then a variant used to repaint the node in the
        // variant's colours at the disabled opacity.
        let look = |el: &El| (el.fill, el.stroke, el.text_color, el.opacity);
        type Variant = fn(El) -> El;
        let variants: [(&str, Variant); 7] = [
            ("primary", El::primary),
            ("destructive", El::destructive),
            ("secondary", El::secondary),
            ("outline", El::outline),
            ("ghost", El::ghost),
            ("selected", El::selected),
            ("invalid", El::invalid),
        ];
        for (name, variant) in variants {
            let after = variant(button("Save")).disabled();
            let before = variant(button("Save").disabled());
            assert_eq!(look(&before), look(&after), "{name}");
            assert!(before.is_disabled(), "{name}");
        }
    }

    #[test]
    fn invalid_keeps_the_trough_of_a_role_filled_input() {
        // The stock inputs author no fill; their `Input` role paints
        // the trough. `.invalid()` must not swap that role away.
        use crate::selection::Selection;
        use crate::widgets::text_input::text_input;
        let selection = Selection::default();
        let field = text_input("k", "v", &selection);
        assert_eq!(field.surface_role, SurfaceRole::Input);
        let invalid = field.invalid();
        assert_eq!(invalid.surface_role, SurfaceRole::Input);
        assert_eq!(invalid.stroke, Some(tokens::DESTRUCTIVE));
        // A node with no fill-providing role still takes Danger.
        let row = crate::row([text("x")]).key("r").invalid();
        assert_eq!(row.surface_role, SurfaceRole::Danger);
    }

    #[test]
    fn disabled_never_raises_an_opacity_and_applies_once() {
        // A hidden node stays hidden, painted or not; disabling twice
        // (an author's `.disabled()` under a composite that disables
        // its children) is the same as once.
        let hidden = text("n/a").opacity(0.0).disabled();
        assert_eq!(hidden.opacity, 0.0);
        let hidden_fill = button("Go").primary().opacity(0.2).disabled();
        assert_eq!(hidden_fill.opacity, 0.2);
        let twice = button("Go").primary().disabled().disabled();
        assert_eq!(twice.opacity, tokens::DISABLED_ALPHA);
    }

    #[test]
    fn icon_button_uses_same_solid_style_surface_as_button() {
        let el = icon_button("menu").primary();
        assert_eq!(el.icon, Some(crate::IconSource::Builtin(IconName::Menu)));
        assert_eq!(el.fill, Some(tokens::PRIMARY));
        assert_eq!(el.text_color, Some(text_on_solid(tokens::PRIMARY)));
        assert_eq!(el.surface_role, SurfaceRole::Raised);
    }

    #[test]
    fn button_with_icon_propagates_variant_content_color() {
        let el = button_with_icon("upload", "Publish").primary();
        assert_eq!(el.fill, Some(tokens::PRIMARY));
        assert_eq!(
            el.children[0].icon,
            Some(crate::IconSource::Builtin(IconName::Upload))
        );
        let expected = text_on_solid(tokens::PRIMARY);
        assert_eq!(el.children[0].text_color, Some(expected));
        assert_eq!(el.children[1].text.as_deref(), Some("Publish"));
        assert_eq!(el.children[1].text_color, Some(expected));
    }

    #[test]
    fn loading_appends_direct_label_text() {
        let el = button("Save").loading();
        assert_eq!(el.text.as_deref(), Some("Save..."));
        assert_eq!(el.opacity, 0.78);
    }

    #[test]
    fn text_roles_apply_inspectable_typographic_defaults() {
        let caption = text("Caption").caption();
        assert_eq!(caption.text_role, TextRole::Caption);
        assert_eq!(caption.font_size, tokens::TEXT_XS.size);
        assert_eq!(caption.line_height, tokens::TEXT_XS.line_height);
        assert_eq!(caption.text_color, Some(tokens::MUTED_FOREGROUND));

        let label = text("Label").label();
        assert_eq!(label.text_role, TextRole::Label);
        assert_eq!(label.font_size, tokens::TEXT_SM.size);
        assert_eq!(label.line_height, tokens::TEXT_SM.line_height);
        assert_eq!(label.font_weight, FontWeight::Medium);

        let code = text("Code").code();
        assert_eq!(code.text_role, TextRole::Code);
        assert_eq!(code.font_size, tokens::TEXT_XS.size);
        assert_eq!(code.line_height, tokens::TEXT_XS.line_height);
        assert_eq!(code.font_weight, FontWeight::Regular);
        assert_eq!(code.text_color, Some(tokens::FOREGROUND));
        assert!(code.font_mono);
    }

    #[test]
    fn explicit_mono_survives_subsequent_role_modifier() {
        // gh#12. The natural reading order `text(s).mono().caption()`
        // ("small mono caption") used to silently render in the
        // proportional face — `.caption()` reset `font_mono = false`
        // because non-Code roles bake the proportional family in. The
        // `explicit_mono` flag set by `.mono()` now suppresses that
        // reset for every non-Code role.
        let mono_first = text("+2").mono().caption();
        assert!(
            mono_first.font_mono,
            "`.mono()` chained before `.caption()` must keep mono on",
        );
        // Caption's other defaults still apply.
        assert_eq!(mono_first.font_size, tokens::TEXT_XS.size);
        assert_eq!(mono_first.text_color, Some(tokens::MUTED_FOREGROUND));

        // Reversed order — the canonical order — also keeps mono on.
        let role_first = text("+2").caption().mono();
        assert!(role_first.font_mono);

        // Same gating across the rest of the role family.
        for el in [
            text("+1").mono().body(),
            text("+1").mono().label(),
            text("+1").mono().title(),
            text("+1").mono().heading(),
            text("+1").mono().display(),
        ] {
            assert!(
                el.font_mono,
                "explicit .mono() must survive every non-Code role",
            );
        }

        // The Code role is unconditionally mono — no explicit_mono
        // gating needed, but verify nothing regressed.
        assert!(text("x").mono().code().font_mono);
    }
}

#[cfg(test)]
mod tracking_tests {
    use crate::tokens;
    use crate::tree::*;

    #[test]
    fn heading_roles_carry_tracking_tight_and_body_resets_it() {
        let h = crate::text("T").text_role(TextRole::Heading);
        assert_eq!(
            h.text_letter_spacing,
            tokens::TRACKING_TIGHT_EM * tokens::TEXT_2XL.size
        );
        let t = crate::text("T").text_role(TextRole::Title);
        assert_eq!(
            t.text_letter_spacing,
            tokens::TRACKING_TIGHT_EM * tokens::TEXT_BASE.size
        );
        // Switching a heading back to Body must not leak the tracking.
        let b = crate::text("T")
            .text_role(TextRole::Heading)
            .text_role(TextRole::Body);
        assert_eq!(b.text_letter_spacing, 0.0);
    }
}
