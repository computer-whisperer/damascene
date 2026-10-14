# Polish review, October 2026: the independent Opus 5.5 read

Companion to `POLISH_REVIEW_2026_10.md`. On 2026-10-14 the maintainer
asked for a second opinion on the polish question from a different
model. Claude Opus 5.5 was given the screenshot set and the five design
documents below, and the question ("why do UIs built on damascene read
as dashboards rather than tools, and what should the library own"),
without the first review's conclusions, so its read is independent. It
sampled pixel values and row pitches from the PNGs rather than judging
by eye; every hex value and pitch it quotes was measured.

The report follows verbatim from the heading below. The image names it
cites refer to this set, which is not committed:

- **apps/**: the maintainer's own apps on stock damascene, default
  theme unless noted. `damascene-volume.png` (PipeWire volume control,
  real window); `volumetric-popover.png` and `volumetric-viewport.png`
  (3D volumetric modelling tool, real window); `tristim.png` (display
  colorimeter capture tool, real window); `whisper-git-modal.png` (git
  GUI, real window, dimmed behind a modal);
  `whisper-git-commit_detail_with_submodules.png`,
  `whisper-git-diff_view_split.png`,
  `whisper-git-drilled_into_submodule.png` (the same git GUI, rendered
  from its SVG snapshot export, which approximates GPU output);
  `REFERENCE-gitkraken.png` (GitKraken, the commercial git GUI the
  maintainer keeps beside whisper-git as the comparison target; not a
  damascene app); `damascene-hero-stock.png` (the README hero: a
  hand-composed fixture on the stock theme).
- **kiosk/**: the Raven production kiosk, a colleague's dense
  instrument-panel app on the forked damascene with the workbench
  profile, as SVG snapshot renders at 1600x1000. `units-*.png`,
  `connections.png`, `production.png`, `updates.png`,
  `shell-nav-widest.png` are app pages; `ui-kit-1..8.png` are the
  kiosk's own widget-kit showcase pages.
- **docs/**: the kiosk's design language, the fork's
  `WORKBENCH_VISION.md` and `NAMING_ORACLE.md`, and main's
  `POLISH_CALIBRATION.md` and `widget_kit.md`.

Where the first review verified or pushed back on a claim here, that
is recorded in `POLISH_REVIEW_2026_10.md` part 4; this file is the
unedited source.

---

# Damascene UI polish review: what the pixels are doing wrong

I looked at all 31 images: 10 in apps/ and 21 in kiosk/. I cropped and zoomed regions, and I sampled surface colors so the claims rest on measured values rather than impressions. I read all five docs. Wherever I give a hex value or a pixel pitch, I sampled it from the PNG.

---

## 1. The missing thing

**Thesis.** Industry tools tell you what kind of thing each element is through several independent channels:
- **Surface level** says where you are.
- **Shape** says whether you can act on it.
- **Position** says its scope.
- **Icon** says its type.
- **Weight** says how much it matters.

They spend **color** only on state and identity. These UIs flatten the kind axis. Almost every element renders as one of three objects: a bordered rounded rectangle, a line of text, or an accent-filled pill. Color then has to do all the differentiating, and it runs out. When there are many actions on a screen, the shortage turns into incoherence.

**Density is not the fault.**
- **whisper-git:** commit rows sit at a 28 px pitch, the same as GitKraken's (whisper-git-commit_detail_with_submodules.png, REFERENCE-gitkraken.png). Its sidebar pitch is 28 px against GitKraken's 24.
- **The kiosk:** it applies all four of the "toy signal" inversions from the fork's WORKBENCH_VISION (dense, squared, layered, tight), and it still reads as incoherent (units-overview.png, units-debugger-flash.png).
- **Conclusion:** the four-signal diagnosis is necessary but not sufficient. What is left over is below.

### F1. Islands on a canvas, not panes in a frame

- **whisper-git is one flat color.** The toolbar, sidebar, commit list, right pane and card bodies are all #09090b. The only lighter fill (#27272a) is on card header bands and the tab strip, so chrome sits inside the content instead of around it (whisper-git-*.png).
- **tristim and volumetric are the same.** tristim is all #09090b. volumetric's popover is #09090b on a #000000 panel, so it does not lift (tristim.png, volumetric-popover.png).
- **GitKraken uses about five levels.** Graph #1c1e23, sidebar and panels #272a31, toolbar #33373f, menu bar #353535, plus selection tints.
- **Regions are separated by gaps and outlined cards, not hairlines.**
  - damascene-hero-stock.png has 11 cards, some nested inside other cards.
  - damascene-volume.png renders every device row as its own card.
  - whisper-git's right pane is four or five stacked cards, where GitKraken uses one panel with disclosure sections.
- **The apps that look best avoid the default palette.** They use the slate-blue pair #111113 / #18191b (damascene-hero-stock.png, damascene-volume.png, all kiosk pages). The apps on the zinc default are the flattest.

### F2. One shape for every kind (affordance collapse)

These all render as the same roughly 22–28 px bordered box with text:
- badges, buttons and tab cells
- toggle segments and kbd hints
- count pills and ref labels

On units-overview.png:
- **Badges:** "valve", "fw 1.4.2", "10 zones", "via HTTPS", "clock via PTP".
- **Buttons:** "Refetch", "Event log", "Firmware log".

The two groups cannot be told apart. ui-kit-1.png shows the badge ladder and the buttons side by side in the same shape. In the hero, eight cards carry a status pill.

In GitKraken, a box means clickable. Toolbar actions are borderless icons with captions, status is a colored glyph or colored text, and counts are bare numerals.

### F3. Accent is the only differentiator, and it is overloaded

**Blue on the kiosk carries at least nine meanings:**
- primary action, selected tab, selected segment, current nav item
- staged value, info badge, "not attempted", "nothing to save", read-only cell

(ui-kit-1.png, ui-kit-3.png, ui-kit-6.png, ui-kit-8.png, units-overview.png)

**White in the stock apps means both "primary" and "selected":**
- volumetric: the toggled "Bounds" pill is the same white as the "Run" button (volumetric-popover.png).
- whisper-git: "Commit", "Save" and "Done" are all white (whisper-git-modal.png, whisper-git-diff_view_split.png).

**Yellow means "anything not fine":** stale, warning, disabled reason, "actuates", "no ELF", "not uploaded".

**Disabled primaries render as a dim blue block with dark text.** They read as selected, not as unavailable (units-overview.png "Fetch settings + info", units-qc.png "Run QC…", ui-kit-1.png, ui-kit-3.png).

### F4. Actions are laid out, not organized

Every verb on these screens is visible, boxed, and placed wherever its code was written.
- There is no overflow menu, no context menu and no hover-revealed row action.
- Placement does not follow scope.
- Disabled actions take up as much layout as enabled ones.

Section 4 covers this in detail.

### F5. Words where tools use glyphs, columns and position

- **Explanatory prose fills the operating surface.**
  - units-debugger-flash.png: "Rows are keyed by index…".
  - connections.png has three table footnotes; tristim.png has a prose legend card; whisper-git-modal.png explains the keyring crate.
- **Text buttons stand in for icon toggles.** damascene-volume.png repeats "Mute" five times.
- **Metadata sits on stacked lines instead of in columns.** whisper-git puts ref labels on their own line, so row heights vary.
- **Icons say nothing.**
  - whisper-git gives every commit the same blue "T" avatar.
  - The kiosk uses the `<>` code icon as a copy glyph.
  - The volume app gives every device the same waveform icon.

### F6. Geometry comes from defaults, not content

- **Equal-width stretched tabs** (damascene-volume.png, units-overview.png, shell-nav-widest.png).
- **Fixed-width cards leaving dead zones** (production.png, damascene-volume.png).
- **Two padding recipes side by side** (damascene-hero-stock.png).
- **Clipping and collisions that shipped:**
  - units-cushion.png: text overlaps the inspector heading.
  - ui-kit-2.png: the value is clipped to a sliver.
  - volumetric-viewport.png: clipped mid-word.
  - units-overview.png: the rail chips are clipped to "pro" and "HTTF".

---

## 2. Per-screenshot findings

### apps/

**REFERENCE-gitkraken.png** is the reference; its encodings are in section 3. It has weaknesses of its own:
1. The purple AI buttons (right panel, top-right, and "Compose commits with AI") are the only off-palette color and pull the eye. A tool keeps one accent family.
2. The saturated magenta and red lanes compete with the red "ADVANCED" badge in the status bar. Lane hues need a chroma ceiling.

**whisper-git-commit_detail_with_submodules.png**
1. **Right pane.** It is four stacked bordered cards (SHA, message, files, submodules), each with its own radius and padding. GitKraken uses one panel with a header block and hairline-separated disclosure sections.
2. **Commit message.** "tagged-only" sits alone in a card with about 40 px of padding as a bold title. A detail pane shows the summary as the header line, with author, date and SHA beneath it, and no box.
3. **Commit list rhythm.** Ref labels (v-only, ORPHAN, origin/main, main) sit on their own line above the hash, so labelled rows are about 58 px against 28 px for plain rows. Put refs in a fixed BRANCH/TAG column (GitKraken) or inline before the summary, so the pitch stays constant.
4. **Avatar column.** Every row has the same blue "T" disc, which is pure noise. Color avatars by author or drop the column.
5. **Toolbar.** The repo path is a raw 120-character temp path at the same weight as the branch name. Show the repo name bold and middle-truncate the path into a muted tooltip.
6. **Shortcut strip.** A permanent kbd strip under the toolbar spends a whole row on five hints. Industry apps put shortcuts in tooltips, menus and the command palette.

**whisper-git-diff_view_split.png**
1. **Diff header.** "Discard" is solid red next to a text-only "Stage", which makes the destructive verb the loudest pixel on screen. Stage should carry the weight; Discard should be a ghost or red-text control behind a confirm.
2. **Empty split side.** Split view for a new file leaves a blank column about 600 px wide with a stub gutter. Diff tools fall back to unified, or label the empty side "new file".
3. **View toggle and status.** "Unified" at the right of the file header reads as a label, not a toggle, and "unstaged" is a bare lowercase word. Use a two-segment Split | Unified control and a status dot.
4. **Commit composer.** The summary field has no visible edge and no placeholder, and the white "Commit" button looks enabled beside "0 staged". It needs an input border, a "Summary" placeholder, and a disabled button until something is staged.
5. **Empty sections.** "Staged 0 (none)" and "Unstaged 0 (none)" each spend about 90 px as boxed empty cards. Collapse empty sections to a header with a count, like GitKraken's "Staged Files (0)".
6. **Untracked row actions.** The row has a solid red icon button with a refresh-like glyph beside "+". Row actions should be ghost icons revealed on hover, with a glyph that actually says discard.

**whisper-git-drilled_into_submodule.png**
1. **Header strips.** Four stacked strips (breadcrumb, pin bar, repo toolbar, shortcut strip) use about 180 px before any content, with no surface step between them. Fold the breadcrumb and pin into the toolbar and drop the shortcut strip.
2. **Back navigation.** "Return to parent (Esc)" is plain text at the far top-right, while its back chevron is at the far top-left. Make it one back button with Esc in its tooltip.
3. **Unexplained hash colors.** The pin bar shows the same hash, 5befacd, in blue ("pin") and in green ("parent HEAD"). Two colors for identical values imply a difference that isn't there.
4. **Siblings footer.** The chips are blue except one orange, with no legend. Encode the change state with a dot or icon and keep the chips neutral.
5. **Commit list.** The labelled-row height irregularity repeats (the PINNED and main pair on its own line).

**whisper-git-modal.png**
1. **Two primaries.** The dialog has two solid-white primaries (Save and Done) plus a text Cancel, so which one commits is ambiguous. Use one primary per dialog; the inline field action should be secondary.
2. **Token in clear text.** The token field shows the secret in the clear ("ghp_demo123"). Mask it with a reveal toggle.
3. **Dialog chrome.** There is no close affordance, and the "GitHub" and "GitLab" section labels sit at near-title weight with no separator. Use a title row, a hairline, and small muted section labels.
4. **List behind the dialog.** Even dimmed, it shows irregular rhythm: 28 px rows, roughly 50 px ref rows, and blank gaps after tag groups. This is the same cause as finding 3 for commit_detail.

**damascene-hero-stock.png**
1. **Cards everywhere.** Every region is a bordered card: 11 of them, including the sidebar, plus four step cards nested inside "Downlink window". Nested cards are the landing-page tell; Grafana and Linear use flush panels with section headers.
2. **Status pills.** Eight status pills ("nominal", "draining", "on plan", "in pass", "hot", "live", "next", "queued") in four colors say less than a dot plus text would. Grafana colors the value or adds a threshold glyph, not a pill per panel.
3. **Inconsistent header insets.** "Command surface" and "Maneuver window" titles sit about 36 px below their card tops; "Ground stations", "Downlink window", "Orbit view" and "Pass queue" sit about 52 px below. Sibling panels need one header inset.
4. **Page header.** An H1, a two-line subtitle, search, a primary button and "…" is a marketing-page heading. Tools put context in the title or breadcrumb and keep a 32–40 px toolbar.
5. **Unfilled bottom row.** "Pass queue" leaves its lower half empty, and the globe uses about 30% of "Orbit view". Heights come from the grid, not from content.
6. **Shortcut legend.** "Command surface" spends the prime top-right slot on a keyboard legend, the same choice as whisper-git's strip.

**damascene-volume.png**
1. **Device cards.** Each device row is its own rounded, bordered card with gaps between them. Mixers (pavucontrol, macOS Sound, the Windows mixer) are rows separated by hairlines or by nothing.
2. **Mute and default.** "Mute" and "Set Default" are text buttons repeated five times, and mute state is invisible. Use a speaker / speaker-x icon toggle by the slider, and a check or radio for the default device.
3. **Over-amplification.** The slider goes past 100% (127%, 211%) with no 100% tick and a uniform blue fill. Mark 0 dB and tint the over-amplification range, as pavucontrol does.
4. **Device names and icons.** Every device has the same waveform icon, and names truncate at a fixed width while the right-hand columns are fixed too. Use device-class icons (headphones, monitor, speaker) and let the name take the flexible column.
5. **Header.** The tab strip is a full-width segmented control with five equal cells, under a 32 px "Volume Control" display title and logo. A desktop app leaves the title to the window and uses a hugging tab bar.
6. **Dead space and footer.** There is a band of about 135 px between the list and the spectrogram, and the status bar shows debug counters ("17 nodes · 7 cards · 5 meters").

**tristim.png**
1. **Selected toggles lose contrast.** "Lab" and "3D Space" render as white pills with gray text, so the selected option has lower contrast than the unselected ones. It looks like a stock style-path bug, worth checking in the library.
2. **Three choice tiers look identical.** View (3D / Chromaticity / Luminance), space (Lab / Lab·abs / …) and gamut (abs and rel sRGB / P3 / 2020) are all detached, gapped buttons. Use connected segmented controls, with the view switch as tabs on the plot panel.
3. **Legend as prose.** The legend card takes the right column ("dots — measured samples, in their own colour…"). Put a glyph legend inside the plot and the detail in a tooltip.
4. **Axis label collisions.** The b* ticks (100 / 80 / 60 / 40 / 20) pile onto the L* axis, and L* "100" collides with the cage corner. Thin or offset the ticks.
5. **Status header.** It is three stacked blocks at similar weight ("tristim / running capture", "Capturing…", "unmanaged · xrgb8888"), each with a dot-joined subline. One status bar holding the capture state and progress would replace all three.

**volumetric-popover.png**
1. **Popover surface.** The popover (#09090b) sits on a #000000 panel with only a faint border, and it covers the inspector: "step_import_op" is clipped and a red button's label is hidden. Popovers need a raised surface and shadow, and placement that doesn't occlude the panel they belong to.
2. **Resolution chips.** Resolution is 13 chips wrapping over three rows. A select, or a slider with power-of-two stops, takes one row.
3. **Property rows.** Steppers are bare chevrons around a number with no field box, toggles align at a different x, and row heights vary (about 36 vs 28 px). In Blender and Figma, property rows are a fixed label column plus a fixed-width field with the arrows inside it, at one pitch.
4. **Mixed content.** Mesh statistics (S1–S4.5 timings) and the Export buttons share the popover with render settings. Split info (status or log) from settings.
5. **Viewport toolbar.** Toggles (Grid, Bounds, SSAO) and momentary actions (Frame, Reset) look the same, and the white "on" state matches the "Run" primary. Use icon toggles with a pressed state and plain icon actions, grouped by dividers.
6. **Top bar.** It runs status pills ("ran 883ms", "ready 1 output"), three labelled switches, a chevron, a download icon and Run together in one row. Status belongs in a status bar; the switches belong in a split-button dropdown on Run.

**volumetric-viewport.png**
1. **Narrow window.** The right panel and top bar clip mid-word ("wgsl_script_operato", "desig", "Auto me"), and a toolbar select shows only "V…". Panels need minimum widths or collapse, and toolbars need an overflow menu.
2. **Unlabelled red bar.** A full-width solid red bar sits in the inspector under "Precursors" with no visible label. A destructive control should never be the unlabelled loudest thing in an inspector.
3. **Tool rail.** Nine icons with no active state and no grouping. Blender and Figma rails fill the active tool and separate groups.
4. **Inspector truncation.** Rows ("Script desig…", "Parameters Inline…") clip with no ellipsis.

### kiosk/

**units-overview.png**
1. **Verb list.** In the Read / Act / Sessions / Go to list, 5 of 7 verbs are disabled. Each disabled verb renders as a gray box plus a reason line, and "no settings transport advertised" appears three times here and a fourth time in Device snapshot. State the shared cause once and dim the verbs, with per-verb reasons in tooltips.
2. **Disabled primary.** "Fetch settings + info" is a dim blue block with dark blue text, so it reads as selected. Disabled should be neutral gray.
3. **Zone table.** Ten three-way segmented controls make 30 buttons with 10 blue fills, and the "live valve" cells are boxed green fills. Use a compact select or icon toggle per row and a dot for live state.
4. **Header facts.** The badge row ("valve", "fw 1.4.2", "10 zones") has the same shape as the buttons ("Refetch", "Event log", "Firmware log"). The `<>` copy icons float about 250–360 px from the values they copy.
5. **Tab bar.** Equal-width segmented tabs with a filled active cell, which is the same treatment as the value toggles below. Use underline tabs, left-aligned.
6. **Global chrome.** The status strip holds a full-height solid primary ("Start session"), and the banner has a persistent solid red "Close all & stop". The left nav carries a 170×190 gray placeholder ("image:5730e79b") on every page.

**units-debugger-flash.png**
1. **Four look-alike segmented controls.** Unit tabs, the Pane switcher (Chip / Flash / Slots / Provision / OTP), SWD speed and the Slot B policy look the same at four levels of hierarchy. Navigation should look like tabs and values like selects or segments, stepping down in size as they nest.
2. **Info alert.** The entire paragraph is blue text on a blue tint. Body text should be the foreground color, with only the icon or edge tinted.
3. **Images table.** "kind" and "boot confirm" are boxed color-filled cells (green, yellow, blue), which reads as a checkerboard. Use a dot plus text, with no per-cell borders.
4. **Verb far from its option.** "Flash…" sits alone at x≈1550, while its only option (Slot B invalidate / keep) is at x≈576 with a two-line explanation in between. Put the option and the verb in one footer row.
5. **Developer footnote.** "Rows are keyed by index — … Selecting a row never starts anything." explains the developer's model on the operator's surface.

**units-zones.png**
1. **One failure, four surfaces.** A single rejection shows as a red alert over the canvas, a rejected field in the inspector, × badges in the change list, and red text in the commit bar. Give the error one home plus an inline marker.
2. **Change list overlap.** The "Will be written" panel floats over the inspector's tuning fields ("offload" and "Δ floor" are cut off). Dock it, or make it a popover that doesn't cover editors.
3. **Chip row above the canvas.** Six chips in mixed styles ("broadcast · bus · just now", "running preset", a red-dot "Comfort", "explicit", "redistribute 600 s", "pump · inflating"), the last clipped. That is a fact bar, so it should be text with separators.
4. **Inspector fields.** They change footprint by state: an empty "min h" box with loose −/+ glyphs, a staged field with a left bar and blue tint, a rejected field with a red box. Keep one frame and recolor it.

**units-settings.png**
1. **Filter input.** The input left of All / Editable / Staged / Read-only is an empty dark box with no placeholder or search icon. It reads as a blank bar, not a field.
2. **Fact bar truncation.** "Schema 6 knobs · 2 groups ·…", "no settings transpor…" and "6 of 6 show…" all truncate. Show fewer facts and move the rest into a tooltip.
3. **Selected row overload.** One row carries a rejected badge, an × button, a red-bordered stepper and a blue left bar: four state markers. Industry settings editors use a single modified bar.
4. **Change list clipping.** The change-list panel overlaps the table, and its own last paragraph is cut mid-line by the commit bar.

**units-cushion.png**
1. **Collision.** "no label printer attached" runs underneath the inspector heading "Select a zone to edit its label and channel". Text must clip or ellipsize at the pane edge.
2. **Doc bar verbs.** "Read from unit" (primary), "Load STEP…", "Load design…", "Open file…" and "New" sit in one row, plus "Save file", "Print label" and a full-width primary "Write to unit…". With the red "End session" and "Close all & stop", the page has four filled calls to action. File verbs belong in a menu or split button.
3. **Canvas debris.** Zone labels overprint the ellipse outline, and a placeholder tag ("image:ef5f3f18") renders mid-canvas.
4. **Duplicate step controls.** There is an arrow nudge pad with "1 mm" in its center, and a "1 mm | 10 mm" segment next to it. That is two controls for one step size.

**units-events.png**
1. **Empty state.** It is centered across about 1000 px with a single description line about 900 px long, and the "Typically 5–15 s… watchdog" footnote floats 60 px below, left-aligned. Constrain empty states to about 480 px and keep their text together.
2. **Third selected style.** "Event rings | Firmware log" uses a solid bright-blue selected fill. The tab bar uses a dark-blue fill with blue text and the debugger Pane switcher uses yet another, so that is three selected-state treatments in one app.
3. **Unused content area.** About 400 px of content area is empty. Put "Fetch" in a tab toolbar and show the table headers or cached entries in the body.

**units-presets.png**
1. **Activate per card.** Two solid-blue "Activate" buttons and one outlined (the running preset) repeat the primary verb on every card. In a list, the per-item verb is secondary, and the active item gets a check or radio.
2. **Edit link.** "Edit" is bare text about 550 px from each card's title, although a selection-driven inspector already exists. Row selection should open the inspector; drop the link.
3. **Inspector grammar.** Within one column it mixes a yellow warning paragraph, a swatch row, a chevron row ("Behaviour") and a mini table. Each is a different row grammar.

**units-qc.png**
1. **"actuates" markers.** Five yellow boxed cells in the hardware column read as five warnings. A small icon (a zap) or muted text says "this step moves hardware" without alarm.
2. **Disabled Run QC.** The disabled "Run QC…" is a dim blue blob with its reason below, and a lone "Connections" link sits at the far right. Put the remedy next to the reason.
3. **Row heights.** The "what it does" prose wraps unevenly, so rows vary between about 24 and 36 px. Truncate to one line with a tooltip or a detail row.

**connections.png**
1. **Mixed control strip.** The policy line mixes a button group ("Re-enumerate: Interfaces / Probes / mDNS") with three switches and their inline labels, with no separators. Separate toolbar verb groups with dividers and move policies to Preferences or a settings popover.
2. **Cell boxes.** "pinned on" and "pinned off" render as blue-outlined boxes, and "attached" and "not claimed" as green or gray boxes. These are cell-level boxes again.
3. **Footnotes.** Each of the three tables ends with a prose paragraph. A tool keeps that in an (i) tooltip or the docs.
4. **Truncated cells.** "radvd is not installed on this host — ins…" and "needs monitoring — radvd advertises on a monito…" carry long reasons. Show a short status in the cell and the reason on hover.
5. **Two primaries.** "Re-check bench" and "Scan 12 s" are both solid blue, alongside the global red pair.

**production.png**
1. **Contradicting headline.** "Both sides are provisioned and running the current release" sits directly above three ⚠ findings (Side missing, Firmware, Health) that contradict it. The headline should be derived from the worst finding.
2. **Findings layout.** Each finding is a label, a wrapped paragraph and an arrow-prefixed remedy line, three times over. A severity icon with one line each, and the remedy as a button, would be scannable.
3. **Fixed card widths.** Two fixed-width cards leave about 440 px empty on the right, with mismatched bottoms. Use a fluid grid or a table of boards.
4. **Acceptable here.** The full-width blue "Run QC…" is acceptable on this operator-register page; it is the one place a single big primary is right.

**updates.png**
1. **Three primaries.** "Retry sync", "Sync all (2)" and "Restart kiosk now…" are all solid blue on one page. One primary per surface.
2. **Fact bar encodings.** It mixes green, gray and two blue chips with a raw feed URL, a truncated prose sentence and a Refresh button. That is four encodings for facts that should be uniform text.
3. **Heading collision.** "Release feed" is followed on the same baseline by a smaller "channel" label and the chip "stable — the only channel in this feed". Metadata should sit below the heading or in muted text.
4. **Prose in the table.** The expanded row nests a kv grid and a verbose error sentence, then two more explanatory paragraphs before the buttons. Too much copy inside the table.

**shell-nav-widest.png**
1. **Wide nav, no gain.** The nav column grows to about 480 px but holds the same six items and a centered gray placeholder. Cap the nav width; the placeholder is a missing asset rendering as a debug box.
2. **Tab spacing.** With equal-width cells, "Telemetry" and "Event Log" nearly touch while "QC" and "Bench" have wide gaps. Use hugging tabs with a fixed gap and an overflow menu.
3. **Empty ellipses.** "Schema …" and "Generation 0 …" are ellipsized down to zero characters. Drop a fact rather than render a bare ellipsis.

**ui-kit-1.png**
1. **Badges look like buttons.** Badges ("not run", "usb-eth", "slot B", count "12") have the same shape and height as the "Refresh" and "Apply" buttons in the left column. Status should lose the box so that a box means clickable.
2. **Two disabled languages.** "Upload & update" is blue-filled with near-invisible text, while "Apply 2 staged changes" and "Program OTP" are outlined with dim text.
3. **Remedy placement.** The remedy "Start session" floats right-aligned as plain text, about 250 px from the reason it fixes, and doesn't look like a button.
4. **kv_grid copy icon.** It uses `<>` (the code icon) for copy, pinned to the far right edge. Use a copy glyph next to the value, revealed on hover.

**ui-kit-2.png**
1. **Clipped values.** The cell form clips the value to a sliver: "ꔪ ms" untouched, "` ms" staged and rejected. The suffix eats the field, so the value needs a minimum width or the unit belongs in the column header.
2. **States change the frame.** Staged adds a left bar and a row tint, error a red box around the field and steppers, live a green bar. Recolor one constant frame instead.
3. **Steppers outside the field.** −/+ sit outside the input border as loose glyphs, so the field edge and the hit target disagree. Blender and Figma put the arrows inside the field.
4. **Disabled reason in warning yellow.** The disabled reason reads as an alert, not an explanation.

**ui-kit-3.png**
1. **Commit bar states.** Idle "Apply" is a blue block with dark text, and in-flight "Applying 2 of 4…" truncates inside its own button. Busy should be neutral with a spinner and a short label.
2. **Blue collision.** The change-list badges use blue for "not attempted", which collides with blue for staged, primary and selected.
3. **Confirm bar variants.** The destructive variant is a red tint with a solid red primary; the caution variant is a brown tint with a blue primary. One component, two color systems.

**ui-kit-4.png**
1. **Remedy buttons.** "Halt & retry" (primary), "Retry slower" and "Mass erase (unloc…" wrap, with the destructive verb truncated on its own row. Never truncate a destructive label; put remedies in a vertical list or menu.
2. **Mechanism copy.** "a probe flash cannot be cancelled mid-write — the worker is watchdogged" breaks the kiosk's own §11 copy law, yet the kit still demonstrates it.
3. **Equal weights.** The "◆ running" badge and the "Cancel" button carry equal weight in the header, and enabled and disabled Cancel differ only slightly.

**ui-kit-5.png**
1. **Filled buttons in the status bar.** The status strip carries a filled primary ("Start session") and a filled red one ("End session"). VS Code status items are flat text plus icon with a hover background, never filled buttons.
2. **Filters drawn as buttons.** In the log sheet, "all levels" and "all units" are bordered buttons with no chevron, so you can't tell they are pickers.
3. **The best idiom in the set.** The section frames (header row, right-aligned headline value, chevron) are the most tool-like pattern in all 31 images. They should be a stock primitive.

**ui-kit-6.png**
1. **Inverted hierarchy.** The header row (about 16 px) and the group row "pump.*" (about 17 px) are larger than the 13 px data rows. Headers should be a smaller, muted caption.
2. **Read-only cell.** It is a blue-filled box spanning the staged column, which makes the one cell you can't edit the loudest thing in the table.
3. **Calmer variant.** The "content_table" variant at the bottom is the better default.

**ui-kit-7.png**
1. **Scrollbars.** Always visible, 6–8 px wide and rounded, sitting beside the pane dividers so they read as double rules. Use overlay scrollbars that appear on hover or scroll.
2. **No surface step.** Pane heads ("identity · tabs") are text over a hairline with no fill step, so head and body read as one surface.

**ui-kit-8.png**
1. **Overloaded pref row.** One row carries two badges ("applies now", "not saved"), a switch, a yellow paragraph and a text "Retry". That is five elements for one boolean.
2. **Doc bar.** "Read from unit…" is solid blue in every state, including "saved", and "nothing to save" is a blue badge. The primary should follow the next useful action.
3. **Badge on every card.** The panel_card header badges ("✓ current", "not uploaded") repeat the hero's pill-per-card pattern.

---

## 3. whisper-git vs GitKraken, encoding by encoding

The information architecture is the same: repo sidebar, commit graph and list, detail and commit pane. The encodings differ as follows (whisper-git-*.png vs REFERENCE-gitkraken.png).

- **Color as identity.**
  - **GitKraken:** each branch lane's color runs across the whole row. It fills the ref label, colors the graph line and the avatar ring, tints the row band in the graph column, and draws a colored left edge on the message cell.
  - **whisper-git:** color appears only in ref label text and outline (orange tag, blue remote, green local). The lanes are thin, and every avatar is the same blue.
- **Color as surface.**
  - **GitKraken:** a blue-gray ramp of about five levels (#1c1e23 list, #272a31 sidebar and panels, #33373f toolbar, #353535 menu bar), with a green tint for the selected worktree.
  - **whisper-git:** one level, #09090b, for everything. The only lighter fill (#27272a) is on card header bands; selection is #232325 with a 1 px outline box, and zebra striping (#0e0e0f) is effectively invisible.
- **Icon.**
  - **GitKraken:** every sidebar row has a type glyph (branch, cloud, worktree, submodule, GitHub) and every section header has an icon. Toolbar verbs are icons with captions (Undo, Redo / Pull, Push, Branch, Stash, Pop / Terminal), file rows get an orange pencil for modified, and submodules get green checks.
  - **whisper-git:** chevrons, a file icon reused for tags, a letter "A" for added, an uninformative "T" avatar, and text-labelled bordered toolbar buttons.
- **Weight.**
  - **GitKraken:** two weights per row (bright summary, muted body on the same line). Paths are dimmed directory plus bright basename. Pickers are a tiny caption ("repository", "branch") over a bold value.
  - **whisper-git:** the summary at one weight with no body in the list. The toolbar shows a raw full path at the same weight as the branch. The commit summary in the detail pane is set as a bold title inside its own card.
- **Density.**
  - **Rows:** equal at 28 px. The sidebar pitch is 24 px in GitKraken vs 28 px in whisper-git.
  - **Vertical rhythm:** GitKraken keeps every commit row at 28 px. whisper-git breaks it whenever a commit has refs (about 58 px rows), so fewer commits fit and the eye can't track rows.
  - **Header strips:** chrome above content is about 105 px in GitKraken (two strips), against about 180 px of header strips in whisper-git-drilled_into_submodule.png.
- **Spacing.**
  - **GitKraken:** panes butt together with 1 px dividers; nothing floats.
  - **whisper-git:** 8–16 px gutters between the sidebar, a rounded list card inset from the window, and right-pane cards with gaps between them.
- **Grouping.**
  - **GitKraken:** one right panel with disclosure headers ("Unstaged Files (3)", "Staged Files (0)") over hairlines. The commit composer is docked at the bottom, and its button is muted until something is staged.
  - **whisper-git:** separate cards per section, and empty sections render boxed "(none)". The composer sits at the top above empty lists, with a white Commit button at 0 staged.
- **Chrome.**
  - **GitKraken has:**
    - column headers in small mono caps (BRANCH / TAG, GRAPH, COMMIT MESSAGE)
    - a toolbar grouped by gaps and dividers, with Pull as a split button
    - a status bar (notifications, zoom, version)
    - uppercase sidebar section headers with accent-blue counts
  - **whisper-git has:**
    - no column headers and no status bar
    - Pull and Push dropdowns as separate bordered boxes beside their buttons
    - a permanent kbd strip
    - sidebar counts in gray pills, each with its own "+" button
- **Placement.**
  - **GitKraken:** ref labels live in their own left column, joined to their commit node by a lane-colored line.
  - **whisper-git:** labels stack above the hash, so ref information competes with the commit text instead of sitting in a scannable column.

**Bottom line:** GitKraken is not denser than whisper-git. It is more encoded. It uses four or five channels per row where whisper-git uses one or two, and that is where the "missing something" lives.

---

## 4. Dense action sets

### Why the dense screens read as incoherent
(units-overview.png, units-debugger-flash.png, volumetric-popover.png, ui-kit-1 through ui-kit-8)

1. **Placement doesn't encode scope.** On units-overview.png:
   - a global act (Close all & stop) is in the top banner;
   - session start is in the status strip;
   - unit verbs are a vertical list in the body;
   - a per-card fetch repeats one of those verbs;
   - refresh is at the top-right;
   - navigation ("Go to… Event log") is drawn as buttons.

   Nothing tells the eye "this acts on the unit" versus "this acts on the app".
2. **Everything is rendered and nothing is disclosed.** There are no overflow menus, context menus or hover-revealed row actions. volumetric-popover.png lays out all 13 resolutions; units-debugger-flash.png lays out all five SWD speeds.
3. **Disabled actions take up layout.** The kiosk's §4 rule ("disabled with a reason, not hidden", reason inline) turns each guard into a box plus a sentence. On units-overview.png that is 5 of 7 verbs, and one sentence repeats four times.
4. **One shape and weight for verbs and facts.** Buttons, badges, segment cells and chips are the same boxed 24 px text (ui-kit-1.png), so the eye can't sort "things I can do" from "things that are true".
5. **Accent goes to selection, not to intent.** On units-overview.png there are about 13 blue elements (10 table segments, tab, nav, Start session, dim Fetch), so the one thing to do next doesn't stand out.
6. **Navigation and values share one control.** units-debugger-flash.png: tabs, pane switcher, speed and slot policy all use the same segmented drawing. In the volumetric toolbar, toggles and momentary actions are the same chip.
7. **States add elements instead of recoloring.** In the ui-kit pages a rejected staged cell carries a badge, an ×, a colored border and a bar (ui-kit-2.png, units-settings.png). Multiply that across four columns and ten rows and the table becomes noise.
8. **Help text sits among the controls.** Footnotes and reasons in prose (connections.png, units-debugger-flash.png, ui-kit-4.png) occupy the same visual tier as controls.
9. **No alignment grid.** Verbs sit far from their options ("Slot B" vs "Flash…"), and copy icons float in the middle of the page (units-overview.png).

### How industry apps keep dense action sets legible

- **Scope maps to place.**
  - window toolbar or title bar → view-scoped actions
  - inspector header or footer → selection-scoped actions
  - row hover or context menu → row-scoped actions
  - status bar → passive state with click-through

  The scope can be read without labels. GitKraken's toolbar, trash and AI buttons are all in the expected places (REFERENCE-gitkraken.png).
- **Three action tiers, and only one is boxed.** One solid primary per view, outlined or neutral secondaries, and tertiary actions as borderless icons that only show a background on hover. Facts are never boxed.
- **Enumerations go into selects and rare verbs into menus.**
  - Split buttons pair a verb with its options (GitKraken's Pull ▾).
  - Overflow "…" holds the long tail.
  - The command palette (VS Code ⌘⇧P, Linear ⌘K) indexes every verb, so the surface doesn't have to show them all.
- **Disabled means dimmed in place.** The reason goes in the tooltip; a shared cause is stated once (a banner or empty state); the layout doesn't move.
- **Accent is budgeted.**
  - Selection uses a desaturated fill (VS Code's list.activeSelectionBackground; GitKraken's green-tinted worktree row).
  - Saturated accent is reserved for the primary and the focus ring.
  - Status uses glyph-colored dots, not filled pills.
- **One state channel per item.** VS Code's settings editor marks a modified setting with a 2 px left bar and nothing else. Figma marks an overridden property with a single tint.
- **Property grids.** Blender's N-panel and Figma's right panel use a fixed label column, a fixed field column, arrows inside the field, collapsible sub-panel headers, and a constant row pitch. That is the shape volumetric-popover.png and ui-kit-2.png are reaching for.

---

## 5. Library vs app

### (a) Caused by library defaults or missing primitives (the author has to fight the library)

1. **Flat default surfaces.**
   - **Evidence:** on the zinc default, CARD, BACKGROUND and POPOVER all render #09090b (sampled in whisper-git-*.png, tristim.png and volumetric-popover.png).
   - **Contrast:** apps on the slate-blue variant get one step (#111113 / #18191b in damascene-volume.png, damascene-hero-stock.png and the kiosk). The README hero, the library's "best stock look", is not on the default palette.
   - **Upstream:** shadcn's own Tailwind-v4 neutral themes moved card off background, if I recall correctly.
2. **Card as the grouping primitive.** widget_kit.md's "Common smells" tells authors that grouped content is a `card(...)`. Core has no pane or section-with-hairline primitive, and per WORKBENCH_VISION it has no per-side borders either. Evidence: whisper-git's right pane, the volume rows, the hero.
3. **Badges as boxed pills, taught as the status idiom.** The same smells list rewrites `text("● Online")` into `badge("Online").success()`, which is the opposite of industry practice (dot plus text). Evidence: damascene-hero-stock.png pills, units-overview.png chips, ui-kit-1.png.
4. **Selected state shares the primary fill.**
   - Evidence: volumetric-popover.png ("Bounds" = "Run").
   - Separately, tristim.png renders selected toggles as muted text on a white fill, which looks like a stock bug.
5. **Disabled solid buttons render as the variant at reduced opacity.** On a dark background that reads as a selected fill (units-overview.png, units-qc.png, ui-kit-1.png, ui-kit-3.png). The kiosk is on a fork, but this is the shadcn-derived treatment.
6. **Stretched equal-width tabs.** Both apps that use tabs render them stretched (damascene-volume.png, units-overview.png). If that is the stock `tabs_list` default, it is a library issue.
7. **Clip as the default text overflow** (widget_kit.md §3.1), plus ellipsis that can collapse to zero characters. Evidence: units-overview.png ("pro", "HTTF"), volumetric-viewport.png, shell-nav-widest.png ("Schema …").
8. **Input with a suffix clips its own value** (ui-kit-2.png). The overflow lint didn't catch it, and it didn't catch the cross-pane text collision on units-cushion.png either.
9. **Small icon set** (widget_kit.md: "the built-in set is small"). Evidence: `<>` used for copy on the kiosk, a file icon for tags in whisper-git, generic icons in volume.
10. **No tool-chrome primitives in core:** status bar, split button, icon-with-caption toolbar button, property row with inline steppers, list column headers, tree, hover row actions packaged as a component.
    - whisper-git hand-rolls a sidebar tree and a shortcut strip.
    - volumetric hand-rolls steppers.
    - The kiosk's status strip and section frames were built on the fork.
11. **The docs teach a page, not a tool.**
    - widget_kit.md's shell example is `.padding(SPACE_8).gap(SPACE_6)` with a `toolbar_title`, which is a landing-page rhythm.
    - §3.2 recommends muted subtitle and description text under titles. Agents fill every slot they are shown.
12. **Table headers larger than body rows.** This appears in the fork's `data_table` (ui-kit-6.png, units-settings.png, units-overview.png).

### (b) App-level taste

- **whisper-git:**
  - refs above rows, identical avatars, the permanent shortcut strip, the raw path
  - composer order, boxed "(none)" sections
  - solid red Discard, two primaries in the dialog, a cleartext token
- **volume:** text Mute buttons, the debug footer, the display title, no 100% mark.
- **tristim:** the prose legend card and the three-block status header.
- **volumetric:** 13 resolution chips, stats in the settings popover, the top-bar composition.
- **kiosk:**
  - inline disabled-with-reason (its own §4)
  - footnote prose despite its own §11
  - a permanent red banner action, several primaries per page
  - the nine-role badge ladder with a glyph ladder on top (§6.2 item 6)
  - segmented cells in tables, the placeholder image
- **hero:** a badge on every card and the marketing page header.

### Both

- **Accent overload.** The library offers one accent and makes `.primary()` the cheapest form of emphasis; apps then spend it on every button they care about (updates.png, units-cushion.png, whisper-git-modal.png).
- **Cards everywhere.** The library makes the card the cheapest group; apps never opt out.
- **Prose.** The library provides description slots and the apps fill them.
- **Disabled-in-layout.** It is the kiosk's rule, but the library has no "toolbar icon, dimmed, reason in tooltip" idiom that would make the right path the short one.

---

## 6. Cheap wins (library changes, ranked by pixels moved per unit of work)

1. **Give the default dark palette a surface ladder.**
   - **Change:** background < sidebar/panel < card/raised < popover. Making the slate-blue values the default would cover most of it.
   - **Also:** give popovers a visible shadow.
   - **Evidence:** #09090b everywhere in whisper-git-*.png and tristim.png; the popover is indistinguishable in volumetric-popover.png; the slate-blue apps (damascene-volume.png, damascene-hero-stock.png, kiosk) already look better.
   - **Cost:** token values only.
2. **Make the default badge a dot plus text, with no box and no border.**
   - **Change:** keep the boxed form as an explicit `.outline()` or tag variant, and rewrite the "status as unicode bullet" smell to recommend it. After this, a box means clickable.
   - **Evidence:** damascene-hero-stock.png pills; the header chips vs buttons in units-overview.png; ui-kit-1.png.
3. **Selected ≠ primary.**
   - **Change:** selected toggle, segment and tab states use a neutral raised fill (secondary or accent) with FOREGROUND text, never the PRIMARY fill.
   - **Also:** fix the muted-text-on-white case.
   - **Evidence:** volumetric-popover.png "Bounds" vs "Run"; tristim.png "Lab"; the blue segment field in units-overview.png.
4. **Disabled solid buttons go neutral.**
   - **Change:** MUTED fill with MUTED_FOREGROUND text, instead of the variant at opacity.
   - **Evidence:** units-overview.png "Fetch settings + info"; units-qc.png "Run QC…"; ui-kit-1.png "Upload & update"; ui-kit-3.png idle "Apply".
5. **Default tabs hug and left-align, with an underline indicator.**
   - **Change:** move the stretched segmented look to `toggle_group`, or to a `segmented` name.
   - **Evidence:** damascene-volume.png, units-overview.png, shell-nav-widest.png.
6. **Ship a section/pane primitive and fix the docs.**
   - **Primitive:** a header row (disclosure, title, count, right-aligned headline value, trailing actions) over a hairline, with per-side borders in core. ui-kit-5.png's section frames are the template.
   - **Docs:** demote `card` in the smells list, and make the first shell example in widget_kit.md a flush tool shell (toolbar, sidebar, panes, status bar, zero page padding).
   - **Evidence:** whisper-git right pane vs GitKraken; damascene-volume.png rows; damascene-hero-stock.png.
7. **Toolbar context.**
   - **Change:** buttons inside `toolbar()` default to ghost or borderless.
   - **Also ship:** `icon_button_with_caption` and `split_button`.
   - **Evidence:** the GitKraken toolbar vs whisper-git's bordered Fetch / Pull / ▾ / Push / ▾; the volumetric viewport toolbar.
8. **Ellipsis by default for single-line text in bounded boxes.**
   - **Change:** never ellipsize below about 3 visible characters (drop the item instead), and give inputs a minimum value width so the suffix can't eat the value.
   - **Evidence:** units-overview.png rail chips; volumetric-viewport.png; shell-nav-widest.png; ui-kit-2.png.
9. **Table and list headers use the caption role.**
   - **Change:** smaller and muted, never larger than the body. Group rows match body size.
   - **Evidence:** ui-kit-6.png, units-settings.png, units-overview.png. Compare GitKraken's small mono-caps column headers.
10. **Add four cheap lints, and widen the icon set.**
    - **Lints:**
      - more than one solid primary per pane or dialog (whisper-git-modal.png, updates.png, units-cushion.png)
      - card nested in card (damascene-hero-stock.png)
      - an input whose visible value is narrower than 3ch (ui-kit-2.png)
      - text overlapping a sibling across a pane boundary (units-cushion.png)
    - **Icons:** add copy, trash, git-merge, git-pull-request, file-diff, pencil, volume-x, headphones and monitor-speaker from Lucide.
    - **Evidence:** the `<>` copy glyph on the kiosk, the tag-as-file icon in whisper-git, identical device icons in volume.

The first five are value and default changes inside existing widgets. Together I'd expect them to remove most of what reads as "shadcn demo" in the whisper-git, tristim, volumetric and volume screenshots, with no app code changed.
