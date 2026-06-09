//! Layout primitives — split panes (resize_handle), virtual_list,
//! scroll viewport.
//!
//! Each demo composes a structural primitive an app needs at the
//! outermost layer of a window.

use damascene_core::prelude::*;

const SPLIT_HANDLE_KEY: &str = "split-resize";

pub struct State {
    /// Current sidebar width in logical pixels.
    pub sidebar_w: f32,
    /// Drag-anchor state owned by the app, fed into
    /// `resize_handle::apply_event_fixed` on every routed event.
    pub sidebar_drag: ResizeDrag,
}

impl Default for State {
    fn default() -> Self {
        Self {
            sidebar_w: tokens::SIDEBAR_WIDTH,
            sidebar_drag: ResizeDrag::default(),
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Layout"),
        paragraph(
            "Three primitives apps reach for at the outermost layer: a \
             resizable split, a scroll viewport with a fixed `Fill(1.0)` \
             height, and a `virtual_list` that only builds rows for the \
             visible window.",
        )
        .muted(),
        section_label("Resizable split"),
        paragraph("Drag the divider, or focus and use Arrow keys.")
            .small()
            .muted(),
        split_demo(state, phone),
        section_label("Virtual list (10,000 rows)"),
        paragraph(
            "`virtual_list` only builds rows for the visible window — \
             below stays smooth even at 10k rows.",
        )
        .small()
        .muted(),
        virtual_demo(),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Stretch)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, event: UiEvent) {
    resize_handle::apply_event_fixed(
        &mut state.sidebar_w,
        &mut state.sidebar_drag,
        &event,
        SPLIT_HANDLE_KEY,
        Axis::Row,
        resize_handle::Side::Start,
        tokens::SIDEBAR_WIDTH_MIN,
        tokens::SIDEBAR_WIDTH_MAX,
    );
}

fn split_demo(state: &State, phone: bool) -> El {
    // Phone clamps the sidebar to ~110px so the body column still has
    // breathing room (≈190px) for a wrapped paragraph and the readout
    // row — at the desktop default (256px) the body would only get
    // ~30px on a 360px viewport.
    let sidebar_w = if phone {
        state.sidebar_w.min(110.0)
    } else {
        state.sidebar_w
    };
    let files = sidebar([
        text("Files").bold(),
        text("README.md").muted(),
        text("Cargo.toml").muted(),
        text("src/").muted(),
        text("examples/").muted(),
        text("tests/").muted(),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .width(Size::Fixed(sidebar_w))
    .radius(tokens::RADIUS_SM);

    let body = column([
        text("README.md").heading().wrap_text().fill_width(),
        text(format!(
            "Drag the divider to resize the sidebar. Width clamps \
             between {min}px and {max}px. The handle is focusable — Tab \
             to it, then ←/→ nudge by {step}px or PageUp/PageDown by \
             {page}px.",
            min = tokens::SIDEBAR_WIDTH_MIN as i32,
            max = tokens::SIDEBAR_WIDTH_MAX as i32,
            step = resize_handle::KEYBOARD_STEP_PX as i32,
            page = resize_handle::KEYBOARD_PAGE_STEP_PX as i32,
        ))
        .muted()
        .wrap_text()
        .fill_width(),
        row([
            text("Sidebar width:").muted().wrap_text(),
            text(format!("{:.0} px", state.sidebar_w)).bold(),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_3)
    .padding(tokens::SPACE_3)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0));

    let demo_height = if phone { 320.0 } else { 220.0 };
    row([files, resize_handle(SPLIT_HANDLE_KEY, Axis::Row), body])
        .height(Size::Fixed(demo_height))
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
}

fn virtual_demo() -> El {
    virtual_list(10_000, 32.0, |i| {
        row([
            badge(format!("#{i}")).muted(),
            text(format!("Row {i}")).label(),
            spacer(),
            text(format!("payload {}", i % 17)).small().muted(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .padding(Sides::xy(tokens::SPACE_3, 0.0))
        .height(Size::Fixed(32.0))
    })
    .key("layout-virtual")
    .height(Size::Fixed(180.0))
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_SM)
}

fn section_label(s: &str) -> El {
    h3(s).label()
}
