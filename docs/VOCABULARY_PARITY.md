# Damascene Vocabulary Parity — Core Proposals

Maintainer-facing. This records the candidate core changes surfaced by
building out-of-tree opinion crates against the stock library, the
evidence for each, and — equally important — the ones that were rejected
and why, so they don't get re-proposed on weaker grounds later.

**Status 2026-07-26:** proposals 1–3 are landed on `workbench-vision`
(merges `13e4c1a` token namespace, `a31ad0a` radius scale, `12f6e6e`
per-side borders; review fixes `bc932d2`, `c74951f`). Building the
first workbench-crate slice against them exposed two further theme
knobs an agent expects (§4 shadow scale, §5 type scale) and one
follow-up to §3 (the radius-origin flag split). Each is adjudicated by
the same acceptance test.

## Premise

Damascene's thesis is vocabulary parity with the LLM training
distribution: an agent should get good results from what it already knows
about web UI development, without becoming a Damascene expert. "The
minimum output should be the correct output."

That thesis is also the acceptance test for any addition:

> Would a web-trained agent reach for this and be surprised it is
> missing?

Not "does some design system need it." A feature that Carbon or MUI have
and shadcn/Tailwind do not is a *weaker* case under this principle, not a
stronger one. Two of the four candidates below fail that test and are
recorded as rejected.

## How these were found

`crates/damascene-carbon` is an out-of-tree opinion crate mimicking IBM
Carbon plus the VS Code workbench — dense, square-cornered, data-heavy.
It is the other side of the landing-page / application-shell split from
the stock shadcn vocabulary.

It required **zero core changes** to build. Every extension point it
needed was already public. So none of what follows is a blocker; these
are parity gaps the crate made visible.

The crate's origin is worth stating plainly because it is a bias risk: an
opinion crate's wishlist is not automatically a parity gap. Each item
below is therefore justified — or rejected — on in-tree evidence
independent of the crate, and where an item survives *only* on the
opinion crate's need, that is said out loud.

---

## 1. Per-side borders (recommended, land first)

### The gap

There is no way to express `border-b`. `El::stroke` sets all four sides.
An author who wants a rule under a toolbar, a table row, a tab strip, or
a panel header must insert a 1px-tall filled `El` as a sibling.

`border-b` is a top-tier Tailwind utility and appears throughout shadcn's
own components. This is not a Carbon requirement; Carbon merely made it
impossible to ignore.

### Evidence — three in-tree confessions

**The stock table already fakes it.** `widgets/table.rs` interleaves 1px
filled `El`s between rows: `table_body` pushes `row_rule()` between each
pair of rows, and `table_header` appends one after the header. `row_rule()`
is a `Kind::Group` with `fill(tokens::BORDER)` and `height(Size::Fixed(1.0))`.
The comments say what is being emulated in so many words — "shadcn's
header row carries the same border-b as body rows", and on `table_head`,
"the border-b rule below the row is the header chrome".

The workaround leaks into structure. An N-row body has 2N−1 children; the
tests at `table.rs:225-228` assert `children[1]` is a rule and
`children[2]` is a row, and `table.rs:58-60` needs a `!row.explicit_radius`
special case to cope with promotion. Nothing in-tree indexes table
children outside those tests, so this is not a live bug — but it is a
structure an author can trip over, and it is the library writing around
its own missing primitive.

**The HTML importer cannot represent it.** `damascene-html/src/css.rs`
handles per-side `padding-*` (`:504-507`) and per-side `margin-*`
(`:513-516`), but its border arms (`:517-536`) are only `border`,
`border-width`, `border-color`, and `border-radius`. `border-bottom`
falls through the catch-all at `:681-686` and is dropped silently. Feed
the importer a shadcn table and every rule vanishes. For a subsystem
whose entire purpose is ingesting web-shaped input, that is a direct hit
on the thesis.

**Opinion crates re-derive it immediately.** `damascene-carbon` has a
`hairline()` helper with 12 call sites — independently reinventing
`row_rule()`, because the primitive is missing at the same place twice.

### Semantics — the corpus answers this, and we already implement it

The initial sketch had per-side borders as paint-only, on the grounds
that it matches `El::stroke` and needs no layout work. That reasoning was
backwards: it optimizes for implementation cost and for consistency with
a primitive that is *itself* the divergence.

Tailwind's preflight sets `box-sizing: border-box`, so on the web:

- explicit height → the border eats inward; outer size unchanged
- auto height → the border adds outward; outer size grows

Either way the border occupies the box model, between padding and margin,
and content never paints under it.

**Damascene already implements exactly this model for padding.**
`layout.rs` insets the content rect at every container site
(`node_rect.inset(node.padding)` at `:789`, `:813`, `:862`, `:2022`,
`:2123`, and `:2967` in `layout_axis` — the main flex pass), and
`Size::Hug` folds padding into the intrinsic — with a test at
`layout.rs:4538-4551` naming the CSS rule it matches.

`El` has no margin concept at all, so damascene's version of the box
model is simpler than the web's: border joins padding in the inset,
full stop, with nothing on the far side to order against.

So the correct semantics are not a new layout feature. They are:

> **Borders join padding in the inset.**

Both cases then fall out correctly with no separate decision, and the
same move settles placement: a border occupying the outermost pixels of
the border box is *inside* by construction. CSS borders are inside;
`El::stroke` is centered on the boundary
(`shaders/rounded_rect.wgsl:235-238`, `abs(d) - stroke_width * 0.5`,
which is SVG semantics). New API should not replicate that.

There is a practical confirmation that this is the right call. Today's
interleaved rules each consume 1px of column layout. If borders consume
layout, converting the stock table grows each row by 1px and deletes the
rules — **net zero**, pixel-identical, no goldens move. The paint-only
version would have shrunk every table by N pixels and churned every
snapshot.

### Implementation

Painting is cheap. The quad instance ABI is full — `paint/mod.rs:63-100`
has `slot_a` fill, `slot_b` stroke, `slot_c` (stroke_width, max_radius,
shadow, focus_width), `slot_d` focus color, `slot_e` per-corner radii,
and `paint/slots.rs:44-46` reserves locations 0/1/5 — but borders do not
need a slot. `push_node` already emits multiple quads per node with
synthetic ids (`draw_ops.rs:462`, `:554`, `:1040` `{id}.scrollbar-thumb`,
`:1136` `{id}.math-rule.{i}`). Border edges are `{id}.border-b` and
friends, emitted immediately after the main surface quad.

Consequently there is **no shader change, no ABI change, and no work in
`damascene-wgpu`, `damascene-vulkano`, or `damascene-web`** — all three
consume the DrawOp stream (`damascene-web` is a wgpu host over a canvas,
not a DOM renderer). `hit_test.rs` has no stroke references and
`bundle/svg.rs` renders per-DrawOp from uniforms, so edge quads flow
through as plain fill rects exactly as today's rules do.

The layout work is the real cost, and it is contained — but do not
patch the 6 container sites individually: introduce a single
`content_inset()` helper (padding ⊕ border) used by all of them and by
the intrinsic calculation, so a future 7th site can't miss the border.

`damascene-html` needs genuinely new code — four sides × the
`border-bottom` / `-width` / `-color` / `-style` forms, kept coherent
with the `ComputedStyle` merge at `css.rs:190-197`.

### Cost that is real, not ambiguous

Per-side borders meeting a nonzero corner radius. CSS specifies this
fully — the border follows the curve with distinct inner and outer radii,
and adjacent sides mitre on the diagonal. It is not ambiguous, it is
expensive: the stock shader draws one uniform stroke band.

Proposed approximation: **the edge quad spans the rect minus the adjacent
corner radii.** Exact at `radius: 0` — which is every case Carbon and the
VS Code workbench care about, and most shadcn cases too, since `border-b`
overwhelmingly lands on square rows, toolbars, and header strips. It
should be documented as an approximation, not presented as the CSS
behavior.

### API shape

The established precedent is the terse Tailwind mapping — `.pt` / `.pb` /
`.pl` / `.pr` / `.px` / `.py` (`tree/layout_modifiers.rs:134-177`) — so
`.border_b()` beats `.border_bottom()`.

Two shape questions worth settling before code:

**Argument, or none?** `.stroke(c)` takes a *color* and defaults width to
1 (`visual_modifiers.rs:58-64`); `.border_b(w)` taking a *width* is
inconsistent. The naive line an agent writes is "1px bottom border in the
border token", so the minimum-correct signature is arguably a zero-arg
`.border_b()` defaulting to 1px `tokens::BORDER`, with width and color as
separate overrides. `border_color: None` should fall back to
`tokens::BORDER` — that is what `* { border-color: var(--border) }` does
in shadcn's preflight.

**`stroke` or `border`?** The crate says *stroke* (SVG vocabulary); the
web says *border*. Shipping a `border_*` family alongside a `stroke`
family institutionalizes that split, which is itself a parity hazard.
`.border(c)` as an alias for `.stroke(c)` would resolve the naming; it
does not resolve the centered-vs-inside semantic difference, and closing
that would move every stroked pixel in the library. **This is the one
question in this document the web corpus cannot answer** — it is a
deprecation-policy call.

### Checklist

- `El` boxes rare payloads by established convention — a dozen
  `Option<Box<...>>` fields in `node.rs`, eight of them carrying an
  explicit "Boxed to keep `El` small" rationale. Use
  `Option<Box<BorderSpec>>`, not ~56 inline bytes on every node.
- The `RawColor` lint checks only `fill` and `stroke`
  (`bundle/lint.rs:811-841`). It needs a `border_color` arm or raw-rgba
  borders skate past token discipline.
- Surface roles rewrite stroke uniforms (`draw_ops.rs:524-539`); border
  quads bypass theme surface recipes. Acceptable, but document it.
- Emit edge quads with the surface paint. Children painted after will
  cover a border they overlap — same z-order as CSS in-flow children,
  so this is correct, but know it.

---

## 2. Open token namespace on `Palette` (recommended, small)

### The gap

`Palette::lookup` (`theme/palette.rs:630-665`) is a closed `match` over
31 shadcn token names returning `None` on a miss, and `resolve`
(`:613-625`) passes misses through with their baked rgba. So
`Color::srgb_token("layer-01", 38, 38, 38, 255)` compiles, paints its
literal fallback, and silently ignores every palette swap forever.

On the web, minting a design token is open-namespace by construction:
`var(--layer-01)` works because you declared it. `Color::srgb_token` is
public API that *looks* open and is secretly closed.

### What this is not

The passthrough is **deliberate and documented** (`palette.rs:603-609`)
for theme-invariant tokens, and `damascene-carbon` relies on it for its
raw gray scale. An open namespace therefore lets a design system opt in;
it does **not** add a diagnostic. A typo'd token still passes through
silently, indistinguishable from an intentional invariant one. Any
proposal that sells this as fixing a silent bug is overselling it.

This item is also honestly Carbon-motivated: no shadcn-vocabulary user
needs it, because every shadcn name is already a member. It survives on
being tiny, additive, and matching how CSS custom properties actually
work — not on in-tree evidence.

**Update 2026-07-25:** no longer only Carbon-motivated. The ratified
workbench opinion crate (`WORKBENCH_VISION.md`) mints a VS Code-derived
key set (`side_bar`, `status_bar`, `panel`, …), and rumble already mints
semantic tokens (`rumble-talking`, …) through the passthrough today.
Two non-Carbon consumers; the evidence is now genuine.

### Design

`Palette { extra: BTreeMap<&'static str, Color> }`, consulted by `lookup`
after the match miss, plus `Palette::with_token(name, color)`. Existing
names hit their arms first, so nothing existing can change behavior.
`BTreeMap::new()` is const-stable, so the `const fn` palette constructors
survive.

Side benefit: it makes the `RawColor` lint meaningful for third-party
design systems, which today cannot be distinguished from an app
hardcoding hex.

---

## 3. Theme radius scale (redesigned; defer)

### The withdrawn design

The first sketch was `RadiusScale { sm, md, lg }` on `ThemeMetrics` plus
`Theme::with_radius_base(f32)` deriving shadcn's ladder, with defaults
equal to today's tokens. **That is arithmetically impossible.**

shadcn's ladder is subtractive with 2px spacing —
`references/shadcn-calibration/tailwind.config.js:40-44` has
`lg = var(--radius)`, `md = calc(r - 2px)`, `sm = calc(r - 4px)`.
Damascene's tokens are 4 / 8 / 12 (`theme/tokens.rs:175-180`), spacing 4.
No base value reproduces today's defaults from that ladder. "Derive
shadcn's ladder" and "defaults unchanged" are mutually exclusive.

It also misses its own target. Buttons, inputs, and tab triggers do not
read `RADIUS_*` at all — `control_metrics` (`metrics.rs:392-398`) has a
separate 5 / 6 / 7 / 8 ladder keyed by `ComponentSize`. A
`RadiusScale{sm,md,lg}` knob leaves every control rounded, failing the
exact "make this app square" scenario that motivates the feature.

And `default_radius` is `pub(crate)` (`visual_modifiers.rs:269`), so
out-of-tree opinion crates cannot mark radius as theme-default at all.

### The redesign

A single **multiplicative** knob dissolves all three problems:

```rust
Theme::with_radius_scale(f32)
```

Applied in the metrics pass to every `!explicit_radius` corner —
including `apply_control`'s output at `metrics.rs:434` — rescaling only
nonzero corners so `Corners::top(...)` shapes survive. `0.0` squares the
whole app including controls; an explicit `.radius()` survives, matching
the flag's existing contract.

The machinery is already there: `metrics.rs:434` rewrites `el.radius`
when `!explicit_radius`, and `default_radius` (40 catalog call sites, vs
20 explicit `.radius()`) already marks "theme default, not the author's
choice." The flag is maintained catalog-wide with only four readers
(`metrics.rs:434`, `metrics.rs:603`, `table.rs:58`, `tabs.rs:385`).

One edge case to decide: `RADIUS_PILL` (999) set via `default_radius`
would square pills and badges at scale 0, whereas on the web
`rounded-full` is independent of `--radius`. Either exempt values at or
above the pill threshold, or accept the divergence knowingly.

### Why defer

Even redesigned, this is the weakest survivor on the acceptance test.
"Make this app square" is a human theming wish, not something a
web-trained agent trips over mid-task. shadcn does have the one-line
knob, so it is real parity — but it should land last, or wait for a
second consumer.

**Update 2026-07-25:** the second consumer arrived. The workbench crate
(`WORKBENCH_VISION.md`) wants the whole shadcn radius ladder scaled to
~0–5px, which is exactly what the multiplicative knob expresses. Still
lands after (1) and (2), but no longer deferred indefinitely.

### Follow-up (2026-07-26): the radius-origin flag split

Landed with one known hole, found in adversarial review: **tab triggers
do not square at scale 0.** `set_trigger_radius`
(`widgets/tabs.rs:385-389`) claims `explicit_radius = true` as its
contract to protect its per-corner segment shapes from `apply_control`
— which also shields them from `apply_radius_scale`. Result: rounded
segments inside a squared strip. The card-corner propagation fix
(`c74951f`, `metrics.rs:681/:690`) claims the same flag for the same
reason, so there are now two library claimants of an author-intent bit.

Acceptance test: shadcn's tab-trigger radius derives from `--radius`
via the calc ladder, so a web-trained agent setting radius to zero
expects tabs to square with everything else. The current behavior is a
damascene-internal artifact, not a corpus behavior.

Design — replace the boolean with a three-state origin (**landed
2026-07-26**, as implemented; one variant differs from the first
sketch, see below):

```rust
enum RadiusOrigin { ThemeDefault, LibraryShape, Fixed }
```

The axis is the *contract*, not the author — what the pass may do to
the value, not who wrote it:

- `ThemeDefault` — `default_radius()` / untouched. `apply_control` may
  restamp it; the radius scale applies.
- `LibraryShape` — a construction-time widget silhouette
  (`set_trigger_radius`). Not restamped, but **scaled**: shapes survive
  proportionally at nonzero scales and square at 0.
- `Fixed` — final value: author `.radius()` calls **and** the in-pass
  card-corner stamping. Neither restamped nor scaled.
- `table.rs` header promotion squares `ThemeDefault` only, as before.

The first sketch named the third state `Author` and had the card
stamping use `LibraryShape`. Implementation refuted that: the metrics
walk is pre-order, so the card's propagation (which runs post-scale at
the parent and stamps the card's *final* corners) executes **before**
the strips' own scale visits — a scalable origin there would
double-scale an already-final value, and stamping pre-scale values
instead resurrects the poke-through for scale-exempt cards. The stamp
needs scale-immunity, which is exactly the author-call contract; so the
state is named for the contract (`Fixed`) and both users share it
honestly. Regression tests: `tab_triggers_square_with_radius_scale_zero`,
`tab_trigger_silhouette_scales_proportionally` (`metrics.rs`).

---

## 4. Theme shadow scale (landed 2026-07-26)

### The gap

`Theme` has no elevation knob. Stock `card()` bakes
`.shadow(tokens::SHADOW_SM)` (`widgets/card.rs:85`), and the paint path
fills role defaults — `Panel` → `SHADOW_SM`, `Raised` → `SHADOW_XS`
(`theme/mod.rs`, deliberately `default_f32` so a widget's declared
elevation survives; the rationale comment is correct and stays). A
theme that wants a flat, layered look — the entire workbench premise,
and "no shadows for chrome" is binding in `WORKBENCH_VISION.md` — has
no way to say so. The first workbench slice shipped with every stock
card still floating and had to silently narrow its own claims.

### Acceptance test

Current shadcn theming ships shadow variables in its theme format, and
"flat theme ⇒ no shadows" is a theme-level property on the web — an
agent theming toward a tool look expects one place to kill chrome
shadows, not a per-widget hunt for `.shadow(0.0)` overrides. Tailwind's
shadow scale is likewise themeable. This is the same species as the
radius knob, found the same way: a real theme couldn't express itself.

### Design

```rust
Theme::with_shadow_scale(f32)   // default 1.0; 0.0 = flat chrome
```

Mirror the radius knob's contract exactly, including the explicit-flag
pattern (`explicit_radius`, `explicit_font_family`, `explicit_mono` are
the precedent — shadow today has no flag at all, which is the first
thing to fix):

- `El` gains the `explicit_shadow` flag; `.shadow()` sets it; widget
  recipes (card, dialog, popover) move to a `pub(crate)`
  `default_shadow()` that doesn't.
- The scale applies to non-explicit `El` shadows and to the surface-role
  default uniforms (both are theme-owned by definition). An author's
  explicit `.shadow()` survives at any scale — on the web, a hardcoded
  `box-shadow` doesn't respond to theme shadow variables either.
- Default 1.0 must be bit-identical to today, same regression-test
  requirement as the radius knob.

The workbench crate then sets `with_shadow_scale(0.0)` for chrome and
keeps popovers/menus shadowed the way VS Code does — via explicit
shadow in its popover recipe, or a nonzero small scale; that choice is
the crate's, which is the point.

**As landed:** one refinement found in implementation — at scale `0.0`
a scaled-away *role default* is **omitted** from the uniforms rather
than written as `0.0`. `with_role_uniform` values apply by `or_insert`
after the role material, so writing an explicit zero would block them;
omission keeps a deliberate re-elevation path open. The workbench
crate uses exactly this: flat app + `Popover`-role `SHADOW_MD`,
VS Code's single `widget.shadow` tier. Regression tests:
`shadow_scale_zero_flattens_stock_recipe_shadows`,
`explicit_shadow_survives_shadow_scale_zero` (`metrics.rs`), and
`shadow_scale_reaches_role_defaults_and_keeps_the_restore_path`
(`draw_ops.rs`, including the end-to-end "role default must not
resurrect a flattened card shadow" case).

---

## 5. Theme type scale (landed 2026-07-26)

### The gap

`TextRole` sizes are hardwired: Body and Label stamp
`tokens::TEXT_SM` (14px), Caption `TEXT_XS`, Title `TEXT_BASE` — fixed
`TypeToken` constants (`theme/style.rs`, `theme/tokens.rs:257-296`).
A theme cannot say "this application's UI type runs at 13px." The
workbench crate claims dense type and cannot deliver it for any stock
control; its own chrome fakes it with `.caption()` calls.

### Acceptance test

The corpus mechanism is **rem**: on the web, "make the UI 13px" is one
declaration — root font-size — because the entire Tailwind type ladder
is rem-based and scales together, line heights included. An agent
expects the equivalent single knob to exist. Damascene has no rem
concept; a theme-level scale over the role ladder is the analogue.

### Design

```rust
Theme::with_type_scale(f32)   // default 1.0; workbench wants ~13/14
```

- Applies to every role-derived `TypeToken` size **and line height**
  (rem scales both; scaling size alone wrecks vertical rhythm) at the
  point where roles stamp type — the `apply_type_token` path.
- Author-set explicit sizes survive, via the same explicit-flag pattern
  (an `explicit_font_size` sibling of `explicit_mono`). The flag is set
  by the raw `El::font_size(f32)` setter (`tree/content.rs:81`) — the
  `text-[15px]` analogue — and **not** by `.small()` / `.xsmall()`,
  which stamp ladder rungs (`TEXT_SM` / `TEXT_XS` via
  `apply_type_token`): on the web `text-sm` is rem-derived and scales
  with the root. Ladder-derived sizes scale, hand-picked px doesn't.
  Since `.small()` and the role pass both stamp raw `f32` sizes at
  distinct times, the practical implementation is "scale any
  non-explicit `font_size` in the theme pass" — token identity is
  already lost by then, and the flag is exactly the needed bit.
- Naming choice, the one genuinely open question:
  `with_type_scale(f32)` (multiplicative, sibling of the radius and
  shadow knobs) vs `with_base_font_size(f32)` (names the rem idiom;
  factor = base/16 since `TEXT_BASE` = 16px = 1rem). The rem name is
  more corpus-faithful; the multiplicative name matches the sibling
  knobs and avoids implying an exact rem contract damascene doesn't
  have. Lean: `with_type_scale`, rustdoc'd with the rem analogy.
- Metrics interaction: control heights (`control_metrics`) do NOT
  scale with type — they are the `ComponentSize` ladder's job. Document
  the boundary so nobody wires the two together later.
- Default 1.0 bit-identical, regression-tested, as with the siblings.

**As landed:** `with_type_scale` chose the multiplicative name; the
flag is `explicit_font_size`, claimed by `.font_size()`,
`.line_height()`, and `.icon_size()` (icons ride the same `font_size`
field, and rem-sized icons scale on the web too — so themed icon sizes
scale, author `icon_size()` survives). One recipe cleanup: `code_block`
called the raw `.font_size(TEXT_SM.size)` setter redundantly (it *is*
the El default), which would have pinned code blocks against the scale
— the call is deleted, not converted. The scale clamps to a 0.05 floor
rather than 0: a zero type scale erases all text, which is never the
intent the way a zero radius or shadow is. Workbench sets `13/14`.
Regression tests: `type_scale_scales_roles_rungs_and_line_heights`,
`explicit_type_metrics_survive_type_scale`,
`type_scale_default_is_identity` (`metrics.rs`).

---

## Rejected: container density / size props

> **Partially reversed 2026-07-28 — read this box before the section
> below.** Table *cell padding* now rides `ComponentSize`
> (`metrics::table_cell_metrics`), so
> `Theme::with_default_component_size` densifies tables along with
> every control. What stays rejected is unchanged and is the larger
> half: **no per-role container size prop** — there is no
> `with_table_size`, no `with_list_size`, no `with_menu_size` — and no
> global density knob. The reversal is one metric on the *existing*
> theme-wide rung, not a new knob.
>
> What reopened it: the ratified workbench emulation target
> (`WORKBENCH_VISION.md`) shipped, and its reference corpus put a number
> on the gap this section waved at. `references/workbench-validation/
> parts/index.html` declares `tbody tr { height: 28px }` and renders at a
> 30px pitch; a stock damascene table under `damascene_workbench::theme`
> measured **35.57px**. Both damascene-side validation apps
> (`examples/parts.rs`, `examples/parts_v2.rs`) independently converged
> on the same workaround — a local `ROW_PAD_Y = 5.0` re-padding every
> cell and every head — which is this section's own "per-cell
> `.padding()` works but is verbose" residue showing up as measured
> duplication rather than as a hypothetical.
>
> The design deliberately does **not** relocate the knob per role:
> - The metric is derived, not picked. A row's content box is the rung's
>   *control height* (`Md` → 36px, which is exactly the
>   `Sides::xy(SPACE_3, SPACE_2)` the constructors hardcoded, so shadcn's
>   own rung is bit-identical to the pre-ladder library). A table is
>   therefore as dense as the buttons and inputs beside it, which is the
>   thing an agent means by "densify the app".
> - The implementation is the child-rewriting path this section
>   predicted (`apply_table_cell_padding`, precedent
>   `apply_tab_trigger_size_to_children`), not new cell roles.
> - `.padding()` / `.py()` on a cell still wins, so the escape hatch the
>   section defended is intact — `examples/parts_match.rs`, a pixel-match
>   reproduction, is untouched by the change.
>
> Still live from the section below: shadcn genuinely has no `size` prop
> on Table, so `.size()` on a table row is damascene vocabulary, not
> parity — it is documented as such and is not the primary path. The
> "lint `.size()` on a role that ignores it" residue also survives, minus
> the two table row roles, which now read it.

The original form was a global density knob. `metrics.rs:5-8` documents
the decision not to have one, and that decision is **correct** under the
acceptance test: shadcn has no density knob, so an agent does not expect
one.

The reshaped form — extend `ComponentSize` to the container roles that
are explicit no-ops at `metrics.rs:284-300`, giving
`Theme::with_table_size(...)` — does not escape the problem. It relocates
the density knob from theme-global to per-role. shadcn's Table,
DropdownMenu, and List have **no** size prop; `size` on data tables is
Carbon vocabulary. A web-trained agent densifying a table writes Tailwind
utility overrides: `py-1` on cells, or `[&_td]:py-1` on the table.

This is the clearest instance in this document of an opinion crate's want
dressed as a parity gap, and it is rejected on that basis.

Two counter-arguments, weighed honestly:

- **`.size()` already compiles and silently no-ops on containers.**
  `layout_modifiers.rs:98` sets `component_size`; container roles never
  read it. Its rustdoc does say "for stock controls", so the no-op is not
  strictly undocumented — but nothing prevents or flags the call, and a
  compiling dead prop is a minimum-output hazard. **This is the residue
  worth acting on:** lint `.size()` on a role that ignores it.
- **The underlying gap is descendant styling.** Damascene has nothing
  corresponding to `[&_td]:py-1`. Per-cell `.padding()` works and
  survives everything, but is verbose. If dense tables become a recurring
  need, that primitive deserves designing on its own terms; per-role size
  props would be a Carbon-shaped patch over it.

Implementation would also be less pattern-parallel than it looks: the
padding lives on `table_head` / `table_cell`, which carry no metrics role
(`table.rs:126-159`) — the row roles are hollow. The pass would need
child-rewriting (precedent: `apply_tab_trigger_size_to_children`,
`metrics.rs:312-313`) or new cell roles.

---

## Adjacent findings

Small, independent, and worth fixing regardless of whether anything above
lands.

- **`El::stroke` is centered on the boundary, CSS borders are inside.**
  `shaders/rounded_rect.wgsl:235-238`. A pre-existing divergence from the
  web corpus, recorded here because closing it would move every stroked
  pixel in the library and so needs its own decision.
- **The CSS importer silently drops per-side border properties.**
  `css.rs:681-686`. The catch-all's documented rationale is vendor
  prefixes and custom properties; `border-bottom` is neither, and the
  module doc at `css.rs:26-29` promises that untranslatable semantics
  emit `DroppedDeclaration`. Add explicit arms — a lint today, real
  support once proposal 1 lands.
- **The CSS importer silently drops per-corner radius.**
  `border-top-left-radius`, and therefore Tailwind's `rounded-t-*`, even
  though `Corners` supports per-corner fully (`tree/geometry.rs:245+`,
  `visual_modifiers.rs:73-82`). Same fix, same place.
- **Derived colors strip token names.** `palette.rs:14-19` — rgb-modifying
  ops detach a color from palette swaps, open namespace or not.

---

## Sequencing

**Landed (2026-07-26, on `workbench-vision`):** per-side borders, open
token namespace, radius scale — in reverse-blast-radius merge order,
each adversarially reviewed. The `RadiusScale{sm,md,lg}` + rung-tag
design did not land in any form and must not; it cannot meet its own
spec.

**Also landed (2026-07-26):** the radius-origin flag split (§3
follow-up) — tabs now square at scale 0 — the shadow scale (§4), and
the type scale (§5). Every proposal in this document is now either
landed or rejected; the workbench theme delivers all four structural
inversions through core knobs.

Container size props: dropped. If the dead `.size()` is a concern, lint
it.

**Amended 2026-07-28:** table cell padding joined the `ComponentSize`
ladder — see the box at the head of the rejection section. Per-role
container size props stay dropped.

Driving consumer for everything above: `WORKBENCH_VISION.md` (ratified
2026-07-25), the workbench opinion crate that supersedes the Carbon
experiment.
