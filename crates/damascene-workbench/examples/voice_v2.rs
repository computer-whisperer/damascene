//! Banter — a voice-chat client (Mumble/TeamSpeak genre) on the
//! workbench theme.
//!
//! A static layout study of the shape this genre always converges on: a
//! channel tree docked left with the people inside each channel nested
//! under it, a self strip pinned to the bottom of that rail, a message
//! log filling the work area, and a status bar carrying the connection
//! and the audio instrumentation.
//!
//! Everything but [`damascene_workbench::chrome`]'s `status_bar` /
//! `pane_header` / `chip` is a stock damascene widget — `text_input`,
//! `icon_button`, `avatar_initials`, `meter_with_color`, `icon` —
//! rendered through [`damascene_workbench::theme::theme`].
//!
//! Only the composer is wired (it types, and the send button clears the
//! draft). The tree, the log and the readouts are fixed content: this is
//! a mock, not an application.
//!
//! Run: `cargo run -p damascene-workbench --example voice_v2`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---- metrics --------------------------------------------------------
//
// The rail is 240px because a channel row has to hold a disclosure
// chevron, a channel glyph, a name long enough to be a real channel
// name, and a member count chip without ellipsizing the name.

/// Width of the channel rail.
const RAIL_WIDTH: f32 = 240.0;
/// Server identity block at the top of the rail — two lines of text.
const SERVER_HEADER_HEIGHT: f32 = 44.0;
/// Channel disclosure row. One rung above the user rows so the tree
/// reads as two levels without indentation alone carrying it.
const CHANNEL_ROW_HEIGHT: f32 = 24.0;
/// User row inside an expanded channel.
const USER_ROW_HEIGHT: f32 = 22.0;
/// Left inset of a user row — clears the channel row's chevron so the
/// names form their own column.
const USER_INDENT: f32 = 24.0;
/// Self strip at the foot of the rail. Taller than a list row because it
/// carries 28px icon buttons plus their focus ring.
const SELF_STRIP_HEIGHT: f32 = 40.0;
/// Channel header over the message log.
const CHANNEL_HEADER_HEIGHT: f32 = 34.0;
/// Timestamp gutter in the message log — wide enough for `HH:MM`.
const TIMESTAMP_WIDTH: f32 = 36.0;
/// Author column in the message log. Fixed rather than hugging so the
/// message bodies form a single left edge down the log.
const AUTHOR_WIDTH: f32 = 96.0;
/// Diameter of a presence dot.
const PRESENCE_DOT: f32 = 6.0;
/// Avatar in the self strip — the list-row rung, not the stock 40px.
const AVATAR_SIZE: f32 = 22.0;
/// Audio level meter in the status bar.
const LEVEL_WIDTH: f32 = 40.0;
/// Level meter thickness. Thinner than the stock meter so it reads as
/// instrumentation inside a 22px bar rather than as a progress control.
const LEVEL_HEIGHT: f32 = 4.0;

const DRAFT_KEY: &str = "composer:draft";
const SEND_KEY: &str = "composer:send";

// ---- content --------------------------------------------------------

/// The three presence states a client shows in a channel list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Presence {
    /// Transmitting right now.
    Speaking,
    /// Connected and quiet.
    Idle,
    /// Marked away by the client.
    Away,
}

impl Presence {
    fn color(self) -> Color {
        match self {
            Presence::Speaking => tokens::SUCCESS,
            Presence::Idle => vs::DESCRIPTION_FG,
            Presence::Away => tokens::WARNING,
        }
    }
}

struct User {
    name: &'static str,
    presence: Presence,
    muted: bool,
    deafened: bool,
    /// The local user — rendered at full foreground so you can find
    /// yourself in the tree.
    is_self: bool,
}

impl User {
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
        self.deafened = true;
        self.muted = true;
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
    users: &'static [User],
}

const LOBBY: &[User] = &[
    User::new("mara.kell", Presence::Idle),
    User::new("dpetrov", Presence::Idle).muted(),
];

const ENGINEERING: &[User] = &[
    User::new("ana.ruiz", Presence::Speaking),
    User::new("tom.hedlund", Presence::Idle),
    User::new("priya.n", Presence::Idle).deafened(),
    User::new("jchen", Presence::Idle),
    User::new("m.varga", Presence::Speaking).me(),
];

const DESIGN: &[User] = &[
    User::new("s.okafor", Presence::Idle),
    User::new("lu.wei", Presence::Idle).muted(),
];

const AFK: &[User] = &[User::new("wgrant", Presence::Away)];

const CHANNELS: &[Channel] = &[
    Channel {
        name: "Lobby",
        expanded: true,
        users: LOBBY,
    },
    Channel {
        name: "Engineering",
        expanded: true,
        users: ENGINEERING,
    },
    Channel {
        name: "Design",
        expanded: true,
        users: DESIGN,
    },
    Channel {
        name: "Music Room",
        expanded: false,
        users: &[],
    },
    Channel {
        name: "AFK",
        expanded: true,
        users: AFK,
    },
];

/// The channel the client is listening to. Index into [`CHANNELS`].
const ACTIVE_CHANNEL: usize = 1;

/// One line of the log. `author: None` is a system/join line — the dim
/// italic rung that every client in this genre folds into the same
/// stream as the chat.
struct Message {
    time: &'static str,
    author: Option<&'static str>,
    body: &'static str,
}

const LOG: &[Message] = &[
    Message {
        time: "13:31",
        author: None,
        body: "You joined the channel",
    },
    Message {
        time: "13:32",
        author: Some("tom.hedlund"),
        body: "morning. standup notes are in the pad, I'm skipping the call — headset \
               died again",
    },
    Message {
        time: "13:33",
        author: Some("m.varga"),
        body: "there's a spare USB one in the drawer by the printer",
    },
    Message {
        time: "13:35",
        author: Some("ana.ruiz"),
        body: "before I forget: the 0.9.1 build on the fleet is still pinned to the old \
               mixer. don't chase bugs against it",
    },
    Message {
        time: "13:36",
        author: Some("priya.n"),
        body: "that explains the double-audio reports from yesterday",
    },
    Message {
        time: "13:38",
        author: Some("ana.ruiz"),
        body: "yeah. two mixer instances, both draining the same ring. it's fixed on \
               main, not on the tag",
    },
    Message {
        time: "13:41",
        author: Some("mara.kell"),
        body: "can we get 0.9.2 out this week? support has four tickets open on it",
    },
    Message {
        time: "13:43",
        author: Some("m.varga"),
        body: "that's the plan, assuming the jitter buffer work lands cleanly",
    },
    Message {
        time: "13:48",
        author: None,
        body: "priya.n deafened",
    },
    Message {
        time: "13:54",
        author: None,
        body: "jchen joined the channel",
    },
    Message {
        time: "13:56",
        author: Some("ana.ruiz"),
        body: "resampler underrun is fixed — 48k in, 48k out, no more clicks when you \
               switch channels mid-talk",
    },
    Message {
        time: "13:57",
        author: Some("tom.hedlund"),
        body: "nice. does that cover the opus frame-size mismatch too?",
    },
    Message {
        time: "13:58",
        author: Some("ana.ruiz"),
        body: "partly. 20 ms frames are clean now; 10 ms still drops the first packet \
               after a rejoin",
    },
    Message {
        time: "14:01",
        author: Some("priya.n"),
        body: "I can reproduce the 10 ms case on staging if you want a pcap",
    },
    Message {
        time: "14:02",
        author: Some("jchen"),
        body: "a capture would help. while we're in there, can we raise the jitter \
               buffer floor to 40 ms?",
    },
    Message {
        time: "14:03",
        author: None,
        body: "s.okafor moved to Design",
    },
    Message {
        time: "14:05",
        author: Some("ana.ruiz"),
        body: "40 ms floor is right for the mobile clients. desktop can stay at 20.",
    },
    Message {
        time: "14:06",
        author: Some("tom.hedlund"),
        body: "I'll take the jitter buffer change — PR before end of day",
    },
    Message {
        time: "14:09",
        author: Some("m.varga"),
        body: "thanks. I'll cut 0.9.2 once it lands and the pcap is attached to #418",
    },
    Message {
        time: "14:11",
        author: Some("mara.kell"),
        body: "heads up: server restart at 15:00, expect a ~10 s drop",
    },
    Message {
        time: "14:13",
        author: Some("jchen"),
        body: "pcap is up — engineering/captures/2026-07-24-rejoin.pcapng",
    },
    Message {
        time: "14:14",
        author: None,
        body: "wgrant is now away",
    },
    Message {
        time: "14:16",
        author: Some("ana.ruiz"),
        body: "got it. the first packet after a rejoin still carries the sequence number \
               from the previous session, so the buffer throws it away",
    },
    Message {
        time: "14:18",
        author: Some("priya.n"),
        body: "so reset the sequence on rejoin rather than raise the floor?",
    },
    Message {
        time: "14:19",
        author: Some("ana.ruiz"),
        body: "both. the floor helps the mobile clients either way",
    },
    Message {
        time: "14:21",
        author: Some("tom.hedlund"),
        body: "agreed — I'll fold the sequence reset into the same PR",
    },
];

const SERVER_NAME: &str = "Northgate Studios";
const SERVER_HOST: &str = "voice.northgate.dev";
const CHANNEL_TOPIC: &str = "audio pipeline · client 0.9.x · pcaps in #418";

// ---- app ------------------------------------------------------------

struct Banter {
    draft: String,
    selection: Selection,
}

impl Banter {
    fn new() -> Self {
        Self {
            draft: String::new(),
            selection: Selection::default(),
        }
    }

    /// Server identity block. Two lines — the server's display name and
    /// the host you are actually connected to, which are different
    /// strings in every client in this genre and get confused whenever
    /// only one of them is shown.
    fn server_header(&self) -> El {
        row([
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
            icon(IconName::Wifi)
                .icon_size(tokens::ICON_XS)
                .color(tokens::SUCCESS),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_2))
        .fill(vs::SIDE_BAR_BG)
        .border_b()
        .border_color(vs::SIDE_BAR_BORDER)
        .height(Size::Fixed(SERVER_HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The channel tree: a flat column of channel rows, each followed by
    /// its members at one indent level.
    ///
    /// Hand-rolled rather than built from a widget, because core has no
    /// tree view — `sidebar_menu_button` is a single-level nav row with
    /// no disclosure affordance, no indent rung and no trailing slot.
    /// See the "Deferred deliberately" list in the crate docs.
    fn channel_tree(&self) -> El {
        let mut rows: Vec<El> = Vec::new();
        for (index, channel) in CHANNELS.iter().enumerate() {
            rows.push(channel_row(channel, index == ACTIVE_CHANNEL));
            if channel.expanded {
                rows.extend(channel.users.iter().map(user_row));
            }
        }

        column(rows)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .padding(Sides::y(tokens::SPACE_1))
            .align(Align::Stretch)
            .scrollable()
    }

    /// The self strip — who you are on this server, and the three
    /// toggles a voice client puts within one click at all times.
    fn self_strip(&self) -> El {
        let me = ENGINEERING
            .iter()
            .find(|u| u.is_self)
            .expect("the local user is in the active channel");

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
                text("Transmitting")
                    .caption()
                    .text_color(tokens::SUCCESS)
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Hug),
            icon_button(IconName::Mic).key("self:mic").ghost(),
            icon_button(IconName::Headphones)
                .key("self:deafen")
                .ghost(),
            icon_button(IconName::Settings).key("self:settings").ghost(),
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
        let population: usize = CHANNELS.iter().map(|c| c.users.len()).sum();

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

    /// Channel header. Name, topic, and the population on the right —
    /// the editor-group header rung, so it steps above the log the way
    /// a tab strip steps above an editor.
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
            text(CHANNEL_TOPIC)
                .caption()
                .text_color(vs::DESCRIPTION_FG)
                .ellipsis()
                .width(Size::Fill(1.0)),
            icon(IconName::Users)
                .icon_size(tokens::ICON_XS)
                .color(vs::DESCRIPTION_FG),
            text(format!("{} in channel", channel.users.len()))
                .caption()
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

    /// The log. `pin_end` is the chat-log scroll policy: the viewport
    /// opens on its tail and follows new lines until the user scrolls up.
    fn message_log(&self) -> El {
        column(LOG.iter().map(message_row).collect::<Vec<_>>())
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
                DRAFT_KEY,
                &self.draft,
                &self.selection,
                TextInputOpts::default().placeholder("Message Engineering…"),
            )
            .width(Size::Fill(1.0)),
            icon_button(IconName::Send)
                .key(SEND_KEY)
                .primary()
                .tooltip("Send message"),
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

// ---- row recipes ----------------------------------------------------

/// A presence dot. `Kind::Custom` rather than `divider()`: this is a
/// state indicator, not a rule, and the inspector should say so.
fn presence_dot(color: Color) -> El {
    El::new(Kind::Custom("presence-dot"))
        .fill(color)
        .radius(PRESENCE_DOT / 2.0)
        .width(Size::Fixed(PRESENCE_DOT))
        .height(Size::Fixed(PRESENCE_DOT))
}

fn channel_row(channel: &Channel, active: bool) -> El {
    let chevron = if channel.expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let name_color = if active {
        vs::LIST_ACTIVE_SELECTION_FG
    } else {
        vs::SIDE_BAR_FG
    };

    let row = row([
        icon(chevron)
            .icon_size(tokens::ICON_XS)
            .color(vs::DESCRIPTION_FG),
        icon(IconName::Volume2).icon_size(tokens::ICON_XS).color(
            if active {
                vs::TEXT_LINK_FG
            } else {
                vs::DESCRIPTION_FG
            },
        ),
        text(channel.name)
            .caption()
            .font_weight(FontWeight::Medium)
            .text_color(name_color)
            .ellipsis()
            .width(Size::Fill(1.0)),
        chip(format!("{}", channel.users.len())),
    ])
    .gap(tokens::SPACE_1)
    .padding(Sides::x(tokens::SPACE_2))
    .height(Size::Fixed(CHANNEL_ROW_HEIGHT))
    .width(Size::Fill(1.0))
    .align(Align::Center);

    if active {
        row.fill(vs::LIST_ACTIVE_SELECTION_BG)
    } else {
        row
    }
}

fn user_row(user: &User) -> El {
    let mut glyphs: Vec<El> = Vec::new();
    if user.muted {
        glyphs.push(
            icon(IconName::MicOff)
                .icon_size(tokens::ICON_XS)
                .color(vs::ERROR_FG),
        );
    }
    if user.deafened {
        glyphs.push(
            icon(IconName::HeadphoneOff)
                .icon_size(tokens::ICON_XS)
                .color(vs::ERROR_FG),
        );
    }

    let name_color = if user.is_self {
        vs::FOREGROUND
    } else {
        vs::SIDE_BAR_FG
    };

    row([
        presence_dot(user.presence.color()),
        text(user.name)
            .caption()
            .text_color(name_color)
            .ellipsis()
            .width(Size::Fill(1.0)),
        row(glyphs)
            .gap(tokens::SPACE_1)
            .width(Size::Hug)
            .align(Align::Center),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides {
        left: USER_INDENT,
        right: tokens::SPACE_2,
        top: 0.0,
        bottom: 0.0,
    })
    .height(Size::Fixed(USER_ROW_HEIGHT))
    .width(Size::Fill(1.0))
    .align(Align::Center)
}

/// One log line: fixed timestamp gutter, fixed author column, wrapped
/// body. `Align::Start` so a wrapped body's first line sits level with
/// its timestamp instead of centering the whole block against it.
fn message_row(message: &Message) -> El {
    let body: El = match message.author {
        Some(author) => row([
            text(author)
                .caption()
                .font_weight(FontWeight::Semibold)
                .text_color(vs::TEXT_LINK_FG)
                .ellipsis()
                .width(Size::Fixed(AUTHOR_WIDTH)),
            text(message.body)
                .body()
                .text_color(vs::EDITOR_FG)
                .wrap_text()
                .width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Start),
        // System lines skip the author column entirely and run dim and
        // italic across the full measure — the genre's convention for
        // "the server said this, not a person".
        None => text(message.body)
            .caption()
            .italic()
            .text_color(vs::DESCRIPTION_FG)
            .wrap_text()
            .width(Size::Fill(1.0)),
    };

    row([
        text(message.time)
            .caption()
            .mono()
            .tabular_numerals()
            .text_color(vs::DESCRIPTION_FG)
            .nowrap_text()
            .width(Size::Fixed(TIMESTAMP_WIDTH)),
        body,
    ])
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
        // A bare column root, not `page()` — a client shell is
        // full-bleed; window padding would float the whole instrument on
        // a margin.
        column([
            row([
                self.rail(),
                column([
                    self.channel_header(),
                    self.message_log(),
                    self.composer(),
                ])
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
                        presence_dot(tokens::SUCCESS),
                        text(format!("Connected — {SERVER_HOST} · 48 ms")).caption(),
                    ])
                    .gap(tokens::SPACE_1)
                    .width(Size::Hug)
                    .align(Align::Center),
                    text("TLS · cert pinned").caption(),
                ],
                [
                    level("IN", 0.62, tokens::SUCCESS),
                    level("OUT", 0.38, vs::TEXT_LINK_FG),
                    chip("Opus 48 kHz"),
                    chip("PTT · F13"),
                ],
            ),
        ])
        .fill(vs::EDITOR_BG)
        .align(Align::Stretch)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if text_input::apply_event(&mut self.draft, &mut self.selection, &event, DRAFT_KEY) {
            return;
        }
        // Nothing is sent anywhere — the draft just clears, which is the
        // honest amount of behavior for a layout study.
        if event.is_click_or_activate(SEND_KEY) {
            self.draft.clear();
            self.selection = Selection::default();
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
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
