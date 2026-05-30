//! Scroll fixture for the scroll substrate.
//!
//! Demonstrates: a vertical scroll viewport with content taller than
//! its visible rect, content clipped by the viewport, and a non-zero
//! scroll offset reflected in the laid-out tree (children translated up,
//! topmost rows clipped by the scissor).
//!
//! Run: `cargo run -p damascene-core --example scroll_list`

use damascene_core::prelude::*;
// This headless artifact example seeds scroll state before rendering,
// so it opts into the explicit advanced state/layout modules.
use damascene_core::{UiState, layout};

fn scroll_list_fixture() -> El {
    let rows: Vec<El> = (0..20)
        .map(|i| {
            row([
                badge(format!("#{i}")).info(),
                text(format!("Notification {i}")).bold(),
                spacer(),
                text(format!("{}m ago", i + 1)).muted(),
            ])
            .gap(tokens::SPACE_2)
            .height(Size::Fixed(44.0))
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        })
        .collect();

    let list = scroll(rows)
        .key("notifications")
        .height(Size::Fixed(420.0))
        .padding(tokens::SPACE_2);

    column([
        h2("Notifications"),
        text("Roll the wheel inside the panel to scroll. The content is taller than the viewport.")
            .muted(),
        list,
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn main() -> std::io::Result<()> {
    // Scroll part-way down so the artifact actually shows the offset
    // applied — the top rows clip and middle rows fill the viewport.
    // Side-map architecture: we assign_ids first to populate the
    // scroll node's computed_id, seed UiState by id, then call
    // render_bundle_with so the layout pass sees the offset.
    let mut root = scroll_list_fixture();
    layout::assign_ids(&mut root);
    let scroll_id = find_id(&root, "notifications").expect("scroll node id");
    let mut ui_state = UiState::new();
    ui_state.set_scroll_offset(scroll_id, 220.0);

    let viewport = Rect::new(0.0, 0.0, 720.0, 600.0);
    let bundle = render_bundle_with(&mut root, &mut ui_state, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "scroll_list")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}

/// Walk the tree (with `computed_id`s already assigned) and return the
/// first node tagged with `key`'s computed_id.
fn find_id(node: &El, key: &str) -> Option<String> {
    if node.key.as_deref() == Some(key) {
        return Some(node.computed_id.clone());
    }
    node.children.iter().find_map(|c| find_id(c, key))
}
