//! Custom-paint commit graph — proof that an app can render its own
//! geometry through damascene's paint stream without a parallel pipeline.
//!
//! What this exercises (the four host-paint affordances a git-client
//! port needs to be confident about):
//!
//! - **Paint-stream emission via custom shader.** A WGSL shader bound at
//!   El level via `ShaderBinding::custom("commit_node")` paints a
//!   per-row commit graph cell — vertical lane line + circle node —
//!   while picking up MSAA / scissor / z-order from the runner for
//!   free.
//! - **Virtualized scroll.** `virtual_list` realizes only the visible
//!   commits and routes scroll through damascene; the app reads no scroll
//!   state directly.
//! - **Hit-test routing.** Click any row → `UiEventKind::Click` with
//!   `route() == Some("commit-{i}")`. The app does its own commit-id
//!   lookup; damascene's routing handed it the row index for free.
//! - **Overlay z-order.** Right-click a row → an damascene `context_menu`
//!   pops above the custom-painted cell; tooltips and modals would
//!   layer the same way.
//!
//! Run: `cargo run -p damascene-examples --bin custom_paint`

use std::sync::Arc;

use damascene_core::prelude::*;

const ROW_HEIGHT: f32 = 28.0;
const GRAPH_WIDTH: f32 = 140.0;
const LANE_COUNT: u8 = 4;
const COMMIT_COUNT: usize = 2_000;

/// commit_node.wgsl — paints one row's commit-graph cell:
///   - a vertical "lane line" running through the row at the commit's lane,
///   - a filled circle node at the row's vertical center,
///   - a ring around the circle (thicker / brighter when selected).
///
/// Per-instance uniforms (encoded into the QuadInstance generic slots):
///
///   vec_a (location 2): node fill color (rgba 0..1)
///   vec_b (location 3): line + ring color (rgba 0..1)
///   vec_c (location 4): packed params:
///     .x = node radius (logical px)
///     .y = ring stroke width (logical px)
///     .z = lane line width (logical px)
///     .w = lane fraction (0..1) — horizontal position of the lane
///          inside the cell, so one shader handles any lane index.
const COMMIT_NODE_WGSL: &str = r#"
struct FrameUniforms { viewport: vec2<f32>, _pad: vec2<f32>, };
@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput  { @location(0) corner_uv: vec2<f32>, };
struct InstanceInput {
    @location(1) rect:  vec4<f32>,
    @location(2) vec_a: vec4<f32>,
    @location(3) vec_b: vec4<f32>,
    @location(4) vec_c: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) @interpolate(perspective, sample) local_px: vec2<f32>,
    @location(1) size:   vec2<f32>,
    @location(2) fill:   vec4<f32>,
    @location(3) ring:   vec4<f32>,
    @location(4) params: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let pos_px = in.corner_uv * inst.rect.zw + inst.rect.xy;
    let clip = vec4<f32>(
        pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_px.y / frame.viewport.y * 2.0,
        0.0, 1.0,
    );
    var out: VertexOutput;
    out.clip_pos = clip;
    out.local_px = in.corner_uv * inst.rect.zw;
    out.size     = inst.rect.zw;
    out.fill     = inst.vec_a;
    out.ring     = inst.vec_b;
    out.params   = inst.vec_c;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius = in.params.x;
    let ring_w = in.params.y;
    let line_w = in.params.z;
    let lane_x = in.params.w * in.size.x;
    let row_y  = in.size.y * 0.5;

    // Circle SDF — d < 0 inside the disc, d > 0 outside.
    let p   = in.local_px - vec2<f32>(lane_x, row_y);
    let d   = length(p) - radius;
    let aa  = max(fwidth(d), 0.5);
    let outer = 1.0 - smoothstep(0.0, aa, d);                      // inside outer edge
    let inner = 1.0 - smoothstep(0.0, aa, d + ring_w);             // inside ring's inner edge
    let ring_a = clamp(outer - inner, 0.0, 1.0);                   // donut
    let body_a = inner;

    // Lane line — masked out wherever the disc covers it.
    let dx     = abs(in.local_px.x - lane_x);
    let aa_l   = max(fwidth(dx), 0.5);
    let line_a = (1.0 - smoothstep(line_w * 0.5 - aa_l,
                                    line_w * 0.5 + aa_l, dx))
                 * (1.0 - outer);

    // Premultiplied additive composite — body / ring / line are
    // non-overlapping by construction, so straight summation produces
    // the correct over-blend output.
    let line_pm = vec4<f32>(in.ring.rgb * (in.ring.a * line_a), in.ring.a * line_a);
    let ring_pm = vec4<f32>(in.ring.rgb * (in.ring.a * ring_a), in.ring.a * ring_a);
    let body_pm = vec4<f32>(in.fill.rgb * (in.fill.a * body_a), in.fill.a * body_a);
    let pm = line_pm + ring_pm + body_pm;
    let a  = clamp(pm.a, 0.0, 1.0);
    if (a <= 0.0) { return vec4<f32>(0.0); }
    return vec4<f32>(pm.rgb / a, a);
}
"#;

/// One fake commit. The lane sequence is precomputed deterministically
/// so the demo doesn't need a real DAG — it just exercises the paint
/// path with believable lane variety.
#[derive(Clone)]
struct FakeCommit {
    sha: String,
    subject: String,
    author: String,
    when: String,
    lane: u8,
}

fn lane_palette(lane: u8) -> Color {
    match lane % LANE_COUNT {
        0 => Color::srgb_u8(96, 165, 230),  // blue
        1 => Color::srgb_u8(96, 200, 200),  // teal
        2 => Color::srgb_u8(140, 200, 110), // green
        _ => Color::srgb_u8(230, 180, 90),  // amber
    }
}

fn make_commits(n: usize) -> Vec<FakeCommit> {
    // Cheap deterministic 32-bit PRNG (xorshift) — no extra deps.
    let mut s: u32 = 0xC0FFEE;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };

    const SUBJECTS: &[&str] = &[
        "fix race condition in scheduler",
        "tweak token tooltip wording",
        "wire avatar fallback identicon",
        "diff: word-level highlight cleanup",
        "ci: bump rust toolchain to 1.85",
        "switch logging to env_logger",
        "drop unused commit_detail::heightcache",
        "stash: persist active stash across reload",
        "context-menu spacing pass",
        "ux: hover tint on file-list rows",
        "render: avoid redundant vertex flush",
        "graph: collapse degenerate fork lanes",
    ];
    const AUTHORS: &[&str] = &[
        "ada", "linus", "joelle", "raphael", "mei", "isabel", "noor", "kira",
    ];

    let mut lane: u8 = 0;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let r = next();
        // Drift lanes with a small probability to mimic a busy graph.
        if r % 7 == 0 {
            lane = (lane + 1) % LANE_COUNT;
        } else if r % 11 == 0 && lane > 0 {
            lane -= 1;
        }
        let subj = SUBJECTS[(r as usize) % SUBJECTS.len()];
        let auth = AUTHORS[(next() as usize) % AUTHORS.len()];
        let mins = (next() % 720) as i64;
        let when = if mins < 60 {
            format!("{}m", mins.max(1))
        } else if mins < 60 * 24 {
            format!("{}h", mins / 60)
        } else {
            format!("{}d", mins / (60 * 24))
        };
        let sha: u32 = next();
        out.push(FakeCommit {
            sha: format!("{:07x}", sha),
            subject: format!("{} (#{i})", subj),
            author: auth.to_string(),
            when,
            lane,
        });
    }
    out
}

fn graph_cell(commit: &FakeCommit, selected: bool) -> El {
    let lane_color = lane_palette(commit.lane);
    let bg = tokens::BACKGROUND;
    // Selection: thicken the ring and tint it bright.
    let ring_color = if selected {
        Color::srgb_u8(245, 245, 250)
    } else {
        lane_color
    };
    let ring_w = if selected { 2.5 } else { 1.5 };
    let radius = 5.0;
    let line_w = 2.0;
    let lane_frac = (commit.lane as f32 + 0.5) / LANE_COUNT as f32;

    El::new(Kind::Custom("graph_cell"))
        .width(Size::Fixed(GRAPH_WIDTH))
        .height(Size::Fixed(ROW_HEIGHT))
        .shader(
            ShaderBinding::custom("commit_node")
                .color("vec_a", bg) // fill: app bg, so the node "punches through" the line
                .color("vec_b", ring_color)
                .vec4("vec_c", [radius, ring_w, line_w, lane_frac]),
        )
        // Tag the cell with the lane palette so the bundle still has
        // useful color info even though the shader paints the ring.
        .fill(lane_color)
}

fn build_row(commit: &FakeCommit, idx: usize, selected: bool) -> El {
    let row_bg = if selected {
        tokens::ACCENT
    } else {
        Color::srgb_u8a(0, 0, 0, 0)
    };
    row([
        graph_cell(commit, selected),
        text(commit.sha.clone()).mono().muted(),
        text(commit.subject.clone()),
        spacer(),
        text(format!("{} · {}", commit.author, commit.when)).muted(),
    ])
    .key(format!("commit-{idx}"))
    .focusable()
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(tokens::SPACE_2, 0.0))
    .height(Size::Fixed(ROW_HEIGHT))
    .align(Align::Center)
    .fill(row_bg)
}

const CTX_ACTIONS: &[&str] = &["Copy SHA", "Checkout", "Cherry-pick", "Revert"];

struct Demo {
    commits: Arc<Vec<FakeCommit>>,
    selected: Option<usize>,
    last_action: Option<String>,
    context_open: bool,
    context_pos: (f32, f32),
    context_idx: Option<usize>,
}

impl Demo {
    fn new() -> Self {
        Self {
            commits: Arc::new(make_commits(COMMIT_COUNT)),
            selected: None,
            last_action: None,
            context_open: false,
            context_pos: (0.0, 0.0),
            context_idx: None,
        }
    }

    fn close_context(&mut self) {
        self.context_open = false;
        self.context_idx = None;
    }
}

impl App for Demo {
    fn build(&self, _cx: &BuildCx) -> El {
        let commits = Arc::clone(&self.commits);
        let selected = self.selected;
        let header_text = match (self.selected, &self.last_action) {
            (_, Some(a)) => a.clone(),
            (Some(i), _) => {
                let c = &self.commits[i];
                format!("selected {} · lane {} · {}", c.sha, c.lane, c.subject)
            }
            _ => format!(
                "{COMMIT_COUNT} commits · scroll with the wheel · click selects · right-click for actions"
            ),
        };

        let main = column([
            h2("Custom-painted commit graph"),
            text(header_text).muted(),
            virtual_list(commits.len(), ROW_HEIGHT, move |i| {
                build_row(&commits[i], i, selected == Some(i))
            })
            .key("commits")
            .height(Size::Fill(1.0)),
        ])
        .padding(tokens::SPACE_4)
        .gap(tokens::SPACE_2);

        overlays(
            main,
            [self.context_open.then(|| {
                context_menu(
                    "ctx-menu",
                    self.context_pos,
                    CTX_ACTIONS
                        .iter()
                        .map(|a| menu_item(*a).key(format!("ctx:{a}"))),
                )
            })],
        )
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        // Escape dismisses any open menu.
        if matches!(event.kind, UiEventKind::Escape) {
            self.close_context();
            return;
        }

        // Outside-click dismisses the context menu.
        if matches!(event.kind, UiEventKind::Click) && event.route() == Some("ctx-menu:dismiss") {
            self.close_context();
            return;
        }

        // Context-menu item selection.
        if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(key) = event.route()
            && let Some(action) = key.strip_prefix("ctx:")
        {
            if let Some(i) = self.context_idx {
                let c = &self.commits[i];
                self.last_action = Some(format!("{} → {} ({})", action, c.sha, c.subject));
            }
            self.close_context();
            return;
        }

        // Row interactions.
        let key = match event.route() {
            Some(k) => k,
            None => return,
        };
        let idx = match key
            .strip_prefix("commit-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            Some(i) => i,
            None => return,
        };

        match event.kind {
            UiEventKind::Click | UiEventKind::Activate => {
                self.selected = Some(idx);
                self.last_action = None;
                self.close_context();
            }
            UiEventKind::SecondaryClick => {
                if let Some(p) = event.pointer_pos() {
                    self.context_pos = p;
                    self.context_idx = Some(idx);
                    self.context_open = true;
                    self.selected = Some(idx);
                }
            }
            _ => {}
        }
    }

    fn shaders(&self) -> Vec<AppShader> {
        vec![AppShader {
            name: "commit_node",
            wgsl: COMMIT_NODE_WGSL,
            samples_backdrop: false,
            samples_time: false,
        }]
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 600.0);
    damascene_winit_wgpu::run(
        "Damascene — custom-paint commit graph",
        viewport,
        Demo::new(),
    )
}
