//! Tree dump — a textual semantic dump of the laid-out tree, designed
//! for the LLM agent loop.
//!
//! Each line is one node. The dump is grep-able: search for a node ID,
//! see its rect, source, and parent context. Used by the agent to
//! reason about layout symbolically without re-deriving pixel math.

use std::fmt::Write as _;

use crate::state::UiState;
use crate::tree::*;

/// Produce a tree dump string. Run after layout has populated
/// `computed_id` and the rect/state side maps in `ui_state`.
pub fn dump_tree(root: &El, ui_state: &UiState) -> String {
    let mut s = String::new();
    dump_node(root, ui_state, 0, &mut s);
    s
}

fn dump_node(n: &El, ui_state: &UiState, depth: usize, s: &mut String) {
    let indent = "  ".repeat(depth);
    let computed = n.computed_rect;
    let _ = write!(
        s,
        "{indent}{id} kind={kind} rect=({x:.0},{y:.0},{w:.0},{h:.0}) size=({sw:?},{sh:?})",
        id = if n.computed_id.is_empty() {
            "<unlaid>"
        } else {
            &n.computed_id
        },
        kind = kind_str(&n.kind),
        x = computed.x,
        y = computed.y,
        w = computed.w,
        h = computed.h,
        sw = n.width,
        sh = n.height,
    );
    // Declared accessible name (`El::aria_label` / `El::alt`) — the
    // only human-readable label an icon-only control has, so it goes
    // right after the identity columns where a reviewer scanning ids
    // will see it. Deliberately the *declared* label, not the derived
    // accname: printing every text node's own content back would
    // drown the signal.
    if let Some(name) = n.declared_label() {
        let _ = write!(s, " name={name:?}");
    }
    // Tooltip text (`El::tooltip`) — the other human-readable label a
    // node can carry, and one that is otherwise invisible headlessly
    // (it only ever paints into a hover-synthesized layer). Sits next
    // to `name=` because reviewers read the two together: a name and a
    // tooltip on the same icon-only control should not contradict.
    if let Some(tooltip) = n.tooltip_text() {
        let _ = write!(s, " tooltip={tooltip:?}");
    }
    let state = ui_state.node_state(&n.computed_id);
    if !matches!(state, InteractionState::Default) {
        let _ = write!(s, " state={state:?}");
    }
    if !matches!(n.surface_role, SurfaceRole::None) {
        let _ = write!(s, " surface_role={}", n.surface_role.name());
    }
    if n.clip {
        s.push_str(" clip=true");
    }
    if n.scrollable {
        let off = ui_state
            .scroll
            .offsets
            .get(&*n.computed_id)
            .copied()
            .unwrap_or(0.0);
        let _ = write!(s, " scroll_y={off:.0}");
    }
    if let Some(text) = &n.text {
        let preview: String = text.chars().take(40).collect();
        let suffix = if text.chars().count() > 40 { "…" } else { "" };
        let _ = write!(s, " text=\"{preview}{suffix}\"");
        if !matches!(n.text_wrap, TextWrap::NoWrap) {
            let _ = write!(s, " wrap={:?}", n.text_wrap);
        }
        if !matches!(n.text_overflow, TextOverflow::Clip) {
            let _ = write!(s, " overflow={:?}", n.text_overflow);
        }
        if !matches!(n.text_role, TextRole::Body) {
            let _ = write!(s, " text_role={}", n.text_role.name());
        }
        if let Some(max_lines) = n.text_max_lines {
            let _ = write!(s, " max_lines={max_lines}");
        }
        if !matches!(n.text_align, TextAlign::Start) {
            let _ = write!(s, " text_align={:?}", n.text_align);
        }
    }
    if let Some(icon) = &n.icon {
        let _ = write!(s, " icon={}", icon.label());
    }
    if let Some(fill) = n.fill {
        let _ = write!(s, " fill={}", color_label(fill));
    }
    if let Some(text_color) = n.text_color {
        let _ = write!(s, " text_color={}", color_label(text_color));
    }
    if let Some(custom) = &n.shader_override {
        let _ = write!(s, " shader={}", custom.handle.name());
    }
    if n.source.line != 0 {
        let _ = write!(s, " source={}:{}", short_path(n.source.file), n.source.line);
    }
    // Widget-author state buckets, surfaced for the agent loop's view.
    // Each entry is `widget_state[short_type]={debug_summary}` — the
    // type_name is short-form (final `::` segment only) since the
    // fully-qualified name is noisy.
    for (type_name, summary) in ui_state.widget_state_summary(&n.computed_id) {
        let short = type_name.rsplit("::").next().unwrap_or(type_name);
        if summary.is_empty() {
            let _ = write!(s, " widget_state[{short}]");
        } else {
            let _ = write!(s, " widget_state[{short}]={{{summary}}}");
        }
    }
    s.push('\n');

    for c in &n.children {
        dump_node(c, ui_state, depth + 1, s);
    }
}

fn kind_str(k: &Kind) -> &str {
    match k {
        Kind::Group => "Group",
        Kind::Card => "Card",
        Kind::Button => "Button",
        Kind::Badge => "Badge",
        Kind::Text => "Text",
        Kind::Heading => "Heading",
        Kind::Spacer => "Spacer",
        Kind::Divider => "Divider",
        Kind::Overlay => "Overlay",
        Kind::Scrim => "Scrim",
        Kind::Modal => "Modal",
        Kind::Scroll => "Scroll",
        Kind::VirtualList => "VirtualList",
        Kind::Inlines => "Inlines",
        Kind::HardBreak => "HardBreak",
        Kind::Math => "Math",
        Kind::Image => "Image",
        Kind::Surface => "Surface",
        Kind::Vector => "Vector",
        Kind::Scene3D => "Scene3D",
        Kind::Plot => "Plot",
        Kind::Viewport => "Viewport",
        Kind::Custom(name) => name,
    }
}

fn color_label(c: Color) -> String {
    match c.token {
        Some(name) => name.to_string(),
        None => {
            let [r, g, b, a] = c.to_srgb_u8a();
            match c.space.primaries.css_color_space() {
                None => format!("rgba({r},{g},{b},{a})"),
                Some(space) => format!("rgba({r},{g},{b},{a}) in {space}"),
            }
        }
    }
}

/// Trim a long file path to the last two components for legibility.
fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::widgets::button::icon_button;

    /// The accessible name is the only human-readable label an
    /// icon-only control has, so it has to reach the dump — that is
    /// what makes a headless reviewer able to tell two icon buttons
    /// apart.
    #[test]
    fn dump_surfaces_accessible_name() {
        let mut tree = crate::row([
            icon_button("settings").key("prefs").aria_label("Settings"),
            icon_button("bell")
                .key("alerts")
                .aria_label("Notifications"),
        ]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let dump = dump_tree(&tree, &state);

        assert!(
            dump.contains(r#"name="Settings""#),
            "expected the accessible name in the dump, got:\n{dump}"
        );
        assert!(
            dump.contains(r#"name="Notifications""#),
            "expected the accessible name in the dump, got:\n{dump}"
        );
    }

    /// Absent by default — the dump prints non-default state only, and
    /// a `name=` column on every unnamed group would drown the signal.
    #[test]
    fn dump_omits_absent_name() {
        let mut tree = crate::row([icon_button("settings").key("prefs")]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let dump = dump_tree(&tree, &state);
        assert!(
            !dump.contains("name="),
            "unnamed nodes should print no name column, got:\n{dump}"
        );
    }

    /// Tooltips only ever paint into a layer the runtime synthesizes on
    /// hover, so without a dump column they are invisible to headless
    /// review — the artifact would show an icon button and no hint that
    /// it explains itself. Same absent-by-default rule as `name=`.
    #[test]
    fn dump_surfaces_tooltip_text() {
        let mut tree = crate::row([
            icon_button("settings").key("prefs").tooltip("Preferences"),
            icon_button("bell").key("alerts"),
        ]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let dump = dump_tree(&tree, &state);

        assert!(
            dump.contains(r#"tooltip="Preferences""#),
            "expected the tooltip text in the dump, got:\n{dump}"
        );
        assert_eq!(
            dump.matches("tooltip=").count(),
            1,
            "the untipped button should print no tooltip column, got:\n{dump}"
        );
    }

    /// `.aria_label()` is stored verbatim on the node — a modifier,
    /// not a derived value.
    #[test]
    fn name_is_stored_on_the_el() {
        let el = icon_button("move").key("move").aria_label("Move");
        assert_eq!(el.declared_label(), Some("Move"));
        // Last write wins, like every other modifier.
        assert_eq!(
            el.aria_label("Reposition").declared_label(),
            Some("Reposition")
        );
        // Independent of key and tooltip.
        let el = icon_button("git-branch")
            .key("branch")
            .aria_label("Switch branch");
        assert_eq!(el.key.as_deref(), Some("branch"));
        assert_eq!(el.declared_label(), Some("Switch branch"));
        assert_eq!(el.tooltip_text(), None);
        // No label declared, nothing for the dump to print.
        assert_eq!(icon_button("bell").declared_label(), None);
    }
}
