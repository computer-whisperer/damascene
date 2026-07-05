//! Structure-viewer stress test.
//!
//! Reproduces the workload shape of a structure-inspection app (the
//! shard-viewer "Map" view): thousands of nested flow cards — op cards,
//! control frames with keyword bands and selector chips, var pills,
//! literal tags, gather bars, and tiny feed-arrow vectors — grouped into
//! file boxes, absolutely placed via the `stack().layout()` escape hatch,
//! wrapped in a single pan/zoom `viewport()`, under an edge-spline
//! overlay. The whole tree is rebuilt from scratch every frame, and in
//! `Measured` sizing mode the app additionally runs its own
//! `layout::intrinsic()` pass over every card and file box during build
//! (exactly what a real viewer does to feed a graph-layout engine).
//!
//! Use the diagnostics panel (or the per-frame stdout log) to see where
//! the pipeline spends its time as the primitive count grows.
//!
//! Run:
//!
//! ```text
//! cargo run --release -p damascene-examples --bin structure_stress -- \
//!     [--cards N] [--complexity shallow|medium|deep] \
//!     [--sizing measured|estimated] [--edges on|off]
//! ```

use damascene_core::layout::intrinsic;
use damascene_core::prelude::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const MIN_CARDS: usize = 1;
const MAX_CARDS: usize = 10_000;
const DEFAULT_CARDS: usize = 1_000;
const CARDS_PER_FILE: usize = 16;
const FILE_PAD: f32 = 14.0;
const FILE_TITLE_H: f32 = 18.0;
const CARD_GAP: f32 = 18.0;
const FILE_GAP: f32 = 28.0;
static LAST_LOGGED_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);

// ---------------------------------------------------------------------------
// Synthetic structure model. Generated once per configuration (like a viewer
// parsing a corpus once); only the El rendering reruns per frame.

enum Node {
    Op {
        head: String,
        inline: String,
        args: Vec<Node>,
    },
    Frame {
        keyword: &'static str,
        detail: String,
        branches: Vec<(String, Node)>,
    },
    List(Vec<Node>),
    Var(String),
    Lit(String),
}

struct CardSpec {
    title: String,
    body: Node,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Complexity {
    Shallow,
    Medium,
    Deep,
}

impl Complexity {
    fn depth(self) -> u32 {
        match self {
            Complexity::Shallow => 2,
            Complexity::Medium => 3,
            Complexity::Deep => 4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Complexity::Shallow => "shallow",
            Complexity::Medium => "medium",
            Complexity::Deep => "deep",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sizing {
    /// Build each card, measure it with `layout::intrinsic()`, and pack by
    /// the measured size — the exact-measurement strategy (and its cost).
    Measured,
    /// Estimate card sizes from a cheap structural walk; never measure.
    Estimated,
}

impl Sizing {
    fn label(self) -> &'static str {
        match self {
            Sizing::Measured => "measured",
            Sizing::Estimated => "estimated",
        }
    }
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

const HEADS: [&str; 24] = [
    "resolve",
    "unify",
    "lower",
    "widen",
    "narrow",
    "fold_expr",
    "hoist",
    "prove",
    "rewrite",
    "subst",
    "occurs",
    "apply",
    "normalize",
    "reduce",
    "infer",
    "check",
    "elab",
    "merge",
    "split_at",
    "collect",
    "emit",
    "route",
    "measure",
    "place",
];
const VARS: [&str; 18] = [
    "env", "ctx", "acc", "goal", "subst", "lhs", "rhs", "head", "tail", "arg", "body", "scope",
    "trace", "depth", "seen", "out", "bind", "k",
];
const LITS: [&str; 10] = [
    "0", "1", "2", "[]", "'a'", "true", "false", "nil", "\"ok\"", "42",
];

fn head_name(r: u64) -> String {
    let base = HEADS[(r % HEADS.len() as u64) as usize];
    // A third of the heads get a numeric suffix so the corpus has thousands
    // of distinct strings, the way real fn/var names do.
    if r >> 8 & 3 == 0 {
        format!("{base}_{}", r >> 16 & 0x1FF)
    } else {
        base.to_string()
    }
}

fn leaf(r: u64) -> Node {
    if r & 1 == 0 {
        Node::Var(VARS[(r >> 1) as usize % VARS.len()].to_string())
    } else {
        Node::Lit(LITS[(r >> 1) as usize % LITS.len()].to_string())
    }
}

fn gen_node(seed: u64, depth: u32) -> Node {
    let r = mix(seed);
    if depth == 0 {
        return leaf(r);
    }
    match r % 10 {
        0..=4 => {
            let n_args = 1 + (r >> 16) as usize % 2;
            Node::Op {
                head: head_name(r),
                inline: if r >> 24 & 3 == 0 {
                    format!("k={}", r >> 32 & 0x3F)
                } else {
                    String::new()
                },
                args: (0..n_args)
                    .map(|i| gen_node(seed.wrapping_mul(31).wrapping_add(i as u64 + 1), depth - 1))
                    .collect(),
            }
        }
        5..=6 => {
            let (keyword, labels): (&str, &[&str]) = if r >> 12 & 1 == 0 {
                ("match", &["Cons h t", "Nil", "Leaf v"])
            } else {
                ("if", &["then", "else"])
            };
            let n_branches = 2 + (r >> 16) as usize % 2;
            Node::Frame {
                keyword,
                detail: format!("scrutinee_{}", r >> 20 & 0xFF),
                branches: (0..n_branches.min(labels.len()))
                    .map(|i| {
                        (
                            labels[i].to_string(),
                            gen_node(seed.wrapping_mul(37).wrapping_add(i as u64 + 5), depth - 1),
                        )
                    })
                    .collect(),
            }
        }
        7 => Node::List(
            (0..2 + (r >> 16) as usize % 2)
                .map(|i| gen_node(seed.wrapping_mul(41).wrapping_add(i as u64 + 9), depth - 1))
                .collect(),
        ),
        _ => leaf(r),
    }
}

fn gen_cards(count: usize, complexity: Complexity) -> Vec<CardSpec> {
    (0..count)
        .map(|i| CardSpec {
            title: format!("{}_{i}", HEADS[i % HEADS.len()]),
            body: gen_node(i as u64 + 1, complexity.depth()),
        })
        .collect()
}

fn node_stats(n: &Node) -> (usize, usize) {
    match n {
        Node::Op { args, .. } => args.iter().fold((1, 1), |(l, d), a| {
            let (al, ad) = node_stats(a);
            (l + al, d.max(ad + 1))
        }),
        Node::Frame { branches, .. } => branches.iter().fold((1, 1), |(l, d), (_, b)| {
            let (bl, bd) = node_stats(b);
            (l + bl, d.max(bd + 1))
        }),
        Node::List(elems) => elems.iter().fold((1, 1), |(l, d), e| {
            let (el, ed) = node_stats(e);
            (l + el, d.max(ed + 1))
        }),
        Node::Var(_) | Node::Lit(_) => (1, 1),
    }
}

// ---------------------------------------------------------------------------
// Card rendering — the flow-card vocabulary of a structure viewer. Every
// helper counts the Els it creates so the diagnostics can report the real
// tree size.

fn render_node(n: &Node, cnt: &mut usize) -> El {
    match n {
        Node::Op { head, inline, args } => render_op(head, inline, args, cnt),
        Node::Frame {
            keyword,
            detail,
            branches,
        } => render_frame(keyword, detail, branches, cnt),
        Node::List(elems) => render_list(elems, cnt),
        Node::Var(name) => var_pill(name, cnt),
        Node::Lit(value) => lit_tag(value, cnt),
    }
}

fn render_op(head: &str, inline: &str, args: &[Node], cnt: &mut usize) -> El {
    let card = op_card(head, inline, cnt);
    if args.is_empty() {
        return card;
    }
    let inputs = column(args.iter().map(|a| render_node(a, cnt)).collect::<Vec<_>>()).gap(6.0);
    let gathered = row([inputs, gather_bar(cnt)])
        .gap(tokens::SPACE_2)
        .align(Align::Stretch);
    *cnt += 3;
    row([gathered, feed_arrow(cnt), card])
        .gap(tokens::SPACE_1)
        .align(Align::Center)
}

fn op_card(head: &str, inline: &str, cnt: &mut usize) -> El {
    let mut kids = vec![
        text(head.to_string())
            .mono()
            .semibold()
            .font_size(13.0)
            .nowrap_text()
            .ellipsis(),
    ];
    if !inline.is_empty() {
        kids.push(
            text(inline.to_string())
                .mono()
                .muted()
                .font_size(11.0)
                .nowrap_text()
                .ellipsis(),
        );
    }
    *cnt += kids.len() + 1;
    column(kids)
        .gap(1.0)
        .padding(6.0)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .radius(6.0)
}

fn gather_bar(cnt: &mut usize) -> El {
    *cnt += 1;
    column(Vec::<El>::new())
        .width(Size::Fixed(2.5))
        .height(Size::Fill(1.0))
        .fill(tokens::INFO)
        .radius(2.0)
}

fn feed_arrow(cnt: &mut usize) -> El {
    *cnt += 1;
    let line = PathBuilder::new()
        .move_to(0.0, 6.0)
        .line_to(11.0, 6.0)
        .stroke_solid(tokens::INFO, 1.6)
        .build();
    let head = arrowhead(4.0, 6.0, 17.0, 6.0, tokens::INFO);
    vector(VectorAsset::from_paths(
        [0.0, 0.0, 18.0, 12.0],
        vec![line, head],
    ))
    .width(Size::Fixed(18.0))
    .height(Size::Fixed(12.0))
}

fn render_frame(keyword: &str, detail: &str, branches: &[(String, Node)], cnt: &mut usize) -> El {
    let band = row([
        text(keyword.to_string())
            .mono()
            .semibold()
            .font_size(12.0)
            .text_color(tokens::INFO_FOREGROUND)
            .nowrap_text(),
        text(detail.to_string())
            .mono()
            .font_size(11.0)
            .text_color(tokens::INFO_FOREGROUND)
            .nowrap_text()
            .ellipsis(),
    ])
    .gap(tokens::SPACE_2)
    .padding(5.0)
    .width(Size::Fill(1.0))
    .fill(tokens::INFO);
    let body = column(
        branches
            .iter()
            .map(|(label, region)| {
                *cnt += 3;
                row([selector_chip(label, cnt), render_node(region, cnt)])
                    .gap(tokens::SPACE_2)
                    .align(Align::Start)
            })
            .collect::<Vec<_>>(),
    )
    .gap(7.0)
    .padding(8.0);
    *cnt += 4;
    column([band, body])
        .fill(tokens::CARD)
        .stroke(tokens::INFO)
        .radius(7.0)
}

fn selector_chip(label: &str, cnt: &mut usize) -> El {
    *cnt += 2;
    row([text(label.to_string())
        .mono()
        .semibold()
        .font_size(10.0)
        .text_color(tokens::INFO_FOREGROUND)
        .nowrap_text()
        .ellipsis()])
    .padding(3.0)
    .radius(5.0)
    .fill(tokens::INFO)
}

fn render_list(elems: &[Node], cnt: &mut usize) -> El {
    let header = text(format!("list · {}", elems.len()))
        .mono()
        .muted()
        .font_size(10.0)
        .nowrap_text();
    let body = column(
        elems
            .iter()
            .map(|e| render_node(e, cnt))
            .collect::<Vec<_>>(),
    )
    .gap(5.0)
    .padding(6.0);
    let bar = column(Vec::<El>::new())
        .width(Size::Fixed(3.0))
        .height(Size::Fill(1.0))
        .fill(tokens::MUTED)
        .radius(2.0);
    *cnt += 5;
    column([header, row([bar, body]).align(Align::Stretch)])
        .gap(3.0)
        .padding(6.0)
        .fill(tokens::CARD)
        .stroke(tokens::MUTED)
        .radius(7.0)
}

fn var_pill(name: &str, cnt: &mut usize) -> El {
    *cnt += 2;
    column([text(name.to_string())
        .mono()
        .semibold()
        .font_size(12.0)
        .center_text()
        .nowrap_text()])
    .padding(5.0)
    .fill(tokens::CARD.mix(tokens::WARNING, 0.32))
    .stroke(tokens::WARNING)
    .radius(13.0)
}

fn lit_tag(value: &str, cnt: &mut usize) -> El {
    *cnt += 2;
    column([text(value.to_string())
        .mono()
        .muted()
        .font_size(11.0)
        .center_text()
        .nowrap_text()])
    .padding(4.0)
    .fill(tokens::BACKGROUND)
    .stroke(tokens::BORDER)
    .radius(4.0)
}

fn render_card(card: &CardSpec, index: usize, cnt: &mut usize) -> El {
    *cnt += 3;
    column([
        text(card.title.clone())
            .mono()
            .semibold()
            .font_size(13.0)
            .nowrap_text(),
        render_node(&card.body, cnt),
    ])
    .key(format!("card-{index}"))
    .gap(tokens::SPACE_2)
    .padding(8.0)
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
    .radius(8.0)
}

fn arrowhead(from_x: f32, from_y: f32, tip_x: f32, tip_y: f32, color: Color) -> VectorPath {
    let (dx, dy) = (tip_x - from_x, tip_y - from_y);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / len, dy / len);
    let (perp_x, perp_y) = (-uy, ux);
    const SIZE: f32 = 9.0;
    const HALF: f32 = 4.0;
    let bx = tip_x - ux * SIZE;
    let by = tip_y - uy * SIZE;
    PathBuilder::new()
        .move_to(tip_x, tip_y)
        .line_to(bx + perp_x * HALF, by + perp_y * HALF)
        .line_to(bx - perp_x * HALF, by - perp_y * HALF)
        .close()
        .fill_solid(color)
        .build()
}

// ---------------------------------------------------------------------------
// Packing: shelf-pack rects into rows against a target width. Used for the
// cards inside a file box and again for the file boxes on the canvas.

fn shelf_pack(sizes: &[(f32, f32)], target_w: f32, gap: f32) -> (Vec<(f32, f32)>, f32, f32) {
    let mut positions = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut row_h, mut max_w) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for &(w, h) in sizes {
        if x > 0.0 && x + w > target_w {
            max_w = max_w.max(x - gap);
            x = 0.0;
            y += row_h + gap;
            row_h = 0.0;
        }
        positions.push((x, y));
        x += w + gap;
        row_h = row_h.max(h);
    }
    max_w = max_w.max(x - gap);
    (positions, max_w.max(0.0), y + row_h)
}

// ---------------------------------------------------------------------------
// The app.

struct StructureStress {
    cards: Vec<CardSpec>,
    card_count: usize,
    complexity: Complexity,
    sizing: Sizing,
    edges: bool,
    pending: Vec<ViewportRequest>,
    frame: u64,
    fit_at_frame: Option<u64>,
    // Written during build (which is `&self`), displayed one frame later.
    last_el_count: Cell<usize>,
    last_measure: Cell<Duration>,
    last_canvas_build: Cell<Duration>,
}

impl StructureStress {
    fn new(card_count: usize, complexity: Complexity, sizing: Sizing, edges: bool) -> Self {
        Self {
            cards: gen_cards(card_count, complexity),
            card_count,
            complexity,
            sizing,
            edges,
            pending: Vec::new(),
            frame: 0,
            fit_at_frame: Some(2),
            last_el_count: Cell::new(0),
            last_measure: Cell::new(Duration::ZERO),
            last_canvas_build: Cell::new(Duration::ZERO),
        }
    }

    fn regen(&mut self) {
        self.cards = gen_cards(self.card_count, self.complexity);
        self.fit_at_frame = Some(self.frame + 2);
    }

    fn set_cards(&mut self, count: usize) {
        self.card_count = count.clamp(MIN_CARDS, MAX_CARDS);
        self.regen();
    }

    /// Build the whole canvas: render every card, size it (measured or
    /// estimated), pack cards into file boxes and file boxes onto the
    /// canvas, then wrap in the pan/zoom viewport under an edge overlay.
    fn canvas(&self) -> El {
        let t_canvas = Instant::now();
        let mut measure = Duration::ZERO;
        let mut el_count = 0usize;

        // Render + size every card.
        let mut rendered: Vec<(El, (f32, f32))> = Vec::with_capacity(self.cards.len());
        for (i, card) in self.cards.iter().enumerate() {
            let el = render_card(card, i, &mut el_count);
            let size = match self.sizing {
                Sizing::Measured => {
                    let t = Instant::now();
                    let size = intrinsic(&el);
                    measure += t.elapsed();
                    size
                }
                Sizing::Estimated => {
                    let (leaves, depth) = node_stats(&card.body);
                    (170.0 + 36.0 * depth as f32, 46.0 + 24.0 * leaves as f32)
                }
            };
            rendered.push((el, size));
        }

        // Pack cards into file boxes, collecting global card rects for edges.
        let mut file_els: Vec<(El, (f32, f32))> = Vec::new();
        let mut card_rects_per_file: Vec<Vec<(f32, f32, f32, f32)>> = Vec::new();
        let mut remaining = rendered.into_iter().peekable();
        let mut file_idx = 0usize;
        while remaining.peek().is_some() {
            let chunk: Vec<(El, (f32, f32))> = remaining.by_ref().take(CARDS_PER_FILE).collect();
            let sizes: Vec<(f32, f32)> = chunk.iter().map(|(_, s)| *s).collect();
            let row_target = sizes.iter().map(|s| s.0).sum::<f32>() / (sizes.len() as f32).sqrt();
            let (positions, w, h) = shelf_pack(&sizes, row_target.max(300.0), CARD_GAP);
            let (bw, bh) = (w + FILE_PAD * 2.0, h + FILE_PAD * 2.0 + FILE_TITLE_H);

            let mut children: Vec<El> = Vec::with_capacity(chunk.len() + 1);
            children.push(
                text(format!("mod_{file_idx:03}.shard"))
                    .mono()
                    .muted()
                    .font_size(11.0)
                    .nowrap_text(),
            );
            el_count += 2;

            let mut local_rects: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(chunk.len() + 1);
            local_rects.push((FILE_PAD, 4.0, bw - FILE_PAD * 2.0, FILE_TITLE_H));
            for (&(px, py), &(cw, ch)) in positions.iter().zip(&sizes) {
                local_rects.push((px + FILE_PAD, py + FILE_PAD + FILE_TITLE_H, cw, ch));
            }
            card_rects_per_file.push(local_rects[1..].to_vec());

            children.extend(chunk.into_iter().map(|(el, _)| el));

            let rects = local_rects.clone();
            let file_el = stack(children)
                .width(Size::Fixed(bw))
                .height(Size::Fixed(bh))
                .fill(tokens::BACKGROUND.mix(tokens::CARD, 0.5))
                .stroke(tokens::BORDER)
                .radius(10.0)
                .layout(move |ctx: LayoutCtx| {
                    let o = ctx.container;
                    rects
                        .iter()
                        .map(|&(x, y, w, h)| Rect::new(o.x + x, o.y + y, w, h))
                        .collect()
                });
            if self.sizing == Sizing::Measured {
                // Faithful to the real viewer: enclosing boxes are re-measured
                // bottom-up at every nesting level.
                let t = Instant::now();
                let _ = intrinsic(&file_el);
                measure += t.elapsed();
            }
            file_els.push((file_el, (bw, bh)));
            file_idx += 1;
        }

        // Pack file boxes onto the canvas.
        let file_sizes: Vec<(f32, f32)> = file_els.iter().map(|(_, s)| *s).collect();
        let total_area: f32 = file_sizes.iter().map(|(w, h)| w * h).sum();
        let canvas_target = (total_area * 1.6).sqrt();
        let (file_positions, cw, ch) = shelf_pack(&file_sizes, canvas_target, FILE_GAP);

        // Edge overlay: one spline+arrowhead between consecutive cards in
        // each file, in canvas coordinates.
        let edges = if self.edges {
            let mut paths = Vec::new();
            for (file_idx, local) in card_rects_per_file.iter().enumerate() {
                let (fx, fy) = file_positions[file_idx];
                for pair in local.windows(2) {
                    let (ax, ay, aw, ah) = pair[0];
                    let (bx, by, _bw, bh) = pair[1];
                    let (x1, y1) = (fx + ax + aw, fy + ay + ah * 0.5);
                    let (x2, y2) = (fx + bx, fy + by + bh * 0.5);
                    let tangent = (x2 - x1).abs().max(40.0) * 0.5;
                    let color = if pair[0].1 < pair[1].1 {
                        tokens::MUTED_FOREGROUND
                    } else {
                        tokens::ACCENT
                    };
                    paths.push(
                        PathBuilder::new()
                            .move_to(x1, y1)
                            .cubic_to(x1 + tangent, y1, x2 - tangent, y2, x2, y2)
                            .stroke_solid(color, 1.5)
                            .build(),
                    );
                    paths.push(arrowhead(x2 - tangent, y2, x2, y2, color));
                }
            }
            VectorAsset::from_paths([0.0, 0.0, cw, ch], paths)
        } else {
            VectorAsset::from_paths([0.0, 0.0, cw, ch], Vec::new())
        };

        let mut children: Vec<El> = Vec::with_capacity(file_els.len() + 1);
        children.push(vector(edges));
        el_count += 2;
        children.extend(file_els.into_iter().map(|(el, _)| el));

        let positions = file_positions;
        let sizes = file_sizes;
        let canvas = stack(children)
            .width(Size::Fixed(cw))
            .height(Size::Fixed(ch))
            .layout(move |ctx: LayoutCtx| {
                let o = ctx.container;
                let mut rects = Vec::with_capacity(positions.len() + 1);
                rects.push(Rect::new(o.x, o.y, cw, ch));
                for (&(x, y), &(w, h)) in positions.iter().zip(&sizes) {
                    rects.push(Rect::new(o.x + x, o.y + y, w, h));
                }
                rects
            });

        self.last_el_count.set(el_count);
        self.last_measure.set(measure);
        self.last_canvas_build.set(t_canvas.elapsed());

        viewport([canvas])
            .key("canvas")
            .min_zoom(0.02)
            .max_zoom(3.0)
            .pan_bounds(PanBounds::Center)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
    }
}

impl App for StructureStress {
    fn build(&self, cx: &BuildCx) -> El {
        let zoom = cx.viewport_view("canvas").map_or(1.0, |v| v.zoom);
        let canvas = self.canvas();
        column([
            toolbar([
                toolbar_group([
                    toolbar_title("Structure stress"),
                    toolbar_description(format!(
                        "{} cards / {} sizing / {} complexity / {} Els last frame",
                        self.card_count,
                        self.sizing.label(),
                        self.complexity.label(),
                        self.last_el_count.get(),
                    )),
                ]),
                spacer(),
                badge(format!("{} cards", self.card_count)).info(),
                control_button("Fit", "fit"),
                control_button("Reset", "reset"),
            ]),
            row([
                preset_button("250", self.card_count == 250, "cards:250"),
                preset_button("1k", self.card_count == 1_000, "cards:1000"),
                preset_button("2.5k", self.card_count == 2_500, "cards:2500"),
                preset_button("5k", self.card_count == 5_000, "cards:5000"),
                spacer(),
                preset_button(
                    "Shallow",
                    self.complexity == Complexity::Shallow,
                    "complexity:shallow",
                ),
                preset_button(
                    "Medium",
                    self.complexity == Complexity::Medium,
                    "complexity:medium",
                ),
                preset_button(
                    "Deep",
                    self.complexity == Complexity::Deep,
                    "complexity:deep",
                ),
                spacer(),
                preset_button(
                    "Measured",
                    self.sizing == Sizing::Measured,
                    "sizing:measured",
                ),
                preset_button(
                    "Estimated",
                    self.sizing == Sizing::Estimated,
                    "sizing:estimated",
                ),
                spacer(),
                preset_button("Edges", self.edges, "edges:toggle"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            diagnostics_panel(self, cx.diagnostics(), zoom),
            canvas,
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4)
        .height(Size::Fill(1.0))
    }

    fn before_build(&mut self) {
        self.frame += 1;
        if let Some(at) = self.fit_at_frame
            && self.frame >= at
        {
            self.fit_at_frame = None;
            self.pending.push(ViewportRequest::FitContent {
                key: "canvas".into(),
                padding: 24.0,
            });
        }
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
            return;
        }
        match event.route() {
            Some("cards:250") => self.set_cards(250),
            Some("cards:1000") => self.set_cards(1_000),
            Some("cards:2500") => self.set_cards(2_500),
            Some("cards:5000") => self.set_cards(5_000),
            Some("complexity:shallow") => {
                self.complexity = Complexity::Shallow;
                self.regen();
            }
            Some("complexity:medium") => {
                self.complexity = Complexity::Medium;
                self.regen();
            }
            Some("complexity:deep") => {
                self.complexity = Complexity::Deep;
                self.regen();
            }
            Some("sizing:measured") => self.sizing = Sizing::Measured,
            Some("sizing:estimated") => self.sizing = Sizing::Estimated,
            Some("edges:toggle") => self.edges = !self.edges,
            Some("fit") => self.pending.push(ViewportRequest::FitContent {
                key: "canvas".into(),
                padding: 24.0,
            }),
            Some("reset") => self.pending.push(ViewportRequest::ResetView {
                key: "canvas".into(),
            }),
            _ => {}
        }
    }

    fn drain_viewport_requests(&mut self) -> Vec<ViewportRequest> {
        std::mem::take(&mut self.pending)
    }
}

// ---------------------------------------------------------------------------
// Diagnostics: on-screen panel plus a per-frame stdout log line, so headless
// capture (`... | tee log`) works without reading the window.

fn diagnostics_panel(app: &StructureStress, diag: Option<&HostDiagnostics>, zoom: f32) -> El {
    let Some(diag) = diag else {
        return text("Host diagnostics unavailable").caption().muted();
    };
    log_diagnostics(app, diag, zoom);
    let cpu_total = diag.last_build + diag.last_prepare + diag.last_submit;
    column([
        row([
            metric("frame", format!("#{}", diag.frame_index)),
            metric("dt", format_dt(diag.last_frame_dt)),
            metric("cpu", format_duration(cpu_total)),
            metric("trigger", diag.trigger.label().to_string()),
            metric("zoom", format!("{:.3}", zoom)),
            metric("els", compact_count(app.last_el_count.get() as u64)),
            metric("app measure", format_duration(app.last_measure.get())),
            metric("app canvas", format_duration(app.last_canvas_build.get())),
        ])
        .gap(tokens::SPACE_3),
        row([
            metric("build", format_duration(diag.last_build)),
            metric("layout", format_duration(diag.last_layout)),
            metric(
                "intr hit",
                compact_count(diag.last_layout_intrinsic_cache_hits),
            ),
            metric(
                "intr miss",
                compact_count(diag.last_layout_intrinsic_cache_misses),
            ),
            metric("draw_ops", format_duration(diag.last_draw_ops)),
            metric("paint", format_duration(diag.last_paint)),
            metric("culled", compact_count(diag.last_paint_culled_ops)),
            metric("gpu", format_duration(diag.last_gpu_upload)),
            metric("snapshot", format_duration(diag.last_snapshot)),
            metric("submit", format_duration(diag.last_submit)),
        ])
        .gap(tokens::SPACE_3),
        row([
            metric("shape hit", compact_count(diag.last_text_layout_cache_hits)),
            metric(
                "shape miss",
                compact_count(diag.last_text_layout_cache_misses),
            ),
            metric(
                "shape evict",
                compact_count(diag.last_text_layout_cache_evictions),
            ),
            metric("shaped", format_bytes(diag.last_text_layout_shaped_bytes)),
        ])
        .gap(tokens::SPACE_3),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .fill(tokens::MUTED)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_SM)
}

fn log_diagnostics(app: &StructureStress, diag: &HostDiagnostics, zoom: f32) {
    if diag.frame_index == 0 {
        return;
    }
    if LAST_LOGGED_FRAME.swap(diag.frame_index, Ordering::Relaxed) == diag.frame_index {
        return;
    }
    println!(
        "structure_stress frame={} trigger={} cards={} sizing={} complexity={} edges={} zoom={zoom:.4} els={} dt={} app_measure={} app_canvas={} build={} prepare={} layout={} intrinsic_hits={} intrinsic_misses={} draw_ops={} paint={} paint_culled={} gpu={} snapshot={} submit={} shape_hits={} shape_misses={} shape_evictions={} shaped_bytes={}",
        diag.frame_index,
        diag.trigger.label(),
        app.card_count,
        app.sizing.label(),
        app.complexity.label(),
        app.edges,
        app.last_el_count.get(),
        format_dt(diag.last_frame_dt),
        format_duration(app.last_measure.get()),
        format_duration(app.last_canvas_build.get()),
        format_duration(diag.last_build),
        format_duration(diag.last_prepare),
        format_duration(diag.last_layout),
        diag.last_layout_intrinsic_cache_hits,
        diag.last_layout_intrinsic_cache_misses,
        format_duration(diag.last_draw_ops),
        format_duration(diag.last_paint),
        diag.last_paint_culled_ops,
        format_duration(diag.last_gpu_upload),
        format_duration(diag.last_snapshot),
        format_duration(diag.last_submit),
        diag.last_text_layout_cache_hits,
        diag.last_text_layout_cache_misses,
        diag.last_text_layout_cache_evictions,
        diag.last_text_layout_shaped_bytes,
    );
}

fn control_button(label: &str, key: &str) -> El {
    button(label)
        .key(key)
        .ghost()
        .height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

fn preset_button(label: &str, active: bool, key: &str) -> El {
    let button = button(label)
        .key(key)
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));
    if active {
        button.primary()
    } else {
        button.secondary()
    }
}

fn metric(label: &str, value: String) -> El {
    column([mono(label).caption().muted(), mono(value).small()])
        .gap(1.0)
        .width(Size::Hug)
}

fn format_dt(dt: Duration) -> String {
    if dt.is_zero() {
        return "-".to_string();
    }
    let ms = dt.as_secs_f64() * 1000.0;
    format!("{ms:.1}ms/{:.1}fps", 1000.0 / ms)
}

fn format_duration(duration: Duration) -> String {
    if duration.is_zero() {
        return "-".to_string();
    }
    format!("{:.1}ms", duration.as_secs_f64() * 1000.0)
}

fn compact_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}m", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(feature = "profiling")]
fn install_profiling() -> Result<Option<tracing_chrome::FlushGuard>, Box<dyn std::error::Error>> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let mut args = std::env::args().skip(1);
    let mut output: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => {
                output = Some(args.next().ok_or("--profile expects a path argument")?);
            }
            other if other.starts_with("--profile=") => {
                output = Some(other.trim_start_matches("--profile=").to_string());
            }
            _ => {}
        }
    }
    let Some(path) = output else {
        return Ok(None);
    };

    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file(&path)
        .include_args(true)
        .build();
    tracing_subscriber::registry().with(chrome_layer).init();
    eprintln!(
        "structure_stress: tracing chrome JSON → {path} (load in chrome://tracing or perfetto)"
    );
    Ok(Some(guard))
}

#[cfg(not(feature = "profiling"))]
fn install_profiling() -> Result<Option<()>, Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--profile" || a.starts_with("--profile=")) {
        return Err(
            "--profile passed but the binary was built without `--features profiling`. \
             Rebuild with `cargo run --release -p damascene-examples --bin structure_stress \
             --features profiling -- --profile out.json`."
                .into(),
        );
    }
    Ok(None)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _profile_guard = install_profiling()?;
    let mut card_count = DEFAULT_CARDS;
    let mut complexity = Complexity::Medium;
    let mut sizing = Sizing::Measured;
    let mut edges = true;
    let mut view_fit = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cards" => {
                card_count = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DEFAULT_CARDS)
            }
            "--complexity" => {
                complexity = match args.next().as_deref() {
                    Some("shallow") => Complexity::Shallow,
                    Some("deep") => Complexity::Deep,
                    _ => Complexity::Medium,
                }
            }
            "--sizing" => {
                sizing = match args.next().as_deref() {
                    Some("estimated") => Sizing::Estimated,
                    _ => Sizing::Measured,
                }
            }
            "--edges" => edges = args.next().as_deref() != Some("off"),
            // `--view topleft` skips the startup FitContent, leaving the
            // viewport at 1:1 on the canvas origin — most content offscreen,
            // for measuring how much the pipeline saves via culling.
            "--view" => view_fit = args.next().as_deref() != Some("topleft"),
            // Consumed by `install_profiling` before we get here.
            "--profile" => {
                let _ = args.next();
            }
            other if other.starts_with("--profile=") => {}
            other => eprintln!("structure_stress: ignoring unknown arg {other}"),
        }
    }

    let mut app = StructureStress::new(card_count, complexity, sizing, edges);
    if !view_fit {
        app.fit_at_frame = None;
    }

    let viewport_rect = Rect::new(0.0, 0.0, 1600.0, 1000.0);
    // A short periodic interval keeps the host on the full rebuild path
    // (Periodic frames rebuild + relayout), so steady-state logs measure the
    // whole pipeline without needing continuous user input.
    let config =
        damascene_winit_wgpu::HostConfig::default().with_redraw_interval(Duration::from_millis(10));
    damascene_winit_wgpu::run_with_config(
        "Damascene — structure stress",
        viewport_rect,
        app,
        config,
    )
}
