# Damascene — Widget Kit

> The contract for building widgets on top of Damascene. **Stock widgets get no privileged APIs that user widgets don't** — this document is the public surface that proves it.

## The symmetry invariant

If a stock widget (button, card, badge, alert, avatar, skeleton, dialog, sheet, modal, scroll, …) can do something, a widget defined in an app crate must be able to do the same thing using the same API. No backdoor methods on `El`, no internal-only fields, no library-side `match` on `Kind` that lights up behaviour for one variant but not another.

Stock widgets in `crates/damascene-core/src/widgets/` are reference compositions, not privileged code paths. An app can fork any of them and produce an equivalent widget without depending on internals. **`widgets/button.rs` is the dogfood proof** — it uses only the surface documented below.

**One caveat when forking stock source.** Constructors set their recipe through `default_*` setters — `default_padding`, `default_gap`, `default_radius`, `default_height` — which write a value *only if the caller hasn't already set it*. These are crate-internal; copying a stock constructor verbatim hits `E0624` (private method). The plain setters (`.padding`, `.gap`, `.radius`, `.height`) are the public equivalent: they force the value. **When forking, swap `.default_X(v)` → `.X(v)`.** The result is identical for an app widget, because your widget's constructor *is* the default layer — there's no further caller to defer to. The `default_*` layer only matters when you want *your* widget's callers to override a value you set; if you need that, take the value as a constructor parameter (the same shape stock widgets like `slider_with_color` use) rather than reaching for the internal setter.

## What's in the kit

A widget is a function (or struct + builder) that returns an [`El`]. To make widgets that look and behave like stock widgets, you have these things to work with — nothing else, nothing less:

### 1. The `El` builder

The whole grammar from `crates/damascene-core/src/tree/`. Sizing (`width`, `height`, `padding`, `gap`, `axis`, `align`, `justify`, `size`, `metrics_role`), visuals (`fill`, `stroke`, `stroke_width`, `radius`, `shadow`, `surface_role`), text (`text`, `text_color`, `text_align`, `text_role`, `font_size`, `font_weight`, `mono`, `italic`, `underline`, `strikethrough`, `link`, `wrap_text`, `text_overflow`, `ellipsis`, `max_lines`), icons (`icon`, `icon_name`, `icon_size`, `icon_stroke_width`), the paint-time transforms (`opacity`, `translate`, `scale`, `animate`), and the cross-cutting flags `clip()` (scissor children to this node's painted rect) and `scrollable()` (route wheel events to this node so it can scroll). `Kind::Scroll` already turns both on; `clip()` and `scrollable()` are the primitives behind it, available to any user widget that wants the same behaviour without claiming the structural variant.

### 1.1 Layout — sizing, alignment, container axes

Containers are El factories with axis + CSS-like defaults. `column([...])` is
`axis = Column, align = Stretch, height = Hug`; `row([...])` is
`axis = Row, align = Stretch, height = Hug`; `stack([...])` is
`axis = Overlay`. Each container has a **main axis** (the axis its children flow
along) and a **cross axis** (perpendicular). Both `column` and `row` default to
`Hug` on their main axis. To make a column or row claim its parent's extent, set
`.width(Size::Fill(1.0))` / `.height(Size::Fill(1.0))` explicitly.

Each child has a `Size` intent on each axis:

- `Fixed(px)` — exact size.
- `Hug` — intrinsic size of the child's content.
- `Fill(weight)` — claim a share of leftover space.

On the **main axis**, Fill siblings split leftover space proportional to weight. On the **cross axis**, Fill always claims the container's full extent — `Align` does not affect Fill children because there is no slack to position. `Align` positions Hug/Fixed children that are smaller than the container.

`Justify` distributes leftover main-axis space (`Start` / `Center` / `End` / `SpaceBetween`).

```rust
// Sidebar + content, both filling viewport height. The row's
// `Center` align is fine — Fill children fill regardless.
row([sidebar(), content()])
    .gap(tokens::SPACE_4)
    .height(Size::Fill(1.0))

// Card row of icon + text + button. `align(Center)` is the
// Tailwind `items-center` equivalent for everyday content rows.
row([icon("settings"), label, button("Edit")])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .align(Align::Center)

// Two-pane fill: left pane gets 1/3, right gets 2/3.
column([
    left_pane().height(Size::Fill(1.0)),
    right_pane().height(Size::Fill(2.0)),
])
```

Common pitfalls to avoid:

- **A normal icon/text/action row usually wants `.align(Align::Center)`.**
  `row()` follows CSS flexbox and defaults to cross-axis stretch. Stock widgets
  set center alignment where they need it, but app-authored rows should spell
  out the familiar `items-center` intent.
- **A `Fill`-cross-axis child neutralizes the parent's `align`.** `align(Center)` only positions children that have slack to be positioned — Fill claims the full extent, so it's a no-op for that child. Where the visible content sits inside a Fill child is then determined by the *child's own* main-axis `justify` (which defaults to `Start`). Symptom: in a row of `align(Center)` siblings, a `Fill`-height column appears to "stick to the top" because its content top-aligns inside the box. Fix: `.height(Size::Hug)` on the inner column, so it sizes to content and the parent center alignment has slack to work with. (`column()` and `row()` now both default to `Hug` on their non-fill axis, which makes this the easy path. The footgun only resurfaces if you explicitly set `Fill` on the cross axis.)
- **Two `Fill` siblings in a column will split the column's height proportionally to weight** — give one of them `.height(Size::Hug)` if it should size to content (panel header above scrollable body, etc).
- **A row of full-height columns needs `.height(Size::Fill(1.0))` on the row itself.** Row defaults to `Hug` height, so it shrinks to its tallest child's hug height; nested `Fill`-height children then have nothing to fill.
- **`stack()` (overlay) children share the parent's rect.** Use it for layered visuals (focus rings, tooltips) — not as a generic container. Z-order is child order.

Shortcuts: `.fill_size()` for `.width(Fill(1.0)).height(Fill(1.0))`; `.hug()` for both Hug. `.padding(Sides::xy(h, v))` for asymmetric padding.

### 1.2 Component size

Two independent knobs: `ComponentSize` scales control height / icon
size on the size-driven widgets (button / tab / input / badge / …);
container padding is per-constructor and overridden at the call site
(see §1.2.1). There is no global density knob.

Stock controls have a t-shirt size that matches shadcn's `size` prop
1:1: `ComponentSize::{Xs, Sm, Md, Lg}`. Local modifiers:

```rust
button("Preview").small()
button("Publish").large()
text_input(&query, &selection, "search").size(ComponentSize::Sm)
progress(value).small()
```

Damascene's built-in default starts at `ComponentSize::Sm` so desktop apps
land in a denser baseline. Use
`Theme::damascene_dark().with_default_component_size(ComponentSize::Md)`
to bump everything one rung, or
`Theme::damascene_dark().with_button_size(ComponentSize::Md)` to scope a
size override to a single control class.

Explicit layout calls always win. If an app writes
`.height(Size::Fixed(44.0))` or `.padding(20.0)`, the metrics pass
leaves that author choice alone. Custom widgets opt into stock control
sizing by setting `.metrics_role(...)` to one of the stock
`MetricsRole`s — `Button` / `IconButton` / `Input` / `TextArea` /
`Badge` for control-like surfaces, `TabTrigger` / `ChoiceControl` for
size-driven children, and `Slider` / `Progress` for range indicators.

### 1.2.1 Page rhythm and per-call padding

There is **no global density knob**. Container surfaces (card / form /
list / menu / table / panel / tabs / text-area) bake their padding /
gap / height / radius recipes directly in their constructors —
shadcn's stock recipe, visible at the call site. The metrics pass does
not override them. Override per-call, Tailwind-shaped:

```rust
card_header([card_title("Documents")])              // shadcn p-6 baked in
card_header([card_title("Settings")]).padding(tokens::SPACE_4)  // p-4 override
card_content([scroll([...])]).padding(0.0)          // flush scroll
card_content([rows]).pt(tokens::SPACE_2)            // p-6 + pt-2 (= shadcn p-6 pt-0 idiom)
accordion_trigger(...)                              // 40px tall, SPACE_3 sides baked in
accordion_trigger(...).height(Size::Fixed(32.0))    // override one widget
```

Page-level rhythm (page padding, section gaps, cluster gaps) is also
not configured by a knob — pick a `tokens::SPACE_*` constant where you
need it, the way a Tailwind author writes `p-8` / `space-y-6` /
`gap-4`. The token ladder maps 1:1 to Tailwind's spacing scale
(`SPACE_4` = `16` = `p-4`, `SPACE_6` = `24` = `p-6`, `SPACE_8` = `32`
= `p-8`). For a typical shadcn-shaped app shell:

```rust
column([
    toolbar([toolbar_title("Documents"), spacer(), button("New").primary()]),
    /* content */
])
.padding(tokens::SPACE_8)       // p-8 page chrome
.gap(tokens::SPACE_6)           // space-y-6 between sections
```

### 1.3 Typography family

Damascene treats the proportional UI font as a theme default, not a random
renderer detail. The default is Inter; Roboto is available as a
Material-style/compatibility alternate via the `roboto` Cargo feature:

```rust
Theme::damascene_dark().with_font_family(FontFamily::Inter)
Theme::damascene_dark().with_font_family(FontFamily::Roboto) // requires `roboto` feature
```

Text nodes inherit the theme family before layout, so intrinsic sizes,
wrapping, ellipsis, SVG artifacts, and backend glyph shaping agree.
Local text can still opt out with `.font_family(...)`, or use the
convenience shorthands `.inter()` and `.roboto()`.

Run `cargo run -p damascene-core --example font_family_comparison
--features roboto` to regenerate the current Roboto/Inter comparison
fixture.

### 1.4 Color vocabulary

Color tokens intentionally mirror the shadcn/Tailwind semantic split.
Use the role name, not the old implementation name:

```rust
tokens::BACKGROUND
tokens::FOREGROUND
tokens::CARD
tokens::CARD_FOREGROUND
tokens::POPOVER
tokens::POPOVER_FOREGROUND
tokens::PRIMARY
tokens::PRIMARY_FOREGROUND
tokens::SECONDARY
tokens::SECONDARY_FOREGROUND
tokens::MUTED
tokens::MUTED_FOREGROUND
tokens::ACCENT
tokens::ACCENT_FOREGROUND
tokens::DESTRUCTIVE
tokens::DESTRUCTIVE_FOREGROUND
tokens::BORDER
tokens::INPUT
tokens::RING
tokens::SUCCESS
tokens::SUCCESS_FOREGROUND
tokens::WARNING
tokens::WARNING_FOREGROUND
tokens::INFO
tokens::INFO_FOREGROUND
```

The paired `*-FOREGROUND` tokens are for solid fills. A primary button
uses `PRIMARY` plus `PRIMARY_FOREGROUND`; a secondary button uses
`SECONDARY` plus `SECONDARY_FOREGROUND`. `BORDER` is the normal
separator/card stroke, `INPUT` is the stronger control stroke, and
`RING` is the focus outline color. Link, scrollbar, overlay, and
selection colors are extension tokens because they describe a specific
component/domain rather than the reusable core palette.

Theme metrics can tune the global page rhythm and per-control-class sizes:

```rust
Theme::damascene_dark()
    .with_default_component_size(ComponentSize::Md)  // bump every control one size
    .with_input_size(ComponentSize::Sm)              // but keep inputs Sm
    .with_tab_size(ComponentSize::Sm)                // and tabs Sm
```

Card / form / list / menu / panel / preference / table padding, gap,
height, and radius are baked into each constructor (shadcn-shaped
defaults visible at the call site) and override per-call via
`.padding(...)` / `.pt(...)` / `.px(...)` / `.height(...)` / `.gap(...)`.
There is no `Density` enum, no `with_*_density` knob, no
`Theme::compact()` — the previous global selectors produced silent
drift across surfaces that an LLM author could not predict from
reading a single constructor.

### 2. Identity & a11y tags

- `key(s)` — stable identity for hit-test routing and event delivery.
- `at_loc(loc)` — source-mapped location, set automatically when your builder is `#[track_caller]`.
- `Kind::Custom("widget-name")` — the recommended kind for any user widget. Surfaces the name in tree dumps and lint output without claiming any built-in behaviour.

The decorative `Kind` variants (`Button`, `Card`, `Badge`, `Heading`, `Modal`, `Scrim`) are inspector tags only. The library does not dispatch behaviour on them. Use them or use `Custom` — the rendered output is the same.

### 3. Style profiles + surface roles

`StyleProfile` (`Solid`, `Tinted`, `Surface`, `TextOnly`) controls how the cross-cutting modifiers (`.primary`, `.success`, `.warning`, `.destructive`, `.info`, `.muted`, `.ghost`, `.outline`, `.secondary`) react to your widget. Set it once in your builder; the modifier vocabulary just works.

This is the rule that lets new widgets ship without editing `style.rs`. If your widget should react to `.primary()` like a button (solid fill), use `StyleProfile::Solid`. Like a badge (tinted alpha), use `Tinted`. Like a card (surface tint), use `Surface`. Pure text colour shifts only, use `TextOnly`.

`SurfaceRole` (`Panel`, `Raised`, `Sunken`, `Popover`, `Selected`, `Current`, `Input`, `Danger`) is the theme-facing semantic role for rect-shaped surfaces. Set it with `.surface_role(...)` when the widget's surface should be themed as a panel, input, popover, selected row, current nav item, and so on. The draw-op pass emits both the normal rounded-rect uniforms and a `surface_role` uniform; `Theme` can route different roles to different shaders via `with_role_shader`.

Use roles for meaning and profiles for modifier behavior. A text input, for example, uses `StyleProfile::Surface` so `.invalid()` can affect stroke/fill, and `SurfaceRole::Input` so a theme can render it as an inset/sunken material.

### 3.1 Text overflow policy

Single-line app chrome should choose an overflow policy explicitly. The default is `TextOverflow::Clip`; `.ellipsis()` switches a nowrap text element to truncation with a trailing ellipsis at draw-op construction time, so SVG fallback and GPU renderers see the same shortened string.

Use `.ellipsis()` for table cells, sidebar labels, command palette rows, email/name columns, badges with bounded slots, and any other fixed-width text where clipping would look like a rendering bug. The lint pass reports horizontally overflowing nowrap text as `FindingKind::TextOverflow` and suggests `.ellipsis()`, `wrap_text()`, or a wider box.

For bounded wrapped copy, use `.wrap_text().max_lines(n)`. The draw-op pass clamps the displayed text and ellipsizes the final visible line, so wrapped descriptions can stay inside cards, list rows, and helper panels without silently expanding the layout.

### 3.2 Typography roles

`TextRole` (`Body`, `Caption`, `Label`, `Title`, `Heading`, `Display`, `Code`) is the semantic typography role for text-bearing nodes. Set it with `.text_role(...)`, or use the role modifiers `.body()`, `.caption()`, `.label()`, `.title()`, `.heading()`, `.display()`, and `.code()`.

Roles apply default size/line-height/weight/color so product code can say what a text run is before overriding a specific detail. Damascene's typography tokens intentionally mirror Tailwind pairs such as `text-sm` = 14/20, `text-2xl` = 24/32, and `text-3xl` = 30/36; use `.line_height(...)` only for deliberate custom typography. For example, table headers and tiny metadata should usually be `.caption()`, button/menu labels should be `.label()`, card titles should be `.title()`, page titles should be `.heading()` or `.display()`, and inline code should use `.code()`. For shadcn-style secondary copy such as page subtitles, card descriptions, and explanatory helper text, prefer `.muted()` on body text; that preserves the normal 14px body rhythm while switching to `MUTED_FOREGROUND`. Tree dumps show non-body roles as `text_role=...`, which gives the agent loop a semantic handle when tuning density and hierarchy.

### 3.3 Icons

Use `icon("search")` for built-in vector icons, `icon_button("menu")` for the standard theme-sized icon-only button surface, and `button_with_icon("upload", "Publish")` for label+icon actions. The names intentionally mirror common lucide/shadcn names: `menu`, `search`, `bell`, `layout-dashboard`, `file-text`, `folder`, `users`, `bar-chart`, `git-branch`, `git-commit`, `refresh-cw`, `alert-circle`, `check`, `x`, `plus`, `chevron-right`, and related basics.

Icons are normal `El`s: set `.color(...)`, `.icon_size(...)`, `.icon_stroke_width(...)`, width/height, padding, or put them inside rows the same way as text. Prefer the icon-size tokens (`tokens::ICON_XS` = 14, `tokens::ICON_SM` = 16, `tokens::ICON_MD` = 20) over borrowing typography tokens for icon geometry. Tree dumps show `icon=<name>`, draw-op artifacts include `Icon` records, and the SVG fallback renders the vector path directly. The wgpu renderer, browser WebGPU path, and Vulkano renderer all render SVG-backed vector geometry through the shared vector mesh.

### 3.4 Form rows

`field_row("Volume (52%)", slider(...).key("volume"))` is the [label … control] row that fills 80% of a settings panel. The label is `.label()`-styled, a spacer pushes the control to the right edge, and the row vertical-centers and fills its parent's width so a column of `field_row`s lays out as a clean form. For multi-control rows (e.g. a value readout next to a slider), wrap them in `row([...])` and pass that as the control. Forks fine — the helper is a 4-line composition over `row`, `spacer`, and `text(...).label()`.

Pair `field_row` with `slider::apply_event(&mut value, &event, key, step, page_step)` for forms with several sliders: like every other widget's `apply_event`, one call dispatches both the pointer drag and the keyboard arrows, so the event handler stays one branch per slider rather than two `match` blocks dispatching by event source. `bin/settings_modal.rs` is the worked example — a tabbed modal at a custom 720×620 panel size, with a scrollable body between sticky tabs and a sticky footer.

### 3.5 Dialog, sheet, and modal anatomy

Use `dialog(key, [dialog_header([...]), body, dialog_footer([...])])` for the shadcn-shaped path: content, header, title, description, body, footer. Use `sheet(key, SheetSide::Right, [...])` for the same anatomy attached to an edge. Both are pure overlay compositions: scrim first, blocking panel second, no portal or retained overlay stack.

The older `modal(key, title, body)` helper stays as the compact convenience API and bakes a 420 px panel. For settings dialogs and other form-heavy modals, compose with `overlay` + `modal_panel` directly so the panel's size lives at the call site:

```rust
overlay([
    scrim("settings:dismiss"),
    modal_panel("Settings", [tabs_list(...), scroll([body]), footer])
        .width(Size::Fixed(720.0))
        .height(Size::Fixed(620.0))
        .block_pointer(),
])
```

`modal_panel` is `axis = Column, align = Stretch`, so a `scroll([body])` child claims the remaining height between any `Hug`-sized siblings (title, tabs, footer) — the footer stays visible while a long form scrolls inside the panel. The `.block_pointer()` chain is what stops clicks on the panel from passing through to the scrim and dismissing the modal.

### 4. Focus + interaction

- `.focusable()` — opt into Tab focus order and the focus ring. The library writes `focus_color` + `focus_width` uniforms onto your node's quad whenever the focus envelope is non-zero (animated by the runtime). The `RoundedRect` stock shader draws the ring in the `paint_overflow` band by default; `.focus_ring_inside()` draws it just inside the hitbox for dense flush rows such as menus. If you bind a custom shader, you receive the same uniforms and decide what to paint with them.
- `.paint_overflow(Sides)` — extend your painted area beyond your layout bounds. Layout-neutral (siblings don't shift, hit-testing still uses layout bounds). Use this to give the focus ring (or a drop shadow, or a glow halo, or a custom focus visual) somewhere to render outside the box.
- `.hit_overflow(Sides)` — extend your pointer hit target beyond your layout bounds without changing paint or layout. Hover, press, cursor, tooltip, and click routing all use the expanded target, while `UiEvent::target_rect()` still reports the visual rect from layout. Stock controls use the conservative `tokens::HIT_OVERFLOW`; use larger values sparingly for intentionally bigger targets around compact chrome, not for making unrelated gutters activate nearby UI.
- `.block_pointer()` — stop pointer events from passing through to siblings underneath. Used by modal panels and similar.

The library handles `Hover` / `Press` / `Focus` envelopes automatically once `focusable` is set: hover lightens, press darkens, focus rings fade in/out. None of these are kind-keyed — they apply to any focusable node.

#### Hover affordances beyond the built-in state

For "show on hover" patterns whose visibility shouldn't shift the surrounding layout — close × on a tab, secondary actions on a list row, hover-only validation icons — reach for `.hover_alpha(rest, peak)`. It binds the element's drawn alpha to the **subtree interaction envelope** (max of hover, focus, and press over the subtree rooted at this node), so a hover-revealed close icon stays visible while the tab is keyboard-focused or while the cursor moves to a focusable descendant. CSS `:hover`-style cascade.

For other hover-driven affordances (lift, scale-pop, tint shift), drive the prop from app code:

```rust
fn build(&self, cx: &BuildCx) -> El {
    let lifted = cx.is_hovering_within("card");
    card([...])
        .key("card")
        .focusable()
        .translate(0.0, if lifted { -2.0 } else { 0.0 })
        .scale(if lifted { 1.02 } else { 1.0 })
        .animate(Timing::SPRING_QUICK)
}
```

`BuildCx::is_hovering_within(key)` reads the same subtree predicate `hover_alpha` consumes. `.animate()` eases the prop between the two build values across frames, so transitions stay smooth without a per-channel declarative API. For transition-driven side effects (analytics, prefetch, sound), match `UiEventKind::PointerEnter` / `PointerLeave` on the corresponding key in `App::on_event`.

### 5. Custom shaders & custom layout

The two **escape hatches** documented in `docs/SHADER_VISION.md`:

- `.shader(ShaderBinding)` — bind your own shader for the surface paint, replacing `stock::rounded_rect`. The library injects `inner_rect` and `focus_color` / `focus_width` (when focusable + focused) uniforms into your binding — your shader can use them or ignore them.
- `.layout(F)` — supply your own positioning function for direct children. The library still recurses into each child and drives hit-test / focus / animation off the rects you return. The `LayoutCtx` handed to your function carries `container` (your inner rect), `children` (read-only), `measure` (intrinsic for any child), and `rect_of_key(&str) → Option<Rect>` (look up any keyed element's laid-out rect — used by anchored popovers and any cross-tree positioning).

### 6. Controlled widget state

App-facing widgets are **controlled**: the app owns their state and passes
that state into the widget builder on every `build()`.

```rust
use damascene_core::prelude::*;

struct Form {
    name: String,
    selection: Selection,
}

impl App for Form {
    fn build(&self, _cx: &BuildCx) -> El {
        text_input("name", &self.name, &self.selection)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.target_key() == Some("name") {
            text_input::apply_event(&mut self.name, &mut self.selection, "name", &event);
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }
}
```

That pattern is intentional. It keeps generated application code
obvious: state lives in the app struct, `build()` projects it into an
`El`, and `on_event()` folds routed events back into the state.

`text_area` uses the same controlled `(String, Selection)` shape, with
one extra responsibility when the area has a fixed height and can scroll
internally: queue a caret-visibility scroll request after accepted
editing or navigation events. The usual pattern is to set a bool when
`text_area::apply_event(...)` returns `true`, then drain one request on
the next frame:

```rust
use damascene_core::scroll::ScrollRequest;
use damascene_core::prelude::*;

struct Notes {
    body: String,
    selection: Selection,
    scroll_body_caret_into_view: bool,
}

impl App for Notes {
    fn build(&self, _cx: &BuildCx) -> El {
        text_area("body", &self.body, &self.selection).height(Size::Fixed(180.0))
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.target_key() == Some("body")
            && text_area::apply_event(&mut self.body, &mut self.selection, "body", &event)
        {
            self.scroll_body_caret_into_view = true;
        }
    }

    fn drain_scroll_requests(&mut self) -> Vec<ScrollRequest> {
        if std::mem::take(&mut self.scroll_body_caret_into_view) {
            text_area::caret_scroll_request_for(&self.body, &self.selection, "body")
                .into_iter()
                .collect()
        } else {
            Vec::new()
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }
}
```

Do not emit the caret scroll request every frame. The resolver no-ops
when the caret is already visible, but a per-frame request would fight a
user who manually wheels the fixed-height editor away from the caret.

The same shape extends to selection-style widgets. `tabs_list("k", &self.tab, [...])` paints a segmented row of triggers; `tabs::apply_event(&mut self.tab, &event, "k", parse)` folds clicks into the app's tab field. The page body is a plain `match self.tab` — there is no implicit "tab content" sibling; Rust's match is more honest than a wrapper that hides itself when not active. The naming and routed-key shape (`{key}:tab:{value}`) mirror shadcn / Radix Tabs and the WAI-ARIA tablist pattern so an LLM author finds familiar terrain. `select_trigger` + `select_menu` follow the same rule with `{key}:option:{value}`, and `radio_group` parallels `tabs_list` with a vertical layout and `{key}:radio:{value}`.

Two-state controls follow the same controlled pattern in their simplest form. `switch("auto_save", self.auto_save)` (track + thumb, like shadcn Switch) and `checkbox("agree", self.agree)` (square + check, like shadcn Checkbox) project a `bool` into a visual; `switch::apply_event(&mut self.auto_save, &event, "auto_save")` and `checkbox::apply_event` fold clicks back into the field. They share the same one-shape rule: app owns the `bool`, widget projects it, helper folds the event.

Read-only data displays skip the helper entirely. `progress(value)` (like shadcn Progress) draws a track + filled portion for a `0.0..=1.0` ratio; there is no `apply_event` because the widget doesn't accept input — the underlying value is whatever the app derived from a snapshot, timer, or computation.

There is also an advanced `UiState::widget_state::<T>` typed bucket used
by tests, diagnostics, and future host/widget experiments. Normal widget
builders do not receive `UiState`, so do not reach for it when writing
app-level widgets. If a stock widget needs hidden state that an app
widget cannot express with controlled state, the kit is missing a public
primitive and should grow one instead.

### 6.1 Optimistic overrides for externally-driven state

The controlled pattern in §6 assumes the *app* owns state. When the
truth lives in an external system (an audio server, a network peer, a
database) and the app sees it through periodic snapshots, naïvely
binding `build()` to the snapshot makes user input feel sluggish: the
slider snaps back to the snapshot value while the side effect is in
flight, then jumps to the new value the next time the snapshot ticks.

The pattern: **keep a `HashMap<Id, Override>` of pending values
alongside the snapshot**, render `override.unwrap_or(snapshot)`, fire
the side effect immediately on user input, and clear the entry on the
next snapshot whose value matches (within a small epsilon for floats).

```rust
fn percent_for(&self, node: &AudioNode) -> u32 {
    let snapshot_pct = node.volume.as_ref().map(Volume::percent);
    let override_pct = self.volume_overrides.borrow().get(&node.id).copied();
    match (override_pct, snapshot_pct) {
        // Snapshot caught up — drop the override.
        (Some(o), Some(s)) if o.abs_diff(s) <= 1 => {
            self.volume_overrides.borrow_mut().remove(&node.id);
            s
        }
        (Some(o), _) => o,            // override wins until reconciled
        (None, Some(s)) => s,         // pure snapshot
        (None, None) => 100,          // safe default before first snapshot
    }
}
```

`damascene-volume` uses this for volume, mute, and active-profile state.
The widget builder remains "controlled" — `build()` reads
`percent_for(node)` and projects that into the slider — but the value
behind it now reconciles two sources without flicker.

### 6.2 Tooltips

`.tooltip(text)` attaches a hover-driven tooltip to any element. The
runtime — not the app — owns the lifecycle: after the pointer rests
on the trigger for ~500ms, the library synthesizes a styled tooltip
layer at the El root, anchored below the trigger (flipping above on
viewport collision). Pointer leaves the trigger, or the user clicks,
the tooltip dismisses.

```rust
button("Save")
    .key("save")
    .tooltip("Save the current document (Ctrl+S)")
```

This is the only floating layer the library adds on the app's
behalf. Modals and popovers stay app-owned (rendered explicitly
from app state at the El root) — see `widgets/popover.rs` for the
"no portal hoist" rationale. Tooltips fit a different rule because
they are a pure read-out of hover state; the trigger doesn't need to
be keyed or focusable, and the synthesized layer is hit-test
transparent so it doesn't interfere with continued hover on the
trigger underneath.

### 7. Hotkeys & key delivery

Hotkeys are an app-level concern (`App::hotkeys()` returns `Vec<(KeyChord, String)>`); the library matches them in `key_down` ahead of focus activation. Widget builders that want a hotkey advertise the chord via the host's hotkey registry — there's no widget-private hotkey table.

Focused-node key capture: a widget that wants to consume Tab/Enter (and arrow keys / Backspace / Delete / Home / End / character keys) opts in with `.capture_keys()`. While that node is the focused target, the library's Tab traversal and Enter/Space activation defaults are bypassed and the raw `KeyDown` is delivered for the widget to interpret. Registered hotkeys still match first — an app's global Ctrl+S beats a text input's local consumption of S. Escape is delivered as a raw `KeyDown` first, then focus is cleared so it still acts as the generic "exit editing" command.

### 8. Host integration surface (not for widgets)

A handful of `UiState` methods exist for **host code** — backend `Runner` shells, the `damascene-web` wasm entry, port crates that integrate Damascene into a larger app — not for widget builders. Calling them from inside a widget would be a symmetry violation, since user widgets have no access to the runner-side state these talk to. They live in the public API because the host crates that use them are *also* downstream of `damascene-core`, but they aren't part of the widget kit.

- `UiState::rect_of_key(root, key) -> Option<Rect>` and `UiState::target_of_key(root, key) -> Option<UiTarget>` — let a host look up the laid-out rect (or full event-routing target) for a keyed element. Used to anchor native overlays over a reserved viewport region, or to forward a host-side event into an externally-painted region. Widget code looking up another node's rect should use `LayoutCtx::rect_of_key` (§5) instead — that's the kit primitive.
- `UiState::set_animation_mode(mode)` — switch between real-time and frozen animation evaluation. Used by headless render fixtures and tests to get deterministic output.
- `UiState::has_animations_in_flight() -> bool` — host's frame-pacing decision: keep ticking the loop or sleep until input. Each backend `Runner::prepare()` already returns a `needs_redraw` derived from this; calling it directly is for hosts that want the signal independent of `prepare()`.
- `UiState::debug_summary() -> String` — terse per-frame state dump for `console.log`-style instrumentation in browser builds.

These all interact with library-owned bookkeeping (focus tracker, animations, computed-rect map). They aren't backdoors past the kit — they're a different audience's surface. If a widget ever wants one of these, that's a sign the kit is missing a primitive, and the right move is to add it under §1–§7, not to reach for the host method.

## Common smells

The library has a small, named vocabulary precisely so a widget — or an app `build()` — doesn't need to invent one. The patterns below mean an existing affordance is being missed:

- **`.font_size(...).font_weight(...).text_color(...)` on a single text node.** That's what role modifiers exist for. `.heading()`, `.title()`, `.label()`, `.caption()`, `.code()` set size + weight + theme-aware color in one call. Reaching for the underlying primitives is how typography drifts (one hand-written 16px semibold title looks subtly different from another).
- **`column([...]).fill(CARD).stroke(BORDER).radius(...)` for grouped content.** That's `card([card_header([card_title("Title")]), card_content([...])])`. Cards route through `SurfaceRole::Panel` so the theme can swap the material later (shader, shadow, inset) without touching the call site.
- **`column([text(label).label(), text_input(...)]).gap(...)` for vertical fields.** That's `form_item([form_label(label), form_control(text_input(...)), form_description(...)])` inside `form([...])`. `form_item` bakes `gap(SPACE_2)` (≈ shadcn `space-y-2`) and `form` bakes `gap(SPACE_3)`; override per-call when a layout calls for tighter or looser stacks.
- **`row([...]).metrics_role(TableRow).align(Center)` for table rows.** That's `table_row([...])` inside `table([table_header([...]), table_body([...])])`. `table_header` promotes direct `table_row` children to header metrics, and table rows center their cells by default.
- **Status as a unicode bullet or emoji** (`text("● Online")`, `text("⚠ Failed")`). That's `badge("Online").success()` / `badge("Failed").destructive()`. Badges read as proper status pills and pick the theme color through the StyleProfile.
- **Callouts as custom cards.** That's `alert([alert_title("Heads up"), alert_description("Details")]).warning()`: the alert bundles the surface profile, the destructive/warning/info color route, and the shadcn-shaped padding recipe so callouts stay visually consistent.
- **Identity chips as ad hoc circles.** That's `avatar_fallback("Alicia Koch")`, `avatar_initials("AK")`, or `avatar_image(img)`. The stock avatar keeps tables, nav, and activity feeds on the same circle size/radius.
- **Loading placeholders as raw muted boxes.** That's `skeleton()` plus normal `.width(...)` / `.height(...)`, or `skeleton_circle(32.0)` for avatar placeholders.
- **Command palette rows as repeated `row([...])` snippets.** That's `command_row("git-branch", "New branch", "Ctrl+B")`, or `command_item([command_icon(...), command_label(...), command_shortcut(...)])` when the row needs custom children.
- **Collapsible sections as button-plus-chevron snippets.** That's `accordion_item(...)`, `accordion_trigger(...)`, and `accordion::apply_event(...)`; the trigger bakes a 40px-tall list-row recipe with focus, pointer cursor, and the standard chevron treatment.
- **Sidebar navigation as custom columns.** That's `sidebar(...)`, `sidebar_group(...)`, `sidebar_menu(...)`, and `sidebar_menu_button_with_icon(...)`; the buttons share the same 40px list-row recipe, and selected items use the shared `.current()` treatment.
- **Toolbars as hand-aligned rows.** That's `toolbar(...)` and `toolbar_group(...)`; action rows should center their controls and use the same gap cadence as table/page chrome.
- **Dropdown menus as a popover full of custom rows.** That's `dropdown_menu(...)`, `dropdown_menu_content(...)`, `dropdown_menu_label(...)`, `dropdown_menu_separator()`, and `dropdown_menu_item_with_icon_and_shortcut(...)`; the stock rows bake the shadcn-shaped menu-item recipe (height + side padding + gap + radius) and arrow navigation.
- **Dialog and sheet surfaces as custom overlay cards.** That's `dialog(key, [dialog_header([...]), ..., dialog_footer([...])])` or `sheet(key, SheetSide::Right, [...])`; both keep the scrim/panel/block-pointer shape consistent with modal and popover behavior.
- **Breadcrumbs as slash-delimited text.** That's `breadcrumb_list([breadcrumb_link(...), breadcrumb_separator(), breadcrumb_page(...)])`; the links, current page, separators, and centered row rhythm are separate named pieces.
- **Pagination as custom button rows.** That's `pagination_content([pagination_previous(), pagination_link("1", true), pagination_next()])`; page links get a stable square control box and previous/next use the built-in chevron icons.
- **`.gap(0.0)`.** The default *is* `0.0`. Setting it explicitly is noise that signals the author misremembered the default — and usually means actual gap is missing somewhere else where it should be added.
- **Wrapping a single child in `row([single])` to apply padding.** `.padding(Sides::all(...))` is on every `El`. The wrapper is dead weight.
- **Tree indent built from `row([spacer().width(Fixed(indent)), ...])`.** Use `.padding(Sides { left: indent, ..Sides::zero() })` on the row — left-only padding does the job without an extra child. `Sides::xy(h, v)` is also there for symmetric horizontal/vertical padding.
- **Explicit `.fill(tokens::BACKGROUND)` on the root.** The host paints `BACKGROUND` behind the tree before draw-ops run; the root fill is redundant.
- **A built-in `IconName::*` standing in for an app-specific SVG.** Apps ship `SvgIcon::parse_current_color(include_str!("..."))` once (typically as a `LazyLock`) and pass the result to `icon(...)` — same pipeline, same `text_color` tinting as the built-ins.

These aren't style nits — they're load-bearing in keeping LLM-authored UI from drifting into raw-rectangle territory. If you find yourself writing one of them, that's a kit-discoverability problem worth flagging in this doc rather than coding around.

## What you don't get

These would be symmetry violations and aren't part of the kit:

- **No stock-only fields on `El`.** Every public field/builder method is yours too.
- **No library-side `match` on `Kind::*`.** The decorative variants are inspector tags. The structural ones (`Group`, `Spacer`, `Divider`, `Scroll`, `VirtualList`, `Inlines`, `HardBreak`, `Custom`, `Text`) earn their place — they affect layout/event semantics — and apply to your widget the same way they apply to stock.
- **No reaching past the runner.** The `Runner` in each backend crate consumes `DrawOp` and `UiState`; widgets produce `El` trees. There's no widget API that pokes the runner directly.

## The dogfood test

A widget passes the kit if it can be written using *only* the items above. The compiler can't enforce this — the API is open. The convention is enforced by `widgets/button.rs`, `widgets/badge.rs`, `widgets/card.rs`: each is a tight composition of public surface, no internals.

If you find yourself wanting a feature that requires reaching past this kit, that's a signal to **add the feature to the kit** rather than carving an exception. Open an issue or rev `widget_kit.md`. The point of the symmetry invariant is that the library is a substrate, not a library of fixed components.
