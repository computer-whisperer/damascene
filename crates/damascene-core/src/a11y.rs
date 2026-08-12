//! Accessibility surfaces shared by every host arrangement.
//!
//! Two halves live here (plan of record: `docs/ACCESSIBILITY_PLAN.md`):
//!
//! - The *user-preference* half: [`AccessibilityPreferences`], the CSS
//!   `prefers-*` media-feature family as a value the host pushes into
//!   the runtime.
//! - The *semantic* half: [`Role`] and [`A11yProps`], the ARIA-shaped
//!   facts each [`El`](crate::tree::El) can carry (`.role(...)`,
//!   `.aria_label(...)`, state setters). Stock widgets self-annotate
//!   through the same public builders user widgets get. With the
//!   `accessibility` cargo feature enabled, the runtime lowers the
//!   laid-out tree into an AccessKit `TreeUpdate` for platform adapters
//!   and routes assistive-technology actions back through the normal
//!   event machinery.
//!
//! Direction of flow: the **host** detects platform/user settings
//! (media queries on web, desktop portal / OS settings natively, env
//! overrides for testing) and calls the runner's
//! `set_accessibility_preferences`. The runtime honors what it owns
//! (motion policy — see below), and **apps** read the rest during build
//! via [`BuildCx::accessibility`] to make theme-shaped decisions
//! (palette choice for [`ColorScheme`], contrast variants). This is the
//! reverse of [`ColorPreferences`], where the *app* declares wishes and
//! the host negotiates — don't conflate the two.
//!
//! What the runtime honors automatically when
//! [`AccessibilityPreferences::reduced_motion`] is `Some(true)`:
//!
//! - App-driven movement props (`scale`, `translate`) snap to their
//!   targets instead of easing — enter transitions keep their opacity
//!   fade but lose zoom/slide, the toast exit keeps its fade but loses
//!   the sink, tap-bounces disappear.
//! - Viewport smooth navigation ([`ViewportRequest`] flights) becomes
//!   instant.
//! - Scene3D camera retargets (refocus / re-fit glides) snap to pose.
//!
//! Color and opacity easing (hover/press mixes, focus-ring fade, plain
//! fades), the caret blink, spinners, and time-driven shader uniforms
//! stay live: reduced motion targets *movement* — the
//! vestibular-trigger class — not essential feedback. This is a
//! deliberately different lever from
//! [`AnimationMode::Settled`](crate::state::AnimationMode), which
//! freezes everything for headless determinism.
//!
//! [`BuildCx::accessibility`]: crate::event::BuildCx::accessibility
//! [`ColorPreferences`]: crate::color::ColorPreferences
//! [`ViewportRequest`]: crate::viewport::ViewportRequest

#[cfg(feature = "accessibility")]
pub mod accesskit;

/// User accessibility preferences reported by the host — the CSS
/// `prefers-*` media-feature family as a value.
///
/// Every field is an `Option`: `None` means the host doesn't know (or
/// the platform reports no preference), which is also the [`Default`].
/// Hosts construct via `Default` and set what they can detect:
///
/// ```
/// use damascene_core::a11y::{AccessibilityPreferences, ColorScheme};
///
/// let prefs = AccessibilityPreferences {
///     reduced_motion: Some(true),
///     color_scheme: Some(ColorScheme::Dark),
///     ..Default::default()
/// };
/// ```
///
/// Push with the runner's `set_accessibility_preferences` whenever a
/// value changes (hosts re-push the whole struct; there is no per-field
/// delta). Apps read the current value with
/// [`BuildCx::accessibility`](crate::event::BuildCx::accessibility) /
/// [`EventCx::accessibility`](crate::event::EventCx::accessibility).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessibilityPreferences {
    /// `prefers-reduced-motion` — `Some(true)` when the user asked the
    /// platform to minimize non-essential motion. The runtime honors
    /// this for library-owned movement automatically (see the module
    /// docs); apps additionally gate their own decorative motion on
    /// [`BuildCx::reduced_motion`](crate::event::BuildCx::reduced_motion).
    pub reduced_motion: Option<bool>,
    /// `prefers-color-scheme` — the user's light/dark preference. The
    /// runtime does not act on this (the app owns its [`Theme`]); read
    /// it during build to pick a palette.
    ///
    /// [`Theme`]: crate::Theme
    pub color_scheme: Option<ColorScheme>,
    /// `prefers-contrast` — the user asked for more or less contrast
    /// than the default. The runtime does not act on this; read it
    /// during build to pick a higher-contrast palette or strengthen
    /// borders. (Forced-colors mode is a future, separate field — it
    /// carries a whole system palette, not a direction.)
    pub contrast: Option<Contrast>,
    /// `prefers-reduced-transparency` — the user asked to minimize
    /// translucent surfaces. The runtime does not act on this; apps
    /// with translucent chrome (scrims, glassy panels) read it and
    /// opaque up.
    pub reduced_transparency: Option<bool>,
    /// Whether an assistive technology that consumes the accessibility
    /// tree (screen reader, voice control, switch access) is actively
    /// connected. Hosts with an AccessKit adapter report activation
    /// flips here; `None` means the host can't tell. Apps use it to
    /// keep transient UI around (suppress toast auto-dismiss), prefer
    /// explicit text over purely visual cues, etc.
    pub screen_reader_active: Option<bool>,
}

impl AccessibilityPreferences {
    /// `true` iff the user explicitly prefers reduced motion.
    /// (`None` — unknown — reads as `false`, matching the web's
    /// `no-preference` default.)
    pub fn prefers_reduced_motion(&self) -> bool {
        self.reduced_motion == Some(true)
    }
}

/// The user's `prefers-color-scheme` value: which of light or dark the
/// platform is set to. No `NoPreference` variant — that state is the
/// enclosing `Option` being `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorScheme {
    /// The platform is set to a light appearance.
    Light,
    /// The platform is set to a dark appearance.
    Dark,
}

/// The user's `prefers-contrast` direction. Mirrors the CSS values
/// `more` / `less`; the no-preference state is the enclosing `Option`
/// being `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contrast {
    /// The user asked for higher contrast (e.g. macOS "Increase
    /// contrast", GNOME high-contrast, `prefers-contrast: more`).
    More,
    /// The user asked for lower contrast (`prefers-contrast: less`).
    Less,
}

/// Semantic role of a node in the accessibility tree — what a screen
/// reader announces the element *as*. Variant names follow the WAI-ARIA
/// `role` attribute values (`role="checkbox"` → [`Role::Checkbox`],
/// `role="img"` → [`Role::Img`]), so web-trained authors can transfer
/// the vocabulary directly. Set with [`El::role`](crate::tree::El::role);
/// stock widgets set their own role, so app code usually only reaches
/// for this in custom widgets.
///
/// The list is the subset the stock widgets and common custom widgets
/// need — not all of ARIA. It is `#[non_exhaustive]`: more roles are
/// added as widgets need them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// `role="button"` — activatable command element.
    Button,
    /// `role="checkbox"` — two-state check control (pair with
    /// [`El::aria_checked`](crate::tree::El::aria_checked)).
    Checkbox,
    /// `role="switch"` — on/off toggle (pair with `aria_checked`).
    Switch,
    /// `role="radio"` — one option in a radio group.
    Radio,
    /// `role="radiogroup"` — container of radios.
    RadioGroup,
    /// `role="slider"` — draggable value input (pair with
    /// [`El::aria_value`](crate::tree::El::aria_value)).
    Slider,
    /// `role="spinbutton"` — steppable numeric input.
    SpinButton,
    /// `role="textbox"` — text entry (single- or multi-line).
    Textbox,
    /// `role="tab"` — one tab in a tablist (pair with `aria_selected`).
    Tab,
    /// `role="tablist"` — container of tabs.
    TabList,
    /// `role="tabpanel"` — content panel a tab controls.
    TabPanel,
    /// `role="menu"` — menu of commands.
    Menu,
    /// `role="menubar"` — horizontal persistent menu container.
    MenuBar,
    /// `role="menuitem"` — one command in a menu.
    MenuItem,
    /// `role="menuitemcheckbox"` — checkable menu item.
    MenuItemCheckbox,
    /// `role="menuitemradio"` — exclusive-choice menu item.
    MenuItemRadio,
    /// `role="listbox"` — selectable option list (select menus).
    Listbox,
    /// `role="option"` — one option in a listbox.
    Option,
    /// `role="link"` — navigational link.
    Link,
    /// `role="heading"` — section heading (pair with
    /// [`El::aria_level`](crate::tree::El::aria_level)).
    Heading,
    /// `role="img"` — image content (pair with
    /// [`El::alt`](crate::tree::El::alt)).
    Img,
    /// `role="group"` — labelled grouping of related elements.
    Group,
    /// `role="dialog"` — dialog/window surface (pair with
    /// [`El::aria_modal`](crate::tree::El::aria_modal)).
    Dialog,
    /// `role="alertdialog"` — dialog that interrupts with a message.
    AlertDialog,
    /// `role="alert"` — important, usually time-sensitive message;
    /// implicitly live.
    Alert,
    /// `role="status"` — advisory live status (toasts, spinners).
    Status,
    /// `role="log"` — appended-to live region (chat/console output).
    Log,
    /// `role="progressbar"` — task-progress indicator (pair with
    /// `aria_value`, or none for indeterminate).
    ProgressBar,
    /// `role="tooltip"` — hover/focus description bubble.
    Tooltip,
    /// `role="list"` — list container.
    List,
    /// `role="listitem"` — one item in a list.
    ListItem,
    /// `role="table"` — static tabular data.
    Table,
    /// `role="row"` — row inside a table or grid.
    Row,
    /// `role="cell"` — data cell inside a table row.
    Cell,
    /// `role="columnheader"` — header cell for a column.
    ColumnHeader,
    /// `role="combobox"` — input controlling an associated popup
    /// (select triggers; pair with `aria_expanded`).
    Combobox,
    /// `role="separator"` — visual divider between sections.
    Separator,
    /// `role="toolbar"` — grouped action bar.
    Toolbar,
    /// `role="grid"` — interactive 2-D container (calendar months).
    Grid,
    /// `role="gridcell"` — cell inside a grid.
    GridCell,
    /// `role="figure"` — self-contained illustrative content
    /// (plots, 3-D scenes).
    Figure,
    /// `role="math"` — mathematical expression.
    Math,
    /// `role="paragraph"` — paragraph of static text.
    Paragraph,
    /// `role="presentation"` — strip this node's implicit semantics;
    /// descendants still participate.
    Presentation,
}

/// Live-region politeness — how urgently assistive technology should
/// announce changes inside the node. Mirrors the ARIA `aria-live`
/// values; set with [`El::aria_live`](crate::tree::El::aria_live).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveRegion {
    /// `aria-live="polite"` — announce at the next graceful
    /// opportunity (status lines, toasts).
    Polite,
    /// `aria-live="assertive"` — announce immediately, interrupting
    /// current speech (errors, alerts).
    Assertive,
}

/// Accessibility properties of one [`El`](crate::tree::El) — the
/// ARIA-shaped semantic facts a screen reader needs beyond layout and
/// paint. Boxed behind one pointer on `El` (`El::a11y`), allocated only
/// when a builder sets something, so nodes without semantics pay 8
/// bytes.
///
/// Authors don't construct this directly — the `El` builders
/// ([`role`](crate::tree::El::role),
/// [`aria_label`](crate::tree::El::aria_label), …) fill it in, and
/// stock widgets self-annotate. The accessible *name* follows the HTML
/// accname model: `label` when set, otherwise assistive technology
/// reads the node's visible text content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct A11yProps {
    /// Semantic role — what this element *is* to assistive technology.
    pub role: Option<Role>,
    /// Accessible-name override (ARIA `aria-label` / HTML `alt`).
    /// Unset, the name derives from visible text content.
    pub label: Option<String>,
    /// Supplementary description (ARIA `aria-describedby` content).
    /// When unset, a `.tooltip(...)` doubles as the description.
    pub description: Option<String>,
    /// Exclude this node and its subtree from the accessibility tree
    /// (ARIA `aria-hidden="true"`): decorative or duplicated content.
    pub hidden: bool,
    /// Announce content changes inside this node (ARIA `aria-live`).
    pub live: Option<LiveRegion>,
    /// Checked state for checkbox / switch / radio / menuitemcheckbox
    /// roles (ARIA `aria-checked`).
    pub checked: Option<bool>,
    /// Expanded/collapsed state for disclosure-style controls (ARIA
    /// `aria-expanded`): accordions, comboboxes, menus.
    pub expanded: Option<bool>,
    /// Selected state for tabs / options / rows (ARIA `aria-selected`).
    pub selected: Option<bool>,
    /// Pressed state for toggle buttons (ARIA `aria-pressed`).
    pub pressed: Option<bool>,
    /// Disabled to assistive technology (ARIA `aria-disabled`). The
    /// stock [`disabled()`](crate::tree::El::disabled) style modifier
    /// sets this alongside its visual/behavioral treatment.
    pub disabled: bool,
    /// Numeric value as `(now, min, max)` for slider / spinbutton /
    /// progressbar roles (ARIA `aria-valuenow/-min/-max`).
    pub value: Option<(f64, f64, f64)>,
    /// Human-readable value override (ARIA `aria-valuetext`), e.g.
    /// `"52%"` or `"March"` where the number alone is meaningless.
    pub value_text: Option<String>,
    /// Heading level 1–6 for [`Role::Heading`] (ARIA `aria-level`).
    pub level: Option<u8>,
    /// Modal surface (ARIA `aria-modal="true"`) — content behind it is
    /// inert while open. The focus-trap layer system already enforces
    /// the behavior; this declares it to assistive technology.
    pub modal: bool,
    /// Editable-text declaration for [`Role::Textbox`] nodes — the
    /// AccessKit text protocol's input. See [`EditableText`]. Boxed:
    /// only text widgets carry it.
    pub text_edit: Option<Box<EditableText>>,
}

/// Declaration of an editable-text widget's assistive-technology text
/// state — what the AccessKit lowering needs to speak the text
/// protocol (per-character `TextRun` children, caret/selection
/// reporting, `SetTextSelection` routing) for a [`Role::Textbox`]
/// node. Set with [`El::editable_text`](crate::tree::El::editable_text).
///
/// Stock widgets (`text_input`, `text_area`) stamp this themselves;
/// custom editable widgets use the same builder (symmetry invariant —
/// no stock-only powers). Without it a `Role::Textbox` node still
/// exposes role/name/value, but screen readers can't read it by
/// character/word/line or track its caret.
///
/// `value` is the *rendered* string — for password fields the bullet
/// mask, matching what a sighted user sees. When the rendered string
/// differs from the app's source-of-truth value, `source_offsets`
/// carries the byte mapping so caret positions round-trip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditableText {
    /// The rendered text assistive technology reads: the field's
    /// display string (bullets for masked fields), empty when the
    /// field is empty (never the placeholder).
    pub value: String,
    /// Whether the widget edits multiple lines (`<textarea>` /
    /// `aria-multiline`). Lowers the platform role to a multiline
    /// text input, which changes how screen readers navigate it.
    pub multiline: bool,
    /// Hint shown while the field is empty (HTML `placeholder`).
    /// Lowered as the platform placeholder property; the accessible
    /// *name* fallback is a separate concern the widget handles via
    /// `aria_label`.
    pub placeholder: Option<String>,
    /// Byte mapping between `value` (the rendered string) and the
    /// app's source-of-truth string, for widgets that render a
    /// transformed value (password masking). Sorted
    /// `(value_byte, source_byte)` boundary pairs covering both ends
    /// (`(0, 0)` … `(value.len(), source.len())`); positions between
    /// entries never land on an AT character boundary. `None` means
    /// the rendered string *is* the source string.
    pub source_offsets: Option<Box<[(u32, u32)]>>,
}

// Only the AccessKit lowering and action routing consume the byte
// mapping; the declaration itself is always present (widgets stamp it
// unconditionally — symmetry invariant).
#[cfg(feature = "accessibility")]
impl EditableText {
    /// Map a byte offset in the rendered `value` to the corresponding
    /// source-string offset (identity when `source_offsets` is
    /// `None`). Non-boundary offsets snap to the nearest mapped
    /// boundary at or below.
    pub(crate) fn visible_to_source(&self, byte: usize) -> usize {
        let Some(map) = &self.source_offsets else {
            return byte;
        };
        match map.binary_search_by_key(&(byte as u32), |(v, _)| *v) {
            Ok(i) => map[i].1 as usize,
            Err(0) => 0,
            Err(i) => map[i - 1].1 as usize,
        }
    }

    /// Inverse of [`Self::visible_to_source`].
    pub(crate) fn source_to_visible(&self, byte: usize) -> usize {
        let Some(map) = &self.source_offsets else {
            return byte;
        };
        match map.binary_search_by_key(&(byte as u32), |(_, s)| *s) {
            Ok(i) => map[i].0 as usize,
            Err(0) => 0,
            Err(i) => map[i - 1].0 as usize,
        }
    }
}

/// Roles whose accessible name derives from their text content when no
/// `aria_label` override is set — the HTML accname name-from-content
/// set. Shared by the AccessKit lowering (text leaves inside such a
/// node are absorbed into the name instead of emitted as separate
/// static-text nodes, so a `button("Save")` announces once) and the
/// [`NoAccessibleName`](crate::bundle::lint::FindingKind::NoAccessibleName)
/// lint, so the two can't disagree about what counts as named.
pub(crate) fn names_from_content(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::Checkbox
            | Role::Switch
            | Role::Radio
            | Role::Tab
            | Role::MenuItem
            | Role::MenuItemCheckbox
            | Role::MenuItemRadio
            | Role::Option
            | Role::Link
            | Role::Heading
            | Role::Tooltip
            | Role::Cell
            | Role::ColumnHeader
            | Role::GridCell
            | Role::ListItem
            | Role::Alert
            | Role::Status
            | Role::Paragraph
    )
}

/// Concatenate the visible text of `node` and its descendants
/// (skipping `aria_hidden` subtrees), the accname name-from-content
/// walk. Joined with single spaces; leading/trailing whitespace
/// trimmed per piece.
pub(crate) fn collect_text(node: &crate::tree::El, out: &mut String) {
    if node.a11y.as_deref().is_some_and(|p| p.hidden) {
        return;
    }
    if let Some(text) = &node.text {
        let piece = text.trim();
        if !piece.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(piece);
        }
    }
    for child in &node.children {
        collect_text(child, out);
    }
}

/// The node's accessible name as assistive technology would resolve it,
/// following the HTML accname order: explicit `aria_label`/`alt`, then
/// text content (for roleless nodes and [`names_from_content`] roles —
/// a roleless focusable's text children are emitted as separate static
/// text but still read as its name), then `.tooltip(...)` as the
/// last-resort fallback (HTML `title` behavior). `None` when nothing
/// names the node. Used by the `NoAccessibleName` / `ImageWithoutAlt`
/// lints; the AccessKit lowering composes the same primitives.
pub(crate) fn accessible_name(node: &crate::tree::El) -> Option<String> {
    if let Some(label) = node.a11y.as_deref().and_then(|p| p.label.clone()) {
        return (!label.trim().is_empty()).then_some(label);
    }
    let role = node.a11y.as_deref().and_then(|p| p.role);
    if role.is_none_or(names_from_content) {
        let mut text = String::new();
        collect_text(node, &mut text);
        if !text.is_empty() {
            return Some(text);
        }
    }
    node.tooltip
        .as_ref()
        .filter(|t| !t.trim().is_empty())
        .cloned()
}

#[cfg(test)]
mod tests {
    use crate::tree::{El, Kind};

    #[test]
    fn a11y_builders_allocate_lazily_and_compose() {
        let plain = El::new(Kind::Group);
        assert!(plain.a11y.is_none(), "no props until a builder runs");

        let el = El::new(Kind::Group)
            .role(super::Role::Checkbox)
            .aria_label("Enable telemetry")
            .aria_checked(true)
            .disabled();
        let props = el.a11y.as_deref().expect("props allocated");
        assert_eq!(props.role, Some(super::Role::Checkbox));
        assert_eq!(props.label.as_deref(), Some("Enable telemetry"));
        assert_eq!(props.checked, Some(true));
        assert!(props.disabled, ".disabled() stamps the semantic fact");
    }

    #[test]
    fn alt_is_the_label_field() {
        let el = El::new(Kind::Image).alt("Boarding pass QR code");
        assert_eq!(
            el.a11y.as_deref().and_then(|p| p.label.as_deref()),
            Some("Boarding pass QR code")
        );
    }
}
