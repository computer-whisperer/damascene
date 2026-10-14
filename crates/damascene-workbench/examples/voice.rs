//! "Banter" — a voice-chat client (the Mumble / TeamSpeak genre) on the
//! workbench theme.
//!
//! A static mock: channel tree with presence, message log, composer,
//! status bar. Every control is a **stock** damascene widget
//! (`icon_button`, `text_input`, `button`, `avatar_initials`,
//! `vertical_separator`) plus the [`damascene_workbench::chrome`]
//! recipes (`pane_header`, `chip`, `status_bar`), rendered through
//! [`damascene_workbench::theme::theme`].
//!
//! The genre test this example exists for: a voice client is *rosters
//! and levels*, not documents. If the workbench vocabulary only holds up
//! for editor-shaped apps it is a skin, not a system. The regions here —
//! a 240px roster rail, a 35px channel header, a 22px status bar, 22px
//! tree rows — are the same layered-surface / 1px-separator / dense-row
//! grammar as `shell.rs`, pointed at a different content shape.
//!
//! Run: `cargo run -p damascene-workbench --example voice`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// Tree row height — the roster is the densest region in the shell, so
/// its rows sit on the same 22px rung as the bars.
const ROW_HEIGHT: f32 = 22.0;

/// Channel header height — VS Code's editor-group header rung, taller
/// than [`vs::PANE_HEADER_HEIGHT`] because it carries a topic.
const CHANNEL_HEADER_HEIGHT: f32 = 35.0;

/// Width of the roster rail.
const RAIL_WIDTH: f32 = 240.0;

/// Gutter width for message timestamps — wide enough for `hh:mm` in
/// tabular mono.
const TIME_COL: f32 = 40.0;

/// Author-name column. Right-aligned, so long names ellipsize away from
/// the message text rather than into it.
const AUTHOR_COL: f32 = 104.0;

// ---------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------

/// What a roster dot says about a participant.
#[derive(Clone, Copy, PartialEq)]
enum Presence {
    /// Transmitting right now.
    Speaking,
    /// Connected, quiet.
    Idle,
    /// Connected, marked away.
    Away,
    /// Microphone muted.
    Muted,
    /// Output muted, which implies the microphone is too.
    Deafened,
}

impl Presence {
    /// The dot color. Deliberately all palette tokens: a presence ramp
    /// is a status ramp, so it follows a theme swap.
    fn color(self) -> Color {
        match self {
            Presence::Speaking => vs::EDITOR_GUTTER_ADDED_BG,
            Presence::Idle => vs::DESCRIPTION_FG,
            Presence::Away => vs::CHAT_EDITED_FILE_FG,
            Presence::Muted | Presence::Deafened => vs::ERROR_FG,
        }
    }

    /// Trailing flag glyphs for the row.
    ///
    /// Each flag carries its own `.key(...)`: a tooltip only fires on a
    /// keyed node, because hit-testing returns keyed nodes and would
    /// otherwise skip past the flag to the enclosing row. `owner` is the
    /// member row's key, which the flags hang a suffix off.
    fn flags(self, owner: &str) -> Vec<El> {
        let flag = |name: IconName, key: String, tip: &'static str| {
            icon(name)
                .icon_size(tokens::ICON_XS)
                .color(vs::ERROR_FG)
                .key(key)
                .tooltip(tip)
        };
        match self {
            Presence::Muted => vec![flag(
                IconName::MicOff,
                format!("{owner}:mic"),
                "Microphone muted",
            )],
            Presence::Deafened => vec![
                flag(IconName::MicOff, format!("{owner}:mic"), "Microphone muted"),
                flag(
                    IconName::HeadphoneOff,
                    format!("{owner}:deaf"),
                    "Deafened — output muted",
                ),
            ],
            _ => Vec::new(),
        }
    }
}

struct Member {
    name: &'static str,
    presence: Presence,
}

struct Channel {
    name: &'static str,
    expanded: bool,
    members: Vec<Member>,
}

struct Message {
    time: &'static str,
    author: &'static str,
    body: &'static str,
    /// Join / move / part notices, rendered as one dim line.
    system: bool,
}

struct Banter {
    server: &'static str,
    channels: Vec<Channel>,
    active: &'static str,
    messages: Vec<Message>,
    draft: String,
    selection: Selection,
}

/// A stable per-author color. Four tokens is the whole vocabulary the
/// workbench palette offers for categorical identity — see the note at
/// the bottom of this file.
fn author_color(name: &str) -> Color {
    match name {
        "marisol.reyes" => vs::TEXT_LINK_FG,
        "t.vaughan" => vs::EDITOR_GUTTER_ADDED_BG,
        "ollie.b" => vs::CHAT_EDITED_FILE_FG,
        _ => vs::ERROR_FG,
    }
}

/// A presence dot: the smallest thing in the shell that carries state.
fn dot(color: Color) -> El {
    column(Vec::<El>::new())
        .width(Size::Fixed(7.0))
        .height(Size::Fixed(7.0))
        .radius(tokens::RADIUS_PILL)
        .fill(color)
}

fn member(name: &'static str, presence: Presence) -> Member {
    Member { name, presence }
}

impl Banter {
    fn new() -> Self {
        Self {
            server: "NEBULA COLLECTIVE",
            active: "Dev — Standup",
            channels: vec![
                Channel {
                    name: "Lobby",
                    expanded: true,
                    members: vec![
                        member("hannah.dietz", Presence::Idle),
                        member("pavel.k", Presence::Muted),
                    ],
                },
                Channel {
                    name: "General",
                    expanded: true,
                    members: vec![
                        member("sam.okafor", Presence::Speaking),
                        member("nadia.q", Presence::Idle),
                        member("grant.ellis", Presence::Deafened),
                        member("kirsten.vogel", Presence::Away),
                    ],
                },
                Channel {
                    name: "Dev — Standup",
                    expanded: true,
                    members: vec![
                        member("marisol.reyes", Presence::Speaking),
                        member("t.vaughan", Presence::Idle),
                        member("ben.kepner", Presence::Idle),
                    ],
                },
                Channel {
                    name: "Gaming",
                    expanded: false,
                    members: vec![member("ollie.b", Presence::Idle)],
                },
                Channel {
                    name: "Music & Media",
                    expanded: false,
                    members: Vec::new(),
                },
                Channel {
                    name: "AFK",
                    expanded: false,
                    members: vec![
                        member("jin.hee", Presence::Away),
                        member("dario.m", Presence::Away),
                    ],
                },
                Channel {
                    name: "Support",
                    expanded: false,
                    members: vec![member("kirsten.vogel", Presence::Idle)],
                },
            ],
            messages: vec![
                Message {
                    time: "09:41",
                    author: "marisol.reyes",
                    body: "Pushed the fix for the audio device enumeration crash — the ALSA \
                           path was handing us a null default device on machines with no \
                           hardware mixer.",
                    system: false,
                },
                Message {
                    time: "09:41",
                    author: "marisol.reyes",
                    body: "Rebuilding now. It should be on the nightly channel in ten minutes.",
                    system: false,
                },
                Message {
                    time: "09:43",
                    author: "t.vaughan",
                    body: "Good — I could reproduce it on the Framework every single cold boot, \
                           so I'll retest there first.",
                    system: false,
                },
                Message {
                    time: "09:44",
                    author: "ollie.b",
                    body: "Do we still need the 20 ms frame-size workaround for the old Opus \
                           builds, or can that go?",
                    system: false,
                },
                Message {
                    time: "09:46",
                    author: "marisol.reyes",
                    body: "Only for 1.3.x. Anything newer negotiates 10 ms fine.",
                    system: false,
                },
                Message {
                    time: "09:47",
                    author: "",
                    body: "ollie.b moved from General to Dev — Standup",
                    system: true,
                },
                Message {
                    time: "09:48",
                    author: "jin.hee",
                    body: "Standup in two. I'm taking notes today, shout if you want something \
                           on the agenda.",
                    system: false,
                },
                Message {
                    time: "09:49",
                    author: "t.vaughan",
                    body: "Can someone look at the jitter buffer graph? I'm seeing 40 ms spikes \
                           through the relay, but only on the new box.",
                    system: false,
                },
                Message {
                    time: "09:51",
                    author: "ollie.b",
                    body: "On it. Suspect the NIC interrupt coalescing settings — that host \
                           never got the tuning profile.",
                    system: false,
                },
                Message {
                    time: "09:52",
                    author: "jin.hee",
                    body: "Agenda then: audio crash, jitter spikes, push-to-talk rebind bug.",
                    system: false,
                },
                Message {
                    time: "09:53",
                    author: "marisol.reyes",
                    body: "+1. The rebind bug is mine — patch is up, it just needs a review.",
                    system: false,
                },
            ],
            draft: "I'll take the review after standup".to_string(),
            selection: Selection::default(),
        }
    }

    // -----------------------------------------------------------------
    // Roster rail
    // -----------------------------------------------------------------

    /// Channel row: expand affordance, name, occupancy count.
    ///
    /// Hand-rolled rather than `sidebar_menu_button_with_icon`, which has
    /// no trailing slot for the count chip — see the note at the bottom.
    fn channel_row(&self, channel: &Channel) -> El {
        let current = channel.name == self.active;
        let chevron = if channel.expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };

        let row = row([
            icon(chevron)
                .icon_size(tokens::ICON_XS)
                .color(vs::DESCRIPTION_FG),
            text(channel.name)
                .label()
                .ellipsis()
                .width(Size::Fill(1.0))
                .color(if current {
                    vs::TAB_ACTIVE_FG
                } else {
                    vs::SIDE_BAR_FG
                }),
            chip(format!("{}", channel.members.len())),
        ])
        .key(format!("channel:{}", channel.name))
        .gap(tokens::SPACE_1)
        .padding(Sides::xy(tokens::SPACE_2, 0.0))
        .height(Size::Fixed(ROW_HEIGHT))
        .width(Size::Fill(1.0))
        .radius(vs::RADIUS)
        .align(Align::Center)
        .focusable()
        .cursor(Cursor::Pointer);

        if current {
            row.fill(vs::LIST_ACTIVE_SELECTION_BG)
        } else {
            row
        }
    }

    /// Participant row, indented under its channel: presence dot, name,
    /// mute / deafen flags.
    fn member_row(&self, channel: &Channel, member: &Member) -> El {
        // Keys are channel-scoped: the same person can be listed in two
        // channels in a mock roster, and two rows sharing a computed id
        // is a `DuplicateId` lint (and, at runtime, one row eating the
        // other's hits).
        let row_key = format!("member:{}:{}", channel.name, member.name);
        let mut children = vec![
            dot(member.presence.color()),
            text(member.name)
                .caption()
                .ellipsis()
                .width(Size::Fill(1.0))
                .color(if member.presence == Presence::Speaking {
                    vs::TAB_ACTIVE_FG
                } else {
                    vs::DESCRIPTION_FG
                }),
        ];
        children.extend(member.presence.flags(&row_key));

        row(children)
            .key(row_key.clone())
            .gap(tokens::SPACE_2)
            .padding(Sides::xy(tokens::SPACE_2, 0.0))
            .pl(tokens::SPACE_6)
            .height(Size::Fixed(ROW_HEIGHT))
            .width(Size::Fill(1.0))
            .radius(vs::RADIUS)
            .align(Align::Center)
            .cursor(Cursor::Pointer)
    }

    /// The whole tree, flattened: expanded channels emit their members
    /// immediately after their own row.
    fn channel_tree(&self) -> El {
        let mut rows: Vec<El> = Vec::new();
        for channel in &self.channels {
            rows.push(self.channel_row(channel));
            if channel.expanded {
                rows.extend(
                    channel
                        .members
                        .iter()
                        .map(|m| self.member_row(channel, m)),
                );
            }
        }

        column(rows)
            .gap(0.0)
            .padding(Sides::xy(tokens::SPACE_1, tokens::SPACE_1))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .clip()
            .scrollable()
    }

    /// The "this is you" strip pinned to the bottom of the rail —
    /// identity on the left, the three controls a voice client is
    /// judged on to the right of it.
    fn self_strip(&self) -> El {
        row([
            // `avatar_initials` pins its label to the caption role,
            // which carries `descriptionForeground` — grey on the
            // accent fill. Restate the color for contrast.
            avatar_initials("BK")
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0))
                .color(vs::LIST_ACTIVE_SELECTION_FG),
            column([
                text("ben.kepner")
                    .caption()
                    .font_weight(FontWeight::Medium)
                    .ellipsis()
                    .color(vs::SIDE_BAR_FG),
                text("Dev — Standup")
                    .caption()
                    .font_size(10.0)
                    .ellipsis()
                    .color(vs::DESCRIPTION_FG),
            ])
            .gap(0.0)
            .width(Size::Fill(1.0)),
            icon_button(IconName::Mic)
                .key("self:mic")
                .ghost()
                .tooltip("Mute microphone — F1"),
            icon_button(IconName::Headphones)
                .key("self:deafen")
                .ghost()
                .tooltip("Deafen — F2"),
            icon_button(IconName::Settings)
                .key("self:settings")
                .ghost()
                .tooltip("Audio settings"),
        ])
        .gap(tokens::SPACE_1)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .width(Size::Fill(1.0))
        .border_t()
        .border_color(vs::SIDE_BAR_BORDER)
        .align(Align::Center)
    }

    fn rail(&self) -> El {
        column([
            pane_header(self.server, [chip("13 online")]),
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

    // -----------------------------------------------------------------
    // Conversation
    // -----------------------------------------------------------------

    /// Channel name, topic, occupancy. Sits in the same
    /// `editorGroupHeader.tabsBackground` trough a tab strip would, so
    /// swapping this for real tabs later is a drop-in.
    fn channel_header(&self) -> El {
        row([
            text("#").label().color(vs::DESCRIPTION_FG),
            text(self.active).label().color(vs::TAB_ACTIVE_FG),
            vertical_separator().height(Size::Fixed(14.0)),
            text("Daily sync · sprint 42 · notes land in the wiki")
                .caption()
                .ellipsis()
                .width(Size::Fill(1.0))
                .color(vs::DESCRIPTION_FG),
            icon(IconName::Users)
                .icon_size(tokens::ICON_XS)
                .color(vs::DESCRIPTION_FG),
            text("3 of 12 connected")
                .caption()
                .color(vs::DESCRIPTION_FG),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(CHANNEL_HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .fill(vs::EDITOR_GROUP_HEADER_TABS_BG)
        .border_b()
        .border_color(vs::EDITOR_GROUP_HEADER_TABS_BORDER)
        .align(Align::Center)
    }

    /// One log line. Three columns — time, author, body — so names and
    /// timestamps form vertical rules the eye can skip down, which is the
    /// whole reason chat logs in this genre are not chat bubbles.
    fn message_row(&self, message: &Message) -> El {
        let time = text(message.time)
            .mono()
            .caption()
            .tabular_numerals()
            .width(Size::Fixed(TIME_COL))
            .color(vs::DESCRIPTION_FG);

        if message.system {
            return row([
                time,
                text(format!("— {}", message.body))
                    .caption()
                    .italic()
                    .wrap_text()
                    .width(Size::Fill(1.0))
                    .color(vs::DESCRIPTION_FG),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .align(Align::Start);
        }

        row([
            time,
            text(message.author)
                .caption()
                .font_weight(FontWeight::Semibold)
                .ellipsis()
                .end_text()
                .width(Size::Fixed(AUTHOR_COL))
                .color(author_color(message.author)),
            text(message.body)
                .body()
                .wrap_text()
                .width(Size::Fill(1.0))
                .color(vs::EDITOR_FG),
        ])
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .align(Align::Start)
    }

    fn message_log(&self) -> El {
        column(
            self.messages
                .iter()
                .map(|m| self.message_row(m))
                .collect::<Vec<El>>(),
        )
        .gap(tokens::SPACE_2)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .clip()
        .scrollable()
        .pin_end()
    }

    /// The composer, pinned under the log by layout order rather than by
    /// any pinning mechanism: the log takes `Fill`, this takes `Hug`.
    fn composer(&self) -> El {
        row([
            text_input_with(
                "composer",
                &self.draft,
                &self.selection,
                TextInputOpts::default().placeholder("Message Dev — Standup"),
            )
            .width(Size::Fill(1.0)),
            button("Send").key("composer:send").primary(),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .border_t()
        .border_color(vs::PANEL_BORDER)
        .align(Align::Center)
    }

    fn conversation(&self) -> El {
        column([self.channel_header(), self.message_log(), self.composer()])
            .fill(vs::EDITOR_BG)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
    }
}

impl App for Banter {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column, not `page()` — same reasoning as `shell.rs`: an
        // application shell is full-bleed, and window padding would
        // float the instrument on a margin.
        //
        // Wrapped in `overlays(...)` because the shell carries tooltips
        // (the roster flag chips, the self-strip buttons). The tooltip
        // layer mounts on an `Axis::Overlay` root; without one, the
        // first hover panics rather than degrading.
        let shell = column([
            row([self.rail(), self.conversation()])
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [row([
                    dot(vs::EDITOR_GUTTER_ADDED_BG),
                    text("Connected — voice.example.com · 48 ms").caption(),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center)],
                [
                    chip("IN 62%"),
                    chip("OUT 41%"),
                    text("Opus · 48 kHz · 10 ms").caption(),
                    text("PTT: F13").caption(),
                ],
            ),
        ])
        .fill(vs::EDITOR_BG)
        .align(Align::Stretch);

        overlays(shell, [])
    }

    // Without this, every control renders in shadcn zinc at 36px.
    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Banter", viewport, Banter::new())
}
