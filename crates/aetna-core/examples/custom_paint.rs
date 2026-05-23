//! custom_paint — headless bundle dump for the "custom-painted commit
//! graph" pattern from `examples/src/bin/custom_paint.rs`.
//!
//! No GPU, no winit, no shader compilation — just the CPU bundle path.
//! Inspecting the artifacts answers two of the host-paint questions
//! whisper-git would ask of aetna:
//!
//! - **Custom-shader Els are visible in the artifact stream.** Each
//!   shader-bound row appears in `out/custom_paint.draw_ops.txt` as a
//!   `Quad shader=custom::commit_node ...` with its full uniform set,
//!   and the shader manifest groups every instance under
//!   `custom::commit_node used N times`. A host's commit-graph paint is
//!   not a black box to the agent loop.
//!
//! - **The SVG fallback degrades gracefully.** Custom-shader rects emit
//!   the documented dashed-magenta placeholder; the surrounding text
//!   and chrome render normally.
//!
//! Run: `cargo run -p aetna-core --example custom_paint`

use aetna_core::prelude::*;

const ROW_HEIGHT: f32 = 28.0;
const GRAPH_WIDTH: f32 = 140.0;
const LANE_COUNT: u8 = 4;

struct FakeCommit {
    sha: &'static str,
    subject: &'static str,
    author: &'static str,
    when: &'static str,
    lane: u8,
}

fn lane_palette(lane: u8) -> Color {
    match lane % LANE_COUNT {
        0 => Color::srgb_u8(96, 165, 230),
        1 => Color::srgb_u8(96, 200, 200),
        2 => Color::srgb_u8(140, 200, 110),
        _ => Color::srgb_u8(230, 180, 90),
    }
}

fn graph_cell(lane: u8, selected: bool) -> El {
    let lane_color = lane_palette(lane);
    let ring_color = if selected {
        Color::srgb_u8(245, 245, 250)
    } else {
        lane_color
    };
    let ring_w = if selected { 2.5 } else { 1.5 };
    let radius = 5.0;
    let line_w = 2.0;
    let lane_frac = (lane as f32 + 0.5) / LANE_COUNT as f32;

    El::new(Kind::Custom("graph_cell"))
        .width(Size::Fixed(GRAPH_WIDTH))
        .height(Size::Fixed(ROW_HEIGHT))
        .shader(
            ShaderBinding::custom("commit_node")
                .color("vec_a", tokens::BACKGROUND)
                .color("vec_b", ring_color)
                .vec4("vec_c", [radius, ring_w, line_w, lane_frac]),
        )
        .fill(lane_color)
}

fn build_row(c: &FakeCommit, idx: usize, selected: bool) -> El {
    row([
        graph_cell(c.lane, selected),
        text(c.sha).mono().muted(),
        text(c.subject),
        spacer(),
        text(format!("{} · {}", c.author, c.when)).muted(),
    ])
    .key(format!("commit-{idx}"))
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(tokens::SPACE_2, 0.0))
    .height(Size::Fixed(ROW_HEIGHT))
    .align(Align::Center)
}

fn fixture() -> El {
    #[rustfmt::skip]
    let commits = [
        FakeCommit { sha: "8a3f1c9", subject: "fix race condition in scheduler", author: "ada",     when: "12m", lane: 0 },
        FakeCommit { sha: "1b07d4e", subject: "tweak token tooltip wording",     author: "linus",   when: "1h",  lane: 0 },
        FakeCommit { sha: "9f2e4a1", subject: "wire avatar fallback identicon",  author: "joelle",  when: "3h",  lane: 1 },
        FakeCommit { sha: "44ab8d2", subject: "diff: word-level highlight pass", author: "raphael", when: "5h",  lane: 1 },
        FakeCommit { sha: "61c0fe7", subject: "ci: bump rust toolchain to 1.85", author: "mei",     when: "7h",  lane: 2 },
        FakeCommit { sha: "a90215b", subject: "switch logging to env_logger",    author: "isabel",  when: "1d",  lane: 2 },
        FakeCommit { sha: "0d7e3c4", subject: "drop unused commit_detail cache", author: "noor",    when: "1d",  lane: 1 },
        FakeCommit { sha: "33b2118", subject: "context-menu spacing pass",       author: "kira",    when: "2d",  lane: 3 },
    ];
    let selected_idx = 3;
    let rows = commits
        .iter()
        .enumerate()
        .map(|(i, c)| build_row(c, i, i == selected_idx))
        .collect::<Vec<_>>();

    column([
        h2("Custom-painted commit graph"),
        text("8 commits · custom shader paints lane line + circle node").muted(),
        column(rows).gap(0.0),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_2)
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();
    let viewport = Rect::new(0.0, 0.0, 900.0, 360.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "custom_paint")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
