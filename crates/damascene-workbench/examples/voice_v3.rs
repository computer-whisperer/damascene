//! Banter — a voice-chat client (Mumble/TeamSpeak genre) on the
//! workbench theme.
//!
//! A static layout study at 1280×800: a channel rail docked left with
//! the people inside each channel nested under it and a self strip
//! pinned to its foot, a message log filling the work area under a
//! channel header, and a status bar carrying the connection and the
//! audio instrumentation.
//!
//! # What this round is testing
//!
//! `examples/voice.rs` and `examples/voice_v2.rs` are the earlier
//! validation rounds of this same brief. Both hand-rolled the two
//! productions core has since minted — a depth-indented hierarchy row
//! (`channel_row` / `user_row` plus a flatten loop) and a presence dot
//! — and `damascene_core::widgets::tree` / `status_dot` cite those
//! hand-rolls as their justification. So this file builds the rail out
//! of the stock [`tree`] / [`tree_item_with`] / [`status_dot`] and
//! keeps *no* private row grammar at all: the only geometry constants
//! below are the ones that describe this app's regions, not its rows.
//!
//! Everything else is stock too — `text_input`, `icon_button`,
//! `button_with_icon`, `avatar_initials`, `meter_with_color`, `icon` —
//! plus [`damascene_workbench::chrome`]'s `status_bar`, `pane_header`
//! and `chip`, rendered through [`damascene_workbench::theme::theme`].
//!
//! Nothing here is wired. The tree does not expand, the composer does
//! not type, the readouts do not move: this is a mock of one frame.
//!
//! Run: `cargo run -p damascene-workbench --example voice_v3`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---- region metrics -------------------------------------------------
//
// Regions only. Row heights, indents and dot sizes come from the stock
// widgets now (`TREE_ITEM_HEIGHT`, `TREE_INDENT`, `STATUS_DOT_SIZE`),
// which is the whole point of this round.

/// Width of the channel rail. Wide enough that a real channel name
/// clears a chevron, a speaker glyph and a member chip without
/// ellipsizing.
const RAIL_WIDTH: f32 = 240.0;

/// Server identity block at the top of the rail — two lines of text
/// plus its own breathing room.
const SERVER_HEADER_HEIGHT: f32 = 44.0;

/// Self strip at the foot of the rail. Taller than a tree row because
/// it carries 28px `Xs` icon buttons *and* their 2px focus ring.
const SELF_STRIP_HEIGHT: f32 = 40.0;

/// Channel header over the log — the editor-group header rung, so the
/// log reads as an editor under a tab strip.
const CHANNEL_HEADER_HEIGHT: f32 = 34.0;

/// Timestamp gutter in the log, sized for `HH:MM` at the mono face.
const TIMESTAMP_WIDTH: f32 = 34.0;

/// Author column. Fixed rather than hugging, so every message body
/// starts on one left edge down the whole log.
const AUTHOR_WIDTH: f32 = 92.0;

/// Avatar in the self strip — the tree-row rung, not the stock 40px.
const AVATAR_SIZE: f32 = 22.0;

/// Audio level meter in the status bar.
const LEVEL_WIDTH: f32 = 42.0;

/// Level meter thickness. Thinner than the stock meter, so it reads as
/// instrumentation inside a 22px bar rather than as a progress control.
const LEVEL_HEIGHT: f32 = 4.0;

// ---- content --------------------------------------------------------

const SERVER_NAME: &str = "Northwind Collective";
const SERVER_HOST: &str = "voice.example.com";
const CHANNEL_TOPIC: &str = "daily standup, 09:15 — sprint 44 burndown";

/// Where a member is, as the client understands it. The mapping onto
/// palette tokens is the *app's* — `status_dot` takes a color for
/// exactly this reason.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Presence {
    /// Transmitting right now.
    Speaking,
    /// Connected and listening.
    Online,
    /// Connected, no audio for a while.
    Idle,
    /// Connected with do-not-disturb set.
    Busy,
}

impl Presence {
    fn color(self) -> Color {
        match self {
            Presence::Speaking => tokens::SUCCESS,
            Presence::Online => vs::TEXT_LINK_FG,
            Presence::Idle => tokens::MUTED_FOREGROUND,
            Presence::Busy => tokens::WARNING,
        }
    }
}

struct Member {
    name: &'static str,
    presence: Presence,
    muted: bool,
    deafened: bool,
    is_self: bool,
}

impl Member {
    const fn new(name: &'static str, presence: Presence) -> Self {
        Self {
            name,
            presence,
            muted: false,
            deafened: false,
            is_self: false,
        }
    }

    const fn muted(mut self) -> Self {
        self.muted = true;
        self
    }

    const fn deafened(mut self) -> Self {
        self.muted = true;
        self.deafened = true;
        self
    }

    const fn me(mut self) -> Self {
        self.is_self = true;
        self
    }
}

struct Channel {
    name: &'static str,
    expanded: bool,
    members: &'static [Member],
}

/// The one channel the client is joined to — indexed into [`CHANNELS`]
/// so the header, the log and the rail's selection cannot disagree.
const ACTIVE_CHANNEL: usize = 1;

const CHANNELS: &[Channel] = &[
    Channel {
        name: "Lobby",
        expanded: true,
        members: &[
            Member::new("ana.ruiz", Presence::Online),
            Member::new("jkwan", Presence::Idle),
        ],
    },
    Channel {
        name: "Engineering — Standup",
        expanded: true,
        members: &[
            Member::new("m.vasquez", Presence::Speaking).me(),
            Member::new("tom.hedlund", Presence::Speaking),
            Member::new("dana.kovac", Presence::Online),
            Member::new("s.oyelaran", Presence::Busy).muted(),
            Member::new("wei.lin", Presence::Idle).deafened(),
        ],
    },
    Channel {
        name: "Design Review",
        expanded: false,
        members: &[
            Member::new("priya.nandakumar", Presence::Online),
            Member::new("lucas.b", Presence::Idle),
        ],
    },
    Channel {
        name: "Deploys & Alerts",
        expanded: true,
        members: &[
            Member::new("buildbot", Presence::Online),
            Member::new("oncall.rota", Presence::Busy).muted(),
        ],
    },
    Channel {
        name: "AFK",
        expanded: false,
        members: &[
            Member::new("hkrause", Presence::Idle),
            Member::new("n.abiodun", Presence::Idle),
            Member::new("rmg", Presence::Idle),
        ],
    },
];

/// One line of the log. `author: None` is a server line — a join, a
/// codec renegotiation — which the genre sets dim and italic across the
/// full measure instead of in the author column.
struct Line {
    time: &'static str,
    author: Option<&'static str>,
    accent: Color,
    body: &'static str,
}

const fn said(time: &'static str, author: &'static str, accent: Color, body: &'static str) -> Line {
    Line {
        time,
        author: Some(author),
        accent,
        body,
    }
}

const fn server(time: &'static str, body: &'static str) -> Line {
    Line {
        time,
        author: None,
        accent: vs::DESCRIPTION_FG,
        body,
    }
}

/// Per-author accents. Four tokens, assigned per person the way every
/// client in this genre does — a raw rgba here would trip the
/// `RawColor` lint, and rightly.
const A_TOM: Color = vs::TEXT_LINK_FG;
const A_DANA: Color = tokens::SUCCESS;
const A_SOLA: Color = tokens::WARNING;
const A_ME: Color = vs::CHAT_EDITED_FILE_FG;

const LOG: &[Line] = &[
    server("08:47", "Channel opened · 12 kbit/s Opus, 48 kHz, VBR"),
    said(
        "08:52",
        "tom.hedlund",
        A_TOM,
        "reminder that the standup slot moved to 09:15 this week — calendar is updated",
    ),
    server("09:02", "dana.kovac joined Engineering — Standup"),
    said(
        "09:03",
        "dana.kovac",
        A_DANA,
        "early. going to make coffee, ping me if you need anything before we start.",
    ),
    server("09:08", "m.vasquez joined Engineering — Standup"),
    said(
        "09:09",
        "m.vasquez",
        A_ME,
        "audio check — tom, am I clipping? my interface reset itself overnight.",
    ),
    said(
        "09:09",
        "tom.hedlund",
        A_TOM,
        "you're fine, maybe a shade hot. drop input gain 3 dB and you're clean.",
    ),
    said("09:10", "m.vasquez", A_ME, "done, thanks."),
    said(
        "09:11",
        "dana.kovac",
        A_DANA,
        "morning — I pushed the retry backoff branch last night, CI is green",
    ),
    said(
        "09:12",
        "tom.hedlund",
        A_TOM,
        "nice. did the flaky gateway test settle down or is it still retrying twice?",
    ),
    said(
        "09:12",
        "dana.kovac",
        A_DANA,
        "settled. it was the fixture clock, not the backoff.",
    ),
    server("09:13", "s.oyelaran joined Engineering — Standup"),
    said(
        "09:13",
        "s.oyelaran",
        A_SOLA,
        "here, mic is off — kids. I'll type. Sprint 44 burndown is at 61%, we're two \
         points behind but the deploy queue is empty so I'm not worried yet.",
    ),
    said(
        "09:14",
        "m.vasquez",
        A_ME,
        "that tracks. the two points are both the migration ticket, which I'd rather \
         slip than rush.",
    ),
    said(
        "09:15",
        "tom.hedlund",
        A_TOM,
        "agreed. slipping it. I'll move it to 45 after standup.",
    ),
    server("09:15", "wei.lin deafened"),
    said(
        "09:16",
        "dana.kovac",
        A_DANA,
        "one ask: someone look at the alert that fired at 03:40? it self-resolved and \
         I don't love that.",
    ),
    said(
        "09:16",
        "m.vasquez",
        A_ME,
        "mine. it's the cert renewal window, I'll widen the threshold today.",
    ),
    said(
        "09:17",
        "s.oyelaran",
        A_SOLA,
        "👍 anything blocking anyone else?",
    ),
    said(
        "09:17",
        "tom.hedlund",
        A_TOM,
        "nothing here. calling it — back at 13:00 for the review.",
    ),
];

/// The composer holds a real draft so the input is not painted empty,
/// but nothing consumes it — see the module docs.
const DRAFT: &str = "widening the cert-renewal alert threshold now";

struct Banter {
    selection: Selection,
}

impl Banter {
    fn new() -> Self {
        Self {
            selection: Selection::default(),
        }
    }

    // ---- rail -------------------------------------------------------

    /// Server identity. Two lines: who you are connected to, and where
    /// — the second line is the thing you check when something is
    /// wrong, so it is dim but always present.
    fn server_header(&self) -> El {
        row([
            icon(IconName::Globe)
                .icon_size(tokens::ICON_SM)
                .color(vs::TEXT_LINK_FG),
            column([
                text(SERVER_NAME)
                    .label()
                    .font_weight(FontWeight::Semibold)
                    .text_color(vs::SIDE_BAR_TITLE_FG)
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text(SERVER_HOST)
                    .caption()
                    .text_color(vs::DESCRIPTION_FG)
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_3))
        .fill(vs::SIDE_BAR_BG)
        .border_b()
        .border_color(vs::SIDE_BAR_BORDER)
        .height(Size::Fixed(SERVER_HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The channel tree.
    ///
    /// Flat list, app-owned expansion: one [`tree_item_with`] per
    /// *visible* node, with an explicit depth, and collapsed channels
    /// simply not emitting their members. That is the stock tree's
    /// contract, and it is exactly the flatten loop the two earlier
    /// rounds wrote by hand around private row recipes.
    fn channel_tree(&self) -> El {
        let mut rows: Vec<El> = Vec::new();

        for (i, channel) in CHANNELS.iter().enumerate() {
            let row = tree_item_with(
                &format!("channel:{}", channel.name),
                0,
                channel.name,
                TreeItemOpts::default()
                    .expanded(channel.expanded)
                    .icon(IconName::Volume2)
                    .trailing(chip(format!("{}", channel.members.len()))),
            );
            // The joined channel is `current`, not `selected`: you are
            // *in* it, which is the persistent-location sense of the
            // pair.
            rows.push(if i == ACTIVE_CHANNEL {
                row.current()
            } else {
                row
            });

            if channel.expanded {
                rows.extend(channel.members.iter().map(|m| member_row(channel, m)));
            }
        }

        scroll([tree(rows)])
            .padding(Sides::y(tokens::SPACE_1))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
    }

    /// The self strip — who you are on this server, and the three
    /// toggles a voice client keeps one click away at all times.
    fn self_strip(&self) -> El {
        let me = CHANNELS[ACTIVE_CHANNEL]
            .members
            .iter()
            .find(|m| m.is_self)
            .expect("the local user is in the joined channel");

        row([
            avatar_initials("MV")
                .width(Size::Fixed(AVATAR_SIZE))
                .height(Size::Fixed(AVATAR_SIZE)),
            column([
                text(me.name)
                    .caption()
                    .font_weight(FontWeight::Medium)
                    .text_color(vs::FOREGROUND)
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("Transmitting · F13")
                    .caption()
                    .text_color(tokens::SUCCESS)
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Hug),
            // `Xs` (28px) rather than the strips' `Xxs`: this row is
            // 40px and these are the controls you hit under pressure.
            icon_button(IconName::Mic)
                .key("self:mic")
                .ghost()
                .size(ComponentSize::Xs)
                .tooltip("Mute microphone"),
            icon_button(IconName::Headphones)
                .key("self:deafen")
                .ghost()
                .size(ComponentSize::Xs)
                .tooltip("Deafen"),
            icon_button(IconName::Settings)
                .key("self:settings")
                .ghost()
                .size(ComponentSize::Xs)
                .tooltip("Audio settings"),
        ])
        .gap(tokens::SPACE_1)
        .padding(Sides::x(tokens::SPACE_2))
        .fill(vs::SIDE_BAR_BG)
        .border_t()
        .border_color(vs::SIDE_BAR_BORDER)
        .height(Size::Fixed(SELF_STRIP_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    fn rail(&self) -> El {
        let population: usize = CHANNELS.iter().map(|c| c.members.len()).sum();

        column([
            self.server_header(),
            pane_header("CHANNELS", [chip(format!("{population}"))]),
            self.channel_tree(),
            self.self_strip(),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_r()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(RAIL_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    // ---- work area --------------------------------------------------

    /// Channel name, topic, and the population on the right.
    fn channel_header(&self) -> El {
        let channel = &CHANNELS[ACTIVE_CHANNEL];

        row([
            icon(IconName::Volume2)
                .icon_size(tokens::ICON_XS)
                .color(vs::TEXT_LINK_FG),
            text(channel.name)
                .label()
                .font_weight(FontWeight::Semibold)
                .text_color(vs::TAB_ACTIVE_FG)
                .nowrap_text(),
            vertical_hairline().height(Size::Fixed(14.0)),
            text(CHANNEL_TOPIC)
                .caption()
                .text_color(vs::DESCRIPTION_FG)
                .ellipsis()
                .width(Size::Fill(1.0)),
            icon(IconName::Users)
                .icon_size(tokens::ICON_XS)
                .color(vs::DESCRIPTION_FG),
            text(format!("{} in channel", channel.members.len()))
                .caption()
                .text_color(vs::DESCRIPTION_FG)
                .nowrap_text(),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_3))
        .fill(vs::EDITOR_GROUP_HEADER_TABS_BG)
        .border_b()
        .border_color(vs::EDITOR_GROUP_HEADER_TABS_BORDER)
        .height(Size::Fixed(CHANNEL_HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The log. `pin_end` is the chat-log scroll *policy*: the viewport
    /// opens on its tail and follows new lines until the reader scrolls
    /// away from the bottom. It says nothing about a log shorter than
    /// its well, though — that one is `Justify::End`, which rests the
    /// day's traffic on the composer instead of hanging it from the
    /// channel header with a screen of nothing underneath.
    fn message_log(&self) -> El {
        column(LOG.iter().map(log_row).collect::<Vec<_>>())
            .justify(Justify::End)
            .gap(tokens::SPACE_2)
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
            .fill(vs::EDITOR_BG)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .scrollable()
            .pin_end()
    }

    fn composer(&self) -> El {
        row([
            text_input_with(
                "composer",
                DRAFT,
                &self.selection,
                TextInputOpts::default().placeholder("Message Engineering — Standup…"),
            )
            .width(Size::Fill(1.0)),
            button_with_icon(IconName::Send, "Send")
                .key("composer:send")
                .primary(),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_2)
        .fill(vs::PANEL_BG)
        .border_t()
        .border_color(vs::PANEL_BORDER)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
    }
}

// ---- leaf recipes ---------------------------------------------------

/// A member inside an expanded channel — a depth-1 [`tree_item_with`]
/// whose trailing slot carries the state cluster.
///
/// The mute / deafen glyphs and the [`status_dot`] all ride in
/// `trailing` because that is the tree row's only *element* slot;
/// `TreeItemOpts::icon` takes an icon source and paints it
/// `MUTED_FOREGROUND`, which neither a colored state glyph nor a dot
/// can go through. Reading right-to-left the cluster is still the
/// genre's own status column.
fn member_row(channel: &Channel, member: &Member) -> El {
    let mut cluster: Vec<El> = Vec::new();
    if member.deafened {
        cluster.push(
            icon(IconName::HeadphoneOff)
                .icon_size(tokens::ICON_XS)
                .color(vs::ERROR_FG),
        );
    }
    if member.muted {
        cluster.push(
            icon(IconName::MicOff)
                .icon_size(tokens::ICON_XS)
                .color(vs::ERROR_FG),
        );
    }
    cluster.push(status_dot(member.presence.color()));

    let row_el = tree_item_with(
        &format!("member:{}:{}", channel.name, member.name),
        1,
        member.name,
        TreeItemOpts::default().trailing(
            row(cluster)
                .gap(tokens::SPACE_1)
                .width(Size::Hug)
                .align(Align::Center),
        ),
    );

    if member.is_self {
        row_el.selected()
    } else {
        row_el
    }
}

/// One log line: a mono timestamp gutter, a fixed author column, and
/// the body filling the rest of the measure.
fn log_row(line: &Line) -> El {
    let time = text(line.time)
        .caption()
        .mono()
        .tabular_numerals()
        .text_color(vs::DESCRIPTION_FG)
        .nowrap_text()
        .width(Size::Fixed(TIMESTAMP_WIDTH));

    match line.author {
        Some(author) => row([
            time,
            text(author)
                .caption()
                .font_weight(FontWeight::Medium)
                .text_color(line.accent)
                .ellipsis()
                .width(Size::Fixed(AUTHOR_WIDTH)),
            text(line.body)
                .caption()
                .text_color(vs::EDITOR_FG)
                .wrap_text()
                .width(Size::Fill(1.0)),
        ]),
        // A server line skips the author column entirely and runs dim
        // and italic across the full measure — the genre's convention
        // for "this was the server, not a person".
        None => row([
            time,
            text(line.body)
                .caption()
                .italic()
                .text_color(vs::DESCRIPTION_FG)
                .wrap_text()
                .width(Size::Fill(1.0)),
        ]),
    }
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
    .align(Align::Start)
}

/// A labelled audio level for the status bar's trailing side.
fn level(label: &str, value: f32, color: Color) -> El {
    row([
        text(label).caption().text_color(vs::DESCRIPTION_FG),
        meter_with_color(value, color)
            .width(Size::Fixed(LEVEL_WIDTH))
            .height(Size::Fixed(LEVEL_HEIGHT)),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Hug)
    .height(Size::Hug)
    .align(Align::Center)
}

impl App for Banter {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root, not `page()`: a client shell is
        // full-bleed, and window padding would float the whole
        // instrument on a margin.
        //
        // The self strip carries tooltips, though, and the runtime
        // pushes the tooltip layer in as the root's last child — under
        // a bare column that becomes a third row and squashes the
        // status bar. `overlays(shell, [])` is the whole fix and is
        // layout-neutral.
        let shell = column([
            row([
                self.rail(),
                column([self.channel_header(), self.message_log(), self.composer()])
                    .fill(vs::EDITOR_BG)
                    .width(Size::Fill(1.0))
                    .height(Size::Fill(1.0))
                    .align(Align::Stretch),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
            status_bar(
                [
                    row([
                        status_dot(tokens::SUCCESS),
                        text(format!("Connected — {SERVER_HOST} · 48 ms")).caption(),
                    ])
                    .gap(tokens::SPACE_1)
                    .width(Size::Hug)
                    .align(Align::Center),
                    text("TLS 1.3 · cert pinned").caption(),
                ],
                [
                    level("IN", 0.64, tokens::SUCCESS),
                    level("OUT", 0.31, vs::TEXT_LINK_FG),
                    chip("Opus 48 kHz"),
                    chip("VBR 64 kbit/s"),
                ],
            ),
        ])
        .fill(vs::EDITOR_BG)
        .align(Align::Stretch);

        overlays(shell, [])
    }

    // Without this, every stock control renders in shadcn zinc at 36px
    // and the layered surfaces collapse into one background.
    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Banter", viewport, Banter::new())
}
