//! virtual_list — exercises the virtualized list primitive.
//!
//! 10,000 rows of fixed height in a small viewport, scrolled into the
//! middle of the list. The bundle artifacts should show that only the
//! ~handful of rows whose rect intersects the viewport are realized
//! — `tree.txt` lists rows in the visible window, not all 10k.
//!
//! Run: `cargo run -p damascene-core --example virtual_list`

use damascene_core::prelude::*;
// This headless artifact example seeds scroll state before rendering,
// so it opts into the explicit advanced state/layout modules.
use damascene_core::{UiState, layout};

const ROW_COUNT: usize = 10_000;
const ROW_HEIGHT: f32 = 44.0;

fn build_row(i: usize) -> El {
    let badge_el = match i % 5 {
        0 => badge("info").muted(),
        1 => badge("warn").warning(),
        2 => badge("ok").success(),
        3 => badge("err").destructive(),
        _ => spacer(),
    };
    row([
        text(format!("#{i:05}")).mono(),
        spacer(),
        text(format!("entry {i}")),
        spacer(),
        badge_el,
    ])
    .key(format!("row-{i}"))
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
    .height(Size::Fixed(ROW_HEIGHT))
}

fn fixture() -> El {
    column([
        h1("Virtualized list"),
        paragraph(format!(
            "{ROW_COUNT} rows × {ROW_HEIGHT}px in a windowed viewport. \
             Only the rows intersecting the viewport are realized — see \
             `tree.txt` for proof."
        ))
        .muted(),
        virtual_list(ROW_COUNT, ROW_HEIGHT, build_row)
            .key("entries")
            .height(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn find_id(node: &El, key: &str) -> Option<String> {
    if node.key.as_deref() == Some(key) {
        return Some(node.computed_id.clone());
    }
    for c in &node.children {
        if let Some(id) = find_id(c, key) {
            return Some(id);
        }
    }
    None
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();
    layout::assign_ids(&mut root);
    let list_id = find_id(&root, "entries").expect("virtual_list id");

    let mut ui_state = UiState::new();
    // Scroll so the realized window is near row 5000.
    ui_state.set_scroll_offset(list_id, 5000.0 * ROW_HEIGHT);

    let viewport = Rect::new(0.0, 0.0, 540.0, 540.0);
    let bundle = render_bundle_with(&mut root, &mut ui_state, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "virtual_list")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
