//! Conversation layout stress test.
//!
//! Non-virtualized scroll content shaped like chat / agent transcripts:
//! many message turns, each with metadata and wrapped message bodies.
//! Use this to find where text layout, wrapping, and cache churn become
//! visible without touching a real application.
//!
//! Run:
//!
//! ```text
//! cargo run -p damascene-examples --bin conversation_stress
//! ```

use damascene_core::prelude::*;
use damascene_markdown::md;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const MIN_TURNS: usize = 1;
const MAX_TURNS: usize = 5_000;
const DEFAULT_TURNS: usize = 120;
static LAST_LOGGED_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySize {
    Short,
    Medium,
    Long,
}

impl BodySize {
    fn paragraphs_per_turn(self) -> usize {
        match self {
            BodySize::Short => 2,
            BodySize::Medium => 5,
            BodySize::Long => 12,
        }
    }

    fn approx_words_per_turn(self) -> usize {
        // The generated paragraph is intentionally stable and prose-like:
        // enough words to exercise wrapping without allocating enormous
        // source constants.
        self.paragraphs_per_turn() * 54
    }

    fn label(self) -> &'static str {
        match self {
            BodySize::Short => "Short",
            BodySize::Medium => "Medium",
            BodySize::Long => "Long",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyMode {
    Plain,
    Markdown,
}

impl BodyMode {
    fn label(self) -> &'static str {
        match self {
            BodyMode::Plain => "plain",
            BodyMode::Markdown => "markdown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListMode {
    Scroll,
    Virtual,
}

impl ListMode {
    fn label(self) -> &'static str {
        match self {
            ListMode::Scroll => "scroll",
            ListMode::Virtual => "virtual",
        }
    }
}

struct ConversationStress {
    turns: usize,
    body_size: BodySize,
    body_mode: BodyMode,
    list_mode: ListMode,
}

impl Default for ConversationStress {
    fn default() -> Self {
        Self {
            turns: DEFAULT_TURNS,
            body_size: BodySize::Medium,
            body_mode: BodyMode::Markdown,
            list_mode: ListMode::Scroll,
        }
    }
}

impl App for ConversationStress {
    fn build(&self, cx: &BuildCx) -> El {
        let text_nodes_per_turn = match self.body_mode {
            BodyMode::Plain => 6 + self.body_size.paragraphs_per_turn(),
            BodyMode::Markdown => 18 + self.body_size.paragraphs_per_turn() * 10,
        };
        let approx_words = self.turns * self.body_size.approx_words_per_turn();
        let approx_text_nodes = self.turns * text_nodes_per_turn;

        let conversation = self.conversation();

        column([
            toolbar([
                toolbar_group([
                    toolbar_title("Conversation stress"),
                    toolbar_description(format!(
                        "{} text, {} list, {approx_text_nodes} estimated text leaves, ~{approx_words} generated words",
                        self.body_mode.label(),
                        self.list_mode.label()
                    )),
                ]),
                spacer(),
                control_button("-100", "turns:-100"),
                control_button("-10", "turns:-10"),
                badge(format!("{} turns", self.turns)).info(),
                control_button("+10", "turns:+10"),
                control_button("+100", "turns:+100"),
            ]),
            row([
                preset_button("50", self.turns == 50, "turns:50"),
                preset_button("250", self.turns == 250, "turns:250"),
                preset_button("1k", self.turns == 1_000, "turns:1000"),
                preset_button("2k", self.turns == 2_000, "turns:2000"),
                spacer(),
                size_button("Short", self.body_size == BodySize::Short, "size:short"),
                size_button("Medium", self.body_size == BodySize::Medium, "size:medium"),
                size_button("Long", self.body_size == BodySize::Long, "size:long"),
                spacer(),
                mode_button("Plain", self.body_mode == BodyMode::Plain, "mode:plain"),
                mode_button(
                    "Markdown",
                    self.body_mode == BodyMode::Markdown,
                    "mode:markdown",
                ),
                spacer(),
                mode_button("Scroll", self.list_mode == ListMode::Scroll, "list:scroll"),
                mode_button(
                    "Virtual",
                    self.list_mode == ListMode::Virtual,
                    "list:virtual",
                ),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            diagnostics_panel(
                cx.diagnostics(),
                self.turns,
                self.body_size,
                self.body_mode,
                self.list_mode,
                approx_text_nodes,
            ),
            conversation,
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_5)
        .height(Size::Fill(1.0))
    }

    fn on_event(&mut self, event: UiEvent) {
        if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
            return;
        }
        let Some(route) = event.route() else {
            return;
        };
        match route {
            "turns:-100" => self.adjust_turns(-100),
            "turns:-10" => self.adjust_turns(-10),
            "turns:+10" => self.adjust_turns(10),
            "turns:+100" => self.adjust_turns(100),
            "turns:50" => self.turns = 50,
            "turns:250" => self.turns = 250,
            "turns:1000" => self.turns = 1_000,
            "turns:2000" => self.turns = 2_000,
            "size:short" => self.body_size = BodySize::Short,
            "size:medium" => self.body_size = BodySize::Medium,
            "size:long" => self.body_size = BodySize::Long,
            "mode:plain" => self.body_mode = BodyMode::Plain,
            "mode:markdown" => self.body_mode = BodyMode::Markdown,
            "list:scroll" => self.list_mode = ListMode::Scroll,
            "list:virtual" => self.list_mode = ListMode::Virtual,
            _ => {}
        }
    }
}

impl ConversationStress {
    fn adjust_turns(&mut self, delta: isize) {
        self.turns = self
            .turns
            .saturating_add_signed(delta)
            .clamp(MIN_TURNS, MAX_TURNS);
    }

    fn conversation(&self) -> El {
        match self.list_mode {
            ListMode::Scroll => {
                let turns: Vec<El> = (0..self.turns)
                    .map(|i| turn(i, self.body_size, self.body_mode))
                    .collect();
                scroll(turns)
                    .key("conversation")
                    .gap(tokens::SPACE_2)
                    .height(Size::Fill(1.0))
                    .padding(tokens::SPACE_3)
            }
            ListMode::Virtual => {
                let body_size = self.body_size;
                let body_mode = self.body_mode;
                virtual_list_dyn(
                    self.turns,
                    estimated_turn_height(body_size),
                    |i| format!("turn-{i}"),
                    move |i| turn(i, body_size, body_mode),
                )
                .key("conversation")
                .gap(tokens::SPACE_2)
                .height(Size::Fill(1.0))
                .padding(tokens::SPACE_3)
            }
        }
    }
}

fn estimated_turn_height(body_size: BodySize) -> f32 {
    match body_size {
        BodySize::Short => 190.0,
        BodySize::Medium => 420.0,
        BodySize::Long => 900.0,
    }
}

fn turn(i: usize, body_size: BodySize, body_mode: BodyMode) -> El {
    let collapsed = i % 7 == 3;
    let paragraphs = if collapsed {
        1
    } else {
        body_size.paragraphs_per_turn()
    };

    let mut body: Vec<El> = Vec::with_capacity(paragraphs + 5);
    body.push(
        row([
            badge(format!("#{i:04}")).secondary(),
            text(if i.is_multiple_of(2) {
                "user"
            } else {
                "assistant"
            })
            .label()
            .bold(),
            text(format!("model=gpt-5.5 size={}", body_size.label()))
                .caption()
                .muted(),
            spacer(),
            text(if collapsed {
                "collapsed"
            } else {
                "full context"
            })
            .caption()
            .muted(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
    );

    body.push(message_body(prompt_text(i), body_mode));

    if collapsed {
        body.push(
            message_body(collapsed_summary(i), body_mode)
                .muted()
                .fill(tokens::MUTED)
                .padding(tokens::SPACE_3)
                .radius(tokens::RADIUS_SM),
        );
    } else {
        for p in 0..paragraphs {
            body.push(message_body(response_paragraph(i, p), body_mode));
        }
    }

    body.push(
        row([
            text(format!("turn_id=conv-{i:04}")).caption().muted(),
            text(format!("body_words~{}", paragraphs * 54))
                .caption()
                .muted(),
            spacer(),
            text("wrapped paragraphs").caption().muted(),
        ])
        .gap(tokens::SPACE_2),
    );

    column(body)
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_4)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fill(1.0))
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

fn size_button(label: &str, active: bool, key: &str) -> El {
    let button = button(label)
        .key(key)
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));
    if active {
        button.primary()
    } else {
        button.ghost()
    }
}

fn mode_button(label: &str, active: bool, key: &str) -> El {
    let button = button(label)
        .key(key)
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));
    if active {
        button.primary()
    } else {
        button.secondary()
    }
}

fn message_body(source: String, mode: BodyMode) -> El {
    match mode {
        BodyMode::Plain => paragraph(source).small(),
        BodyMode::Markdown => md(&source).width(Size::Fill(1.0)),
    }
}

fn diagnostics_panel(
    diag: Option<&HostDiagnostics>,
    turns: usize,
    body_size: BodySize,
    body_mode: BodyMode,
    list_mode: ListMode,
    estimated_text_nodes: usize,
) -> El {
    let Some(diag) = diag else {
        return text("Host diagnostics unavailable").caption().muted();
    };
    log_diagnostics(
        diag,
        turns,
        body_size,
        body_mode,
        list_mode,
        estimated_text_nodes,
    );
    let cpu_total = diag.last_build + diag.last_prepare + diag.last_submit;
    let cache_pressure = if estimated_text_nodes > 1_024 {
        "over 1024 layout-cache entries"
    } else {
        "below 1024 layout-cache entries"
    };
    column([
        row([
            metric("frame", format!("#{}", diag.frame_index)),
            metric("dt", format_dt(diag.last_frame_dt)),
            metric("cpu", format_duration(cpu_total)),
            metric("trigger", diag.trigger.label().to_string()),
            metric("cache", cache_pressure.to_string()),
        ])
        .gap(tokens::SPACE_3),
        row([
            metric("build", format_duration(diag.last_build)),
            metric("prepare", format_duration(diag.last_prepare)),
            metric("layout", format_duration(diag.last_layout)),
            metric(
                "intr hit",
                compact_count(diag.last_layout_intrinsic_cache_hits),
            ),
            metric(
                "intr miss",
                compact_count(diag.last_layout_intrinsic_cache_misses),
            ),
            metric("pruned", compact_count(diag.last_layout_pruned_subtrees)),
            metric("zeroed", compact_count(diag.last_layout_pruned_nodes)),
            metric("draw_ops", format_duration(diag.last_draw_ops)),
            metric(
                "draw cull",
                compact_count(diag.last_draw_ops_culled_text_ops),
            ),
            metric("paint", format_duration(diag.last_paint)),
            metric("culled", compact_count(diag.last_paint_culled_ops)),
            metric("gpu", format_duration(diag.last_gpu_upload)),
            metric("snapshot", format_duration(diag.last_snapshot)),
            metric("submit", format_duration(diag.last_submit)),
        ])
        .gap(tokens::SPACE_3),
        row([
            metric("cache hit", compact_count(diag.last_text_layout_cache_hits)),
            metric(
                "cache miss",
                compact_count(diag.last_text_layout_cache_misses),
            ),
            metric(
                "cache evict",
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

fn log_diagnostics(
    diag: &HostDiagnostics,
    turns: usize,
    body_size: BodySize,
    body_mode: BodyMode,
    list_mode: ListMode,
    estimated_text_nodes: usize,
) {
    if diag.frame_index == 0 {
        return;
    }
    if LAST_LOGGED_FRAME.swap(diag.frame_index, Ordering::Relaxed) == diag.frame_index {
        return;
    }
    let cpu_total = diag.last_build + diag.last_prepare + diag.last_submit;
    println!(
        "conversation_stress frame={} trigger={} turns={} body={} mode={} list={} est_text_nodes={} dt={} cpu={} build={} prepare={} layout={} intrinsic_hits={} intrinsic_misses={} layout_pruned={} layout_zeroed={} draw_ops={} draw_culled_text={} paint={} paint_culled={} gpu={} snapshot={} submit={} cache_hits={} cache_misses={} cache_evictions={} shaped_bytes={}",
        diag.frame_index,
        diag.trigger.label(),
        turns,
        body_size.label(),
        body_mode.label(),
        list_mode.label(),
        estimated_text_nodes,
        format_dt(diag.last_frame_dt),
        format_duration(cpu_total),
        format_duration(diag.last_build),
        format_duration(diag.last_prepare),
        format_duration(diag.last_layout),
        diag.last_layout_intrinsic_cache_hits,
        diag.last_layout_intrinsic_cache_misses,
        diag.last_layout_pruned_subtrees,
        diag.last_layout_pruned_nodes,
        format_duration(diag.last_draw_ops),
        diag.last_draw_ops_culled_text_ops,
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

fn metric(label: &str, value: String) -> El {
    column([mono(label).caption().muted(), mono(value).small()])
        .gap(1.0)
        .width(Size::Hug)
}

fn format_dt(dt: std::time::Duration) -> String {
    if dt.is_zero() {
        return "-".to_string();
    }
    let ms = dt.as_secs_f64() * 1000.0;
    let fps = 1000.0 / ms;
    format!("{ms:.1}ms/{fps:.1}fps")
}

fn format_duration(duration: std::time::Duration) -> String {
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

fn prompt_text(turn: usize) -> String {
    format!(
        "Prompt {turn}: inspect the current **repository state**, preserve *unrelated edits*, \
         compare `SHAPE_CACHE` churn with `cosmic-text` full-buffer shaping, and keep the response \
         grounded in [local code paths](https://example.invalid/damascene)."
    )
}

fn collapsed_summary(turn: usize) -> String {
    format!(
        "Collapsed turn {turn}: earlier work included **repository inspection**, `cargo` output, \
         inline review of `metrics::SHAPE_CACHE`, and a compact summary of the discussion. The \
         original messages were longer, but the renderer still lays this summary out as ordinary \
         markdown with *emphasis*, **strong text**, and `inline code`."
    )
}

fn response_paragraph(turn: usize, paragraph_index: usize) -> String {
    let topic = match (turn + paragraph_index) % 5 {
        0 => "layout cache pressure",
        1 => "cosmic text shaping",
        2 => "scroll viewport measurement",
        3 => "markdown transcript rendering",
        _ => "diagnostic instrumentation",
    };
    format!(
        "Turn {turn}, paragraph {paragraph_index}: this synthetic assistant response discusses \
         **{topic}**. It mixes *italic qualifiers*, **bold conclusions**, `inline_identifiers`, \
         and [reference links](https://example.invalid/turn/{turn}/{paragraph_index}) the way LLM \
         markdown often does. The text includes `stable_id_{turn}_{paragraph_index}`, quoted \
         phrases, and mixed sentence lengths so each message becomes a distinct layout key. The \
         goal is to make cache misses, eviction, styled inline runs, and large wrapped paragraphs \
         visible while keeping the example deterministic."
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 980.0, 720.0);
    let config =
        damascene_winit_wgpu::HostConfig::default().with_redraw_interval(Duration::from_secs(1));
    damascene_winit_wgpu::run_with_config(
        "Damascene - conversation stress",
        viewport,
        ConversationStress::default(),
        config,
    )
}
