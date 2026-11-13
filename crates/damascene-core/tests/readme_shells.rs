//! The app shells the crate README teaches under "Common app shells",
//! built through the public prelude exactly as a reader would write
//! them, laid out at a desktop viewport and held to a clean lint
//! report. An agent's first build copies these skeletons to the
//! letter, so a skeleton that lints dirty or leaves a ragged edge is a
//! defect in every app that starts from it.
//!
//! Keep the two builders in step with the README's code blocks.

use damascene_core::prelude::*;

const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1100.0,
    h: 720.0,
};

fn nav() -> El {
    sidebar([
        sidebar_header([h3("Repository")]),
        sidebar_content([sidebar_group([
            sidebar_group_label("Branches"),
            sidebar_menu([
                sidebar_menu_button_with_icon("git-branch", "main", true).key("branch:main"),
                sidebar_menu_button_with_icon("git-branch", "release", false).key("branch:release"),
            ]),
        ])]),
    ])
}

fn rows() -> El {
    scroll([item_group((0..40).map(|i| {
        item([item_content([
            item_title(format!("Commit {i}")),
            item_description("a change"),
        ])])
        .key(format!("commit:{i}"))
    }))])
    .key("commits")
}

/// README: "A two-pane workbench (sidebar + main)".
fn two_pane() -> El {
    overlays(
        row([
            nav(),
            column([
                toolbar([
                    toolbar_title("main"),
                    spacer(),
                    button("Push").primary().key("push"),
                ]),
                rows(),
            ])
            .padding(tokens::SPACE_4)
            .gap(tokens::SPACE_4)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
        ]),
        [],
    )
}

/// README: "A three-column workbench (sidebar + center + inspector)".
fn three_column() -> El {
    overlays(
        row([
            nav(),
            column([toolbar([toolbar_title("main"), spacer()]), rows()])
                .padding(tokens::SPACE_4)
                .gap(tokens::SPACE_4)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            sidebar_with(
                SidebarOpts::default().side(SidebarSide::Right),
                [
                    sidebar_header([
                        row([
                            h3("Commit 7"),
                            spacer(),
                            button("Copy ID").ghost().key("copy"),
                        ])
                        .align(Align::Center)
                        .gap(tokens::SPACE_2),
                        text("a change").muted().caption(),
                    ]),
                    sidebar_content([scroll([
                        section("Identity", [text("Author"), text("Date")]),
                        // A control flush against the pane's scroll: its focus
                        // ring needs the scroll's clip margin.
                        section(
                            "Files",
                            [text("3 changed"), button("Open diff").key("diff")],
                        ),
                    ])
                    .key("inspector")]),
                ],
            )
            .width(Size::Fixed(320.0)),
        ]),
        [],
    )
}

fn find<'a>(el: &'a El, pred: &dyn Fn(&El) -> bool, out: &mut Vec<&'a El>) {
    if pred(el) {
        out.push(el);
    }
    for child in &el.children {
        find(child, pred, out);
    }
}

fn laid_out(mut root: El) -> El {
    let bundle = render_bundle(&mut root, VIEWPORT);
    assert!(bundle.lint.findings.is_empty(), "{}", bundle.lint.text());
    root
}

#[test]
fn the_two_pane_shell_is_flush_and_lints_clean() {
    let root = laid_out(two_pane());
    let mut rails = Vec::new();
    find(
        &root,
        &|el| el.fill == Some(tokens::CARD) && el.border.is_some(),
        &mut rails,
    );
    let rail = rails[0].computed_rect;
    assert_eq!(
        (rail.x, rail.y, rail.h),
        (0.0, 0.0, VIEWPORT.h),
        "the rail runs the window's full height from its top-left corner"
    );

    // One top edge across the panes: the rail's header and the main
    // pane's toolbar (whose tallest child is the Push button) start on
    // the same line.
    let top_of = |pred: &dyn Fn(&El) -> bool| {
        let mut hits = Vec::new();
        find(&root, pred, &mut hits);
        assert_eq!(hits.len(), 1);
        hits[0].computed_rect.y
    };
    let rail_header = top_of(&|el| el.text.as_deref() == Some("Repository"));
    let toolbar = top_of(&|el| el.key.as_deref() == Some("push"));
    assert_eq!(rail_header, toolbar, "pane headers share a top edge");
}

#[test]
fn the_three_column_shell_is_flush_and_lints_clean() {
    let root = laid_out(three_column());
    let mut rails = Vec::new();
    find(
        &root,
        &|el| el.fill == Some(tokens::CARD) && el.border.is_some(),
        &mut rails,
    );
    assert_eq!(rails.len(), 2, "nav rail and inspector");
    let (nav, inspector) = (rails[0].computed_rect, rails[1].computed_rect);
    assert_eq!((nav.x, nav.y, nav.h), (0.0, 0.0, VIEWPORT.h));
    assert_eq!(inspector.w, 320.0);
    assert_eq!(
        (inspector.right(), inspector.y, inspector.h),
        (VIEWPORT.w, 0.0, VIEWPORT.h),
        "the inspector runs the window's full height at its right edge"
    );
}
