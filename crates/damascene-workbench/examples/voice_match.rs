//! "Banter" — a voice-chat client, built to match a designer's target
//! screenshot (`references/workbench-validation/voice/reference.png`).
//!
//! Where `examples/voice.rs` is a *genre* test ("does the workbench
//! vocabulary survive a roster-and-levels app at all"), this one is a
//! *fidelity* test: reproduce one specific 1280×800 comp as closely as
//! the stock vocabulary allows, and let whatever refuses to close be the
//! finding. The regions and their measured metrics, read off the comp:
//!
//! | region | metric |
//! |---|---|
//! | title strip | 30px, `titleBar.activeBackground`, under-rule |
//! | activity rail | 44px wide, `activityBar.background`, right rule |
//! | side bar | 240px wide, `sideBar.background`, right rule |
//! | server header | 48px, under-rule |
//! | tree rows | 22px, 18px indent per depth |
//! | channel header | 44px, on `editor.background`, under-rule |
//! | self strip | 48px, over-rule |
//! | status bar | 22px |
//!
//! Everything structural is stock: `pane_header`, `chip`, `status_bar`
//! from [`damascene_workbench::chrome`]; `icon_button`, `avatar_initials`,
//! `text_input_with`, `button_with_icon`, `vertical_separator`,
//! `menubar`, `code_block_chrome`, `text_runs` from core.
//!
//! # Where the comp outruns the library
//!
//! Recorded here rather than faked, because the gap list is the point:
//!
//! 1. **Icon vocabulary — CLOSED.** This was the finding that drove the
//!    expansion of [`IconName`] from 26 to 57 members: the vocabulary held
//!    none of the glyphs a voice client is *made* of, and every one below
//!    was a stock name pressed into a role it did not draw. `Mic`,
//!    `MicOff`, `Headphones`, `HeadphoneOff`, `Volume2`, `VolumeX`,
//!    `MessageSquare`, `ScreenShare`, `Lock`, `Send`, `Paperclip`,
//!    `Smile`, `Wifi` and `LogOut` are all built in now, and this file
//!    draws them. What is still missing is only the window caption set
//!    (`Minimize` / `Maximize`), noted at its one call site in the title
//!    strip.
//! 2. **Syntax colors.** [`vs`] is the *workbench* half of a VS Code
//!    theme; the `editor.tokenColorCustomizations` half (comment green,
//!    keyword blue, control-flow magenta, number sage) has no tokens, so
//!    the code card carries local constants.
//! 3. **No level meter.** A segmented dB meter is the single most
//!    genre-defining widget in this comp and there is no recipe for it;
//!    [`meter`] hand-rolls one from 2px rects, the same way `voice.rs`
//!    hand-rolls [`dot`].
//! 4. **No outlined chip.** `chrome::chip` is solid-filled; the comp's
//!    `admin` / `you` / `Enter` tags are 1px-outlined and transparent, so
//!    [`tag`] and [`kbd`] hand-roll that variant.
//!
//! Run: `cargo run -p damascene-workbench --example voice_match`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---------------------------------------------------------------------
// Metrics — measured off the comp, not guessed.
// ---------------------------------------------------------------------

/// Activity rail width (comp: content x0–42, rule at x43).
const RAIL_WIDTH: f32 = 44.0;
/// Side bar width including its right rule (comp: x44–283).
const SIDE_WIDTH: f32 = 240.0;
/// Tree row pitch (comp: 22px, the same rung as the bars).
const ROW_HEIGHT: f32 = 22.0;
/// Indent step per tree depth (comp: 18px).
const INDENT: f32 = 18.0;
/// Server header block (comp: y30–77, rule at y78).
const SERVER_HEADER_HEIGHT: f32 = 48.0;
/// Self strip (comp: rule at y730, content to y777).
const SELF_STRIP_HEIGHT: f32 = 48.0;
/// Channel header (comp: y30–72, rule at y73). Taller than
/// [`vs::PANE_HEADER_HEIGHT`] because it carries a topic line.
const CHANNEL_HEADER_HEIGHT: f32 = 44.0;
/// Timestamp gutter (comp: text at x303, body at x346, log pad 16).
const TIME_COL: f32 = 40.0;
/// Code card width (comp: x346–985).
const CODE_CARD_WIDTH: f32 = 640.0;

// ---------------------------------------------------------------------
// Colors the workbench token set does not carry.
//
// GAP (LIBRARY): `damascene_workbench::tokens` is calibrated from the
// Dark Modern *workbench* colors. The comp also uses Dark+ **syntax**
// colors, which have no constants — there is no `vs::TOKEN_COMMENT`,
// `vs::TOKEN_KEYWORD`, `vs::TOKEN_NUMBER`, `vs::TOKEN_TYPE`. These are
// the upstream Dark+ `tokenColors` values, transcribed.
// ---------------------------------------------------------------------

/// Dark+ `comment` — `#6A9955`.
const TOK_COMMENT: Color = Color::srgb_u8a(106, 153, 85, 255);
/// Dark+ `keyword.other` / storage — `#569CD6`.
const TOK_KEYWORD: Color = Color::srgb_u8a(86, 156, 214, 255);
/// Dark+ `keyword.control` — `#C586C0`.
const TOK_CONTROL: Color = Color::srgb_u8a(197, 134, 192, 255);
/// Dark+ `constant.numeric` — `#B5CEA8`.
const TOK_NUMBER: Color = Color::srgb_u8a(181, 206, 168, 255);
/// Dark+ `entity.name.type` — `#4EC9B0`.
const TOK_TYPE: Color = Color::srgb_u8a(78, 201, 176, 255);
/// Dark+ `variable` — `#9CDCFE`.
const TOK_VAR: Color = Color::srgb_u8a(156, 220, 254, 255);

/// `symbolIcon.*` cyan — the comp's brand accent (logo, code-card edge).
const ACCENT_CYAN: Color = Color::srgb_u8a(79, 193, 255, 255);
/// The comp's code-card fill, one step above `editor.background`.
const CODE_CARD_BG: Color = Color::srgb_u8a(36, 36, 36, 255);
/// The comp's composer trough — darker than `input.background` (`#313131`)
/// because it is a docked well, not a floating field.
const COMPOSER_BG: Color = Color::srgb_u8a(42, 42, 42, 255);
/// Unlit meter segment.
const METER_OFF: Color = Color::srgb_u8a(75, 75, 75, 255);
/// Offline / disconnected presence.
const PRESENCE_OFFLINE: Color = Color::srgb_u8a(110, 110, 110, 255);

// ---------------------------------------------------------------------
// Small recipes core does not have.
// ---------------------------------------------------------------------

/// A presence dot. Same shape `voice.rs` uses — the smallest thing in
/// the shell that carries state.
fn dot(color: Color) -> El {
    column(Vec::<El>::new())
        .width(Size::Fixed(8.0))
        .height(Size::Fixed(8.0))
        .radius(tokens::RADIUS_PILL)
        .fill(color)
}

/// A segmented level meter.
///
/// GAP (LIBRARY): there is no `meter()` / `level_meter()` recipe and
/// `progress()` is a continuous track, so this is 2px rects in a row.
/// `lit` segments light, and from `hot` they light amber instead of green.
fn meter(total: usize, lit: usize, hot: usize, width: f32, height: f32) -> El {
    row((0..total)
        .map(|i| {
            let c = if i >= lit {
                METER_OFF
            } else if i >= hot {
                vs::CHAT_EDITED_FILE_FG
            } else {
                vs::EDITOR_GUTTER_ADDED_BG
            };
            column(Vec::<El>::new())
                .width(Size::Fixed(width))
                .height(Size::Fixed(height))
                .fill(c)
        })
        .collect::<Vec<El>>())
    .gap(1.0)
    .align(Align::Center)
}

/// A 1px-outlined tag — `admin`, `you`.
///
/// GAP (LIBRARY): `chrome::chip` is solid `badge.background`; stock
/// `badge()` is 16px+ (the `Xxs` chrome rung) on a 6px radius with a
/// tinted fill. The comp wants a 15px transparent outline, which is
/// neither.
fn tag(label: &str, c: Color) -> El {
    row([text(label).caption().font_size(10.0).color(c)])
        .stroke(c.with_alpha_u8(110))
        .radius(vs::RADIUS)
        .height(Size::Fixed(15.0))
        .width(Size::Hug)
        .padding(Sides::x(tokens::SPACE_1))
        .align(Align::Center)
        .justify(Justify::Center)
}

/// A keycap. Same shape as [`tag`] but mono, for the composer hint row.
///
/// GAP (LIBRARY): no `kbd()` widget. `command_shortcut` /
/// `menubar_shortcut` are dim un-boxed text, not keycaps.
fn kbd(label: &str) -> El {
    row([text(label).mono().caption().font_size(11.0).color(vs::DESCRIPTION_FG)])
        .fill(COMPOSER_BG)
        .stroke(METER_OFF)
        .radius(vs::RADIUS)
        .height(Size::Fixed(17.0))
        .width(Size::Hug)
        .padding(Sides::x(tokens::SPACE_1))
        .align(Align::Center)
        .justify(Justify::Center)
}

/// A message-log author tile — `avatar_initials` squared down to 16px.
fn tile(initials: &str, fg: Color, size: f32, radius: f32) -> El {
    avatar_initials(initials)
        .width(Size::Fixed(size))
        .height(Size::Fixed(size))
        .radius(radius)
        .fill(fg.with_alpha_u8(38))
        .stroke(fg.with_alpha_u8(70))
        .color(fg)
        .font_size(if size <= 18.0 { 8.5 } else { 11.0 })
}

// ---------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------

/// What a roster dot says about a participant.
#[derive(Clone, Copy, PartialEq)]
enum Presence {
    /// Connected, quiet.
    Online,
    /// Connected, marked away.
    Away,
    /// Connected but not present at the machine.
    Offline,
}

impl Presence {
    fn color(self) -> Color {
        match self {
            Presence::Online => vs::EDITOR_GUTTER_ADDED_BG,
            Presence::Away => vs::CHAT_EDITED_FILE_FG,
            Presence::Offline => PRESENCE_OFFLINE,
        }
    }
}

/// The trailing state marker on a roster row.
#[derive(Clone, Copy, PartialEq)]
enum Marker {
    /// Nothing to say.
    None,
    /// Transmitting right now.
    Speaking,
    /// Sharing their screen.
    Sharing,
    /// Microphone muted.
    Muted,
    /// Output muted, which implies the microphone is too.
    Deafened,
    /// This is you.
    You,
}

impl Marker {
    fn el(self, owner: &str) -> Option<El> {
        let marked = |name: IconName, c: Color, tip: &str| {
            Some(
                icon(name)
                    .icon_size(tokens::ICON_XS)
                    .color(c)
                    .key(format!("{owner}:mark"))
                    .tooltip(tip),
            )
        };
        match self {
            Marker::None => None,
            Marker::Speaking => marked(
                IconName::Volume2,
                vs::EDITOR_GUTTER_ADDED_BG,
                "Transmitting",
            ),
            Marker::Sharing => marked(
                IconName::ScreenShare,
                vs::EDITOR_GUTTER_ADDED_BG,
                "Sharing screen",
            ),
            Marker::Muted => marked(IconName::MicOff, vs::ERROR_FG, "Microphone muted"),
            Marker::Deafened => marked(
                IconName::HeadphoneOff,
                vs::ERROR_FG,
                "Deafened — output muted",
            ),
            Marker::You => Some(tag("you", vs::DESCRIPTION_FG)),
        }
    }
}

struct Member {
    name: &'static str,
    presence: Presence,
    marker: Marker,
}

fn member(name: &'static str, presence: Presence, marker: Marker) -> Member {
    Member {
        name,
        presence,
        marker,
    }
}

struct Channel {
    name: &'static str,
    depth: usize,
    expanded: Option<bool>,
    count: usize,
    private: bool,
    members: Vec<Member>,
}

/// The author identity carried by a message header: display name, tile
/// initials, and the stable per-author color.
struct Author {
    name: &'static str,
    initials: &'static str,
    color: Color,
    role: Option<&'static str>,
}

fn author(
    name: &'static str,
    initials: &'static str,
    color: Color,
    role: Option<&'static str>,
) -> Author {
    Author {
        name,
        initials,
        color,
        role,
    }
}

struct Banter {
    draft: String,
    selection: Selection,
    channels: Vec<Channel>,
    away: Vec<Channel>,
}

impl Banter {
    fn new() -> Self {
        Self {
            draft: String::new(),
            selection: Selection::default(),
            channels: vec![
                Channel {
                    name: "Lobby",
                    depth: 0,
                    expanded: Some(true),
                    count: 2,
                    private: false,
                    members: vec![
                        member("marta.vogel", Presence::Online, Marker::None),
                        member("dan.okafor", Presence::Online, Marker::Muted),
                    ],
                },
                Channel {
                    name: "Build Room",
                    depth: 0,
                    expanded: Some(true),
                    count: 4,
                    private: false,
                    members: vec![
                        member("priya.raghavan", Presence::Online, Marker::Speaking),
                        member("tomas.lindqvist", Presence::Online, Marker::Sharing),
                        member("aiko.tanaka", Presence::Online, Marker::Deafened),
                        member("ben.kepner", Presence::Online, Marker::You),
                    ],
                },
                Channel {
                    name: "Pairing",
                    depth: 0,
                    expanded: Some(true),
                    count: 1,
                    private: false,
                    members: Vec::new(),
                },
                Channel {
                    name: "Pairing A",
                    depth: 1,
                    expanded: Some(true),
                    count: 1,
                    private: false,
                    members: vec![member("sam.whitlock", Presence::Online, Marker::Muted)],
                },
                Channel {
                    name: "Pairing B",
                    depth: 1,
                    expanded: Some(false),
                    count: 0,
                    private: false,
                    members: Vec::new(),
                },
                Channel {
                    name: "Design Review",
                    depth: 0,
                    expanded: Some(false),
                    count: 2,
                    private: true,
                    members: Vec::new(),
                },
                Channel {
                    name: "Release War Room",
                    depth: 0,
                    expanded: Some(true),
                    count: 1,
                    private: false,
                    members: vec![member("hana.mbeki", Presence::Away, Marker::Muted)],
                },
            ],
            away: vec![Channel {
                name: "AFK",
                depth: 0,
                expanded: Some(true),
                count: 3,
                private: false,
                members: vec![
                    member("jonas.ferrer", Presence::Away, Marker::Deafened),
                    member("rui.nakamura", Presence::Away, Marker::None),
                    member("elena.brandt", Presence::Offline, Marker::Muted),
                ],
            }],
        }
    }

    // -----------------------------------------------------------------
    // Title strip
    // -----------------------------------------------------------------

    /// Title strip. A `stack` rather than spacer-spacer-spacer, because
    /// the comp centers the document title on the *window*, not on the
    /// space left over by the menu — a row of spacers would drift right.
    fn title_strip(&self) -> El {
        let controls = row([
            // stand-in: no `IconName::Minimize` / `Maximize`; the close
            // glyph exists (`IconName::X`) but is drawn as text here so
            // the three caption buttons share one cell metric.
            self.caption_button("win:min", "\u{2500}"),
            self.caption_button("win:max", "\u{25A1}"),
            self.caption_button("win:close", "\u{2715}"),
        ])
        .gap(0.0)
        .align(Align::Center);

        let bar = row([
            icon(IconName::Activity)
                .icon_size(tokens::ICON_SM)
                .color(ACCENT_CYAN),
            text("Banter")
                .label()
                .font_weight(FontWeight::Bold)
                .color(vs::TITLE_BAR_ACTIVE_FG),
            menubar([
                // GAP (LIBRARY): `menubar_trigger`'s only static
                // highlight is `open: true`, which paints the Solid
                // accent (VS Code's blue). The comp shows the *hover*
                // wash (`button.secondaryHoverBackground`), which no
                // parameter reaches — so it is restated as a fill.
                menubar_trigger("menu", "server", "Server", false)
                    .fill(vs::BUTTON_SECONDARY_HOVER_BG),
                menubar_trigger("menu", "channel", "Channel", false),
                menubar_trigger("menu", "audio", "Audio", false),
                menubar_trigger("menu", "view", "View", false),
                menubar_trigger("menu", "help", "Help", false),
            ])
            .fill(vs::TITLE_BAR_ACTIVE_BG)
            .stroke(vs::TITLE_BAR_ACTIVE_BG)
            .height(Size::Fixed(24.0))
            .padding(Sides::all(0.0)),
            spacer(),
            controls,
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::left(tokens::SPACE_2))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Center);

        let title = row([
            text("Build Room — Northwind Collective")
                .caption()
                .color(vs::DESCRIPTION_FG),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Center)
        .justify(Justify::Center);

        stack([title, bar])
            .fill(vs::TITLE_BAR_ACTIVE_BG)
            .border_b()
            .border_color(vs::TITLE_BAR_BORDER)
            .height(Size::Fixed(vs::TITLE_BAR_HEIGHT))
            .width(Size::Fill(1.0))
            .align(Align::Stretch)
    }

    fn caption_button(&self, key: &str, glyph: &str) -> El {
        row([text(glyph).caption().color(vs::DESCRIPTION_FG)])
            .key(key.to_string())
            .width(Size::Fixed(46.0))
            .height(Size::Fixed(vs::TITLE_BAR_HEIGHT))
            .align(Align::Center)
            .justify(Justify::Center)
            .cursor(Cursor::Pointer)
    }

    // -----------------------------------------------------------------
    // Activity rail
    // -----------------------------------------------------------------

    /// A rail item: 44px square, with the active one carrying VS Code's
    /// 2px left indicator (`activityBar.activeBorder`).
    fn rail_item(&self, key: &str, name: IconName, active: bool, tip: &str) -> El {
        let item = row([icon(name).icon_size(tokens::ICON_MD).color(if active {
            vs::ACTIVITY_BAR_FG
        } else {
            vs::ACTIVITY_BAR_INACTIVE_FG
        })])
        .key(key.to_string())
        .width(Size::Fill(1.0))
        .height(Size::Fixed(44.0))
        .align(Align::Center)
        .justify(Justify::Center)
        .cursor(Cursor::Pointer)
        .tooltip(tip);

        if active {
            // GAP (LIBRARY): `border_l()` is fixed at 1px unless you drop
            // to `.border_widths(...)`; the comp's indicator is 2px.
            item.border_l()
                .border_widths(Sides::left(2.0))
                .border_color(vs::TITLE_BAR_ACTIVE_FG)
        } else {
            item
        }
    }

    fn activity_rail(&self) -> El {
        column([
            self.rail_item("rail:voice", IconName::Volume2, true, "Voice"),
            // The unread badge overlaps the icon's lower-left, which is
            // what `stack` is for.
            stack([
                self.rail_item("rail:chat", IconName::MessageSquare, false, "Messages"),
                row([chip("3").fill(vs::BUTTON_BG).color(vs::BUTTON_FG)])
                    .width(Size::Fill(1.0))
                    .height(Size::Fill(1.0))
                    .padding(Sides::xy(tokens::SPACE_1, 12.0))
                    .align(Align::End)
                    .justify(Justify::Start),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fixed(44.0)),
            self.rail_item("rail:notify", IconName::Bell, false, "Notifications"),
            row([hairline()])
                .padding(Sides::xy(12.0, tokens::SPACE_1))
                .width(Size::Fill(1.0)),
            self.rail_server("NW", true),
            self.rail_server("ML", false),
            self.rail_server("HX", false),
            self.rail_item("rail:add", IconName::Plus, false, "Add a server"),
            spacer(),
            self.rail_item("rail:audio", IconName::Headphones, false, "Audio devices"),
            self.rail_item("rail:prefs", IconName::Settings, false, "Preferences"),
        ])
        .gap(0.0)
        .fill(vs::ACTIVITY_BAR_BG)
        .border_r()
        .border_color(vs::ACTIVITY_BAR_BORDER)
        .width(Size::Fixed(RAIL_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// A server tile in the rail — a squared-off `avatar_initials`.
    fn rail_server(&self, initials: &'static str, active: bool) -> El {
        let color = if active {
            vs::FOCUS_BORDER
        } else {
            vs::DESCRIPTION_FG
        };
        row([tile(initials, color, 30.0, 6.0)])
            .key(format!("server:{initials}"))
            .width(Size::Fill(1.0))
            .height(Size::Fixed(44.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .cursor(Cursor::Pointer)
    }

    // -----------------------------------------------------------------
    // Side bar
    // -----------------------------------------------------------------

    fn server_header(&self) -> El {
        row([
            column([
                text("Northwind Collective")
                    .label()
                    .font_weight(FontWeight::Semibold)
                    .ellipsis()
                    .color(vs::TAB_ACTIVE_FG),
                text("voice.example.com:64738")
                    .mono()
                    .caption()
                    .font_size(10.0)
                    .ellipsis()
                    .color(vs::DESCRIPTION_FG),
            ])
            .gap(1.0)
            .width(Size::Fill(1.0)),
            icon_button(IconName::Search)
                .key("server:search")
                .ghost()
                .tooltip("Search this server"),
            icon_button(IconName::MoreHorizontal)
                .key("server:more")
                .ghost()
                .tooltip("Server actions"),
        ])
        .gap(tokens::SPACE_1)
        .padding(Sides::x(tokens::SPACE_3))
        .width(Size::Fill(1.0))
        .height(Size::Fixed(SERVER_HEADER_HEIGHT))
        .border_b()
        .border_color(vs::SIDE_BAR_BORDER)
        .align(Align::Center)
    }

    fn channel_row(&self, channel: &Channel, active: bool) -> El {
        let chevron = match channel.expanded {
            Some(true) => IconName::ChevronDown,
            _ => IconName::ChevronRight,
        };
        let mut children = vec![
            icon(chevron)
                .icon_size(tokens::ICON_XS)
                .color(vs::DESCRIPTION_FG),
            // Every channel in this genre is a voice channel and wears a
            // speaker glyph.
            icon(IconName::Volume2)
                .icon_size(tokens::ICON_XS)
                .color(if active {
                    vs::TAB_ACTIVE_FG
                } else {
                    vs::DESCRIPTION_FG
                }),
            text(channel.name)
                .label()
                .font_weight(if active {
                    FontWeight::Semibold
                } else {
                    FontWeight::Regular
                })
                .ellipsis()
                .width(Size::Fill(1.0))
                .color(if active {
                    vs::TAB_ACTIVE_FG
                } else {
                    vs::SIDE_BAR_FG
                }),
        ];
        if channel.private {
            children.push(
                icon(IconName::Lock)
                    .icon_size(tokens::ICON_XS)
                    .color(vs::DESCRIPTION_FG),
            );
        }
        children.push(
            text(format!("{}", channel.count))
                .mono()
                .caption()
                .tabular_numerals()
                .color(if active {
                    vs::TAB_ACTIVE_FG
                } else {
                    vs::DESCRIPTION_FG
                }),
        );

        let row = row(children)
            .key(format!("channel:{}", channel.name))
            .gap(tokens::SPACE_1)
            .padding(Sides::x(tokens::SPACE_2))
            .pl(tokens::SPACE_2 - 2.0 + INDENT * channel.depth as f32)
            .height(Size::Fixed(ROW_HEIGHT))
            .width(Size::Fill(1.0))
            .align(Align::Center)
            .focusable()
            .cursor(Cursor::Pointer);

        if active {
            row.fill(vs::LIST_ACTIVE_SELECTION_BG)
                .border_l()
                .border_widths(Sides::left(2.0))
                .border_color(vs::FOCUS_BORDER)
        } else {
            row
        }
    }

    fn member_row(&self, channel: &Channel, m: &Member, active: bool) -> El {
        let key = format!("member:{}:{}", channel.name, m.name);
        let mut children = vec![
            dot(m.presence.color()),
            text(m.name)
                .label()
                .font_weight(if active && m.marker == Marker::You {
                    FontWeight::Semibold
                } else {
                    FontWeight::Regular
                })
                .ellipsis()
                .width(Size::Fill(1.0))
                .color(if active {
                    vs::EDITOR_FG
                } else {
                    vs::DESCRIPTION_FG
                }),
        ];
        children.extend(m.marker.el(&key));

        row(children)
            .key(key)
            .gap(tokens::SPACE_2)
            .padding(Sides::x(tokens::SPACE_2))
            .pl(tokens::SPACE_2 + 34.0 + INDENT * channel.depth as f32)
            .height(Size::Fixed(ROW_HEIGHT))
            .width(Size::Fill(1.0))
            .align(Align::Center)
            .cursor(Cursor::Pointer)
    }

    /// Flatten a channel list into rows. `active` names the joined
    /// channel, which is the one row in the whole rail that is filled.
    fn tree(&self, channels: &[Channel], active: &str) -> Vec<El> {
        let mut rows = Vec::new();
        for c in channels {
            let is_active = c.name == active;
            rows.push(self.channel_row(c, is_active));
            if c.expanded == Some(true) {
                rows.extend(c.members.iter().map(|m| self.member_row(c, m, is_active)));
            }
        }
        rows
    }

    /// The "this is you" strip pinned under the roster.
    fn self_strip(&self) -> El {
        row([
            stack([
                tile("BK", vs::CHAT_EDITED_FILE_FG, 28.0, 4.0),
                row([dot(vs::EDITOR_GUTTER_ADDED_BG)])
                    .width(Size::Fill(1.0))
                    .height(Size::Fill(1.0))
                    .align(Align::End)
                    .justify(Justify::End),
            ])
            .width(Size::Fixed(28.0))
            .height(Size::Fixed(28.0)),
            column([
                text("ben.kepner")
                    .caption()
                    .font_weight(FontWeight::Semibold)
                    .ellipsis()
                    .color(vs::EDITOR_FG),
                row([
                    meter(12, 7, 6, 3.0, 8.0),
                    text("-18 dB")
                        .mono()
                        .caption()
                        .font_size(10.0)
                        .tabular_numerals()
                        .color(vs::DESCRIPTION_FG),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0)),
            // `.secondary()`, not `.ghost()`: the comp shows this one
            // control carrying a resting fill, because it is the toggle
            // whose state the user is always tracking.
            icon_button(IconName::Mic)
                .key("self:mic")
                .secondary()
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
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_2))
        .width(Size::Fill(1.0))
        .height(Size::Fixed(SELF_STRIP_HEIGHT))
        .border_t()
        .border_color(vs::SIDE_BAR_BORDER)
        .align(Align::Center)
    }

    fn side_bar(&self) -> El {
        let mut roster = vec![pane_header("CHANNELS", [text("11 online").mono().caption()])];
        roster.extend(self.tree(&self.channels, "Build Room"));
        roster.push(row([hairline()]).padding(Sides::y(tokens::SPACE_1)));
        roster.push(pane_header("AWAY", [text("3").mono().caption()]));
        roster.extend(self.tree(&self.away, "Build Room"));

        column([
            self.server_header(),
            column(roster)
                .gap(0.0)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch)
                .clip()
                .scrollable(),
            self.self_strip(),
        ])
        .gap(0.0)
        .fill(vs::SIDE_BAR_BG)
        .border_r()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(SIDE_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    // -----------------------------------------------------------------
    // Conversation
    // -----------------------------------------------------------------

    fn channel_header(&self) -> El {
        row([
            icon(IconName::Volume2)
                .icon_size(tokens::ICON_SM)
                .color(vs::DESCRIPTION_FG),
            text("Build Room")
                .label()
                .font_size(15.0)
                .font_weight(FontWeight::Semibold)
                .color(vs::TAB_ACTIVE_FG),
            text(
                "Renderer milestone M6 — lane plots, tick precision, and the Thursday cut. \
                 Screen share is fair game.",
            )
            .caption()
            .ellipsis()
            .width(Size::Fill(1.0))
            .color(vs::DESCRIPTION_FG),
            icon(IconName::Users)
                .icon_size(tokens::ICON_XS)
                .color(vs::DESCRIPTION_FG),
            text("4 in voice").caption().color(vs::DESCRIPTION_FG),
            icon_button(IconName::LayoutDashboard)
                .key("chan:pin")
                .ghost()
                .tooltip("Pinned messages"),
            icon_button(IconName::Search)
                .key("chan:search")
                .ghost()
                .tooltip("Search this channel"),
            icon_button(IconName::MoreHorizontal)
                .key("chan:more")
                .ghost()
                .tooltip("Channel actions"),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_3))
        .width(Size::Fill(1.0))
        .height(Size::Fixed(CHANNEL_HEADER_HEIGHT))
        .border_b()
        .border_color(vs::PANEL_BORDER)
        .align(Align::Center)
    }

    /// The timestamp gutter cell. Every log row starts with one, so the
    /// times form the vertical rule the eye skips down.
    fn time(&self, t: &str) -> El {
        text(t)
            .mono()
            .caption()
            .font_size(11.0)
            .tabular_numerals()
            .width(Size::Fixed(TIME_COL))
            .height(Size::Fixed(20.0))
            .color(vs::DESCRIPTION_FG)
    }

    /// A message with an author header, plus any number of body blocks.
    fn message(&self, t: &str, a: Author, body: Vec<El>) -> El {
        let mut head = vec![
            tile(a.initials, a.color, 16.0, 3.0),
            text(a.name)
                .label()
                .font_weight(FontWeight::Semibold)
                .color(a.color),
        ];
        if let Some(role) = a.role {
            head.push(tag(role, vs::CHAT_EDITED_FILE_FG));
        }

        let mut stackk = vec![
            row(head)
                .gap(tokens::SPACE_2)
                .height(Size::Fixed(20.0))
                .width(Size::Fill(1.0))
                .align(Align::Center),
        ];
        stackk.extend(body);

        row([
            self.time(t),
            column(stackk)
                .gap(2.0)
                .width(Size::Fill(1.0))
                .align(Align::Start),
        ])
        .gap(0.0)
        .width(Size::Fill(1.0))
        .align(Align::Start)
    }

    /// A follow-on line from the same author — timestamp plus body only.
    fn continuation(&self, t: &str, body: El) -> El {
        row([self.time(t), body])
            .gap(0.0)
            .width(Size::Fill(1.0))
            .align(Align::Start)
    }

    /// A join / move / state notice, rendered as one dim line.
    fn notice(&self, t: &str, glyph: IconName, glyph_color: Color, who: &str, what: &str) -> El {
        row([
            self.time(t),
            row([
                icon(glyph).icon_size(tokens::ICON_XS).color(glyph_color),
                text(who)
                    .caption()
                    .font_weight(FontWeight::Semibold)
                    .color(vs::EDITOR_FG),
                text(what).caption().color(vs::DESCRIPTION_FG),
            ])
            .gap(tokens::SPACE_2)
            .height(Size::Fixed(20.0))
            .width(Size::Fill(1.0))
            .align(Align::Center),
        ])
        .gap(0.0)
        .width(Size::Fill(1.0))
        .align(Align::Start)
    }

    /// Message body text.
    fn body(&self, s: &str) -> El {
        text(s)
            .body()
            .wrap_text()
            .width(Size::Fill(1.0))
            .color(vs::EDITOR_FG)
    }

    /// The syntax-highlighted code card.
    ///
    /// `code_block_chrome` is the documented entry point for exactly this
    /// — "any pre-built body El, typically a styled `text_runs` paragraph
    /// produced by a syntax highlighter". The card's file/meta header is
    /// not part of the chrome, so it goes in the same column; the 2px
    /// cyan edge is a sibling rect because a per-side border would be
    /// fighting the chrome's own all-round stroke.
    fn code_card(&self) -> El {
        let run = |s: &str, c: Color| text(s).mono().caption().color(c);

        let body = column([
            row([
                text("<>").mono().caption().color(vs::DESCRIPTION_FG),
                text("time_scale.rs").mono().caption().color(vs::EDITOR_FG),
                spacer(),
                text("4 lines · shared to channel")
                    .mono()
                    .caption()
                    .color(vs::DESCRIPTION_FG),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            text_runs([
                run(
                    "// ticks are computed relative to the epoch, never absolute",
                    TOK_COMMENT,
                ),
                hard_break(),
                run("let ", TOK_KEYWORD),
                run("rel = (t - self.epoch) ", TOK_VAR),
                run("as ", TOK_CONTROL),
                run("f64;", TOK_TYPE),
                hard_break(),
                run("let ", TOK_KEYWORD),
                run("px  = rel * self.px_per_ns + self.origin_px;", TOK_VAR),
                hard_break(),
                run("debug_assert!", TOK_CONTROL),
                run("(rel.abs() < ", TOK_VAR),
                run("4.5e15", TOK_NUMBER),
                run("); ", TOK_VAR),
                run("// f64 exact-int range", TOK_COMMENT),
            ])
            .width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_3)
        .width(Size::Fill(1.0));

        row([
            column(Vec::<El>::new())
                .width(Size::Fixed(2.0))
                .height(Size::Fill(1.0))
                .fill(ACCENT_CYAN),
            code_block_chrome(body)
                .fill(CODE_CARD_BG)
                .stroke(vs::PANEL_BORDER)
                .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
                .width(Size::Fill(1.0)),
        ])
        .gap(0.0)
        .width(Size::Fixed(CODE_CARD_WIDTH))
        .align(Align::Stretch)
    }

    fn message_log(&self) -> El {
        let blue = TOK_VAR;
        let magenta = TOK_CONTROL;
        let teal = TOK_TYPE;
        let violet = Color::srgb_u8a(177, 145, 233, 255);
        let sky = Color::srgb_u8a(102, 178, 232, 255);

        column(vec![
            self.continuation(
                "",
                self.body(
                    "Morning. I pushed the epoch-relative tick branch last night — deep zoom \
                     stops drifting once the span goes past ~40 minutes.",
                ),
            ),
            self.continuation(
                "09:05",
                text_runs([
                    text("We were folding the epoch into the scale ").body(),
                    text("before").body().italic(),
                    text(" the subtraction, so everything past the first hour landed in f32 mush.")
                        .body(),
                ])
                .width(Size::Fill(1.0)),
            ),
            self.message(
                "09:07",
                author("Tomas Lindqvist", "TL", magenta, None),
                vec![self.body(
                    "That's the same thing you flagged Tuesday on the sub-second ticks, right? \
                     I assumed it was the label formatter.",
                )],
            ),
            self.message(
                "09:09",
                author("Priya Raghavan", "PR", blue, Some("admin")),
                vec![
                    text_runs([
                        text("Same root cause. The fix is three lines — the scale carries an ")
                            .body(),
                        text("i64").code().color(vs::CHAT_EDITED_FILE_FG),
                        text(" epoch now and everything downstream is relative:").body(),
                    ])
                    .width(Size::Fill(1.0)),
                    self.code_card(),
                ],
            ),
            self.message(
                "09:12",
                author("Aiko Tanaka", "AT", teal, None),
                vec![self.body(
                    "Nice. I'll take the lane-plot side today if nobody's already on it — the \
                     legend still clips at 3 lanes.",
                )],
            ),
            self.notice(
                "09:14",
                IconName::LogOut,
                vs::DESCRIPTION_FG,
                "dan.okafor",
                "moved to Lobby.",
            ),
            self.message(
                "09:16",
                author("Tomas Lindqvist", "TL", magenta, None),
                vec![self.body(
                    "All yours. I'm staying on the renderer — there's a validation-layer warning \
                     on the second pass I want to chase before it turns into a Thursday problem.",
                )],
            ),
            self.message(
                "09:21",
                author("Ben Kepner", "BK", sky, Some("you")),
                vec![self.body(
                    "Can we do a quick share at 10:00? I want to see tick spacing at 200× before \
                     we lock the public API on this.",
                )],
            ),
            self.message(
                "09:22",
                author("Priya Raghavan", "PR", blue, Some("admin")),
                vec![self.body(
                    "Works for me. I'll have the showcase build warm so we're not watching a \
                     compile.",
                )],
            ),
            self.message(
                "09:28",
                author("Marta Vogel", "MV", violet, None),
                vec![self.body(
                    "Heads up: I'm rebooting the build box at 09:45. CI goes red for about six \
                     minutes, don't file anything.",
                )],
            ),
            self.notice(
                "09:31",
                IconName::HeadphoneOff,
                vs::ERROR_FG,
                "aiko.tanaka",
                "deafened themselves.",
            ),
            self.message(
                "09:33",
                author("Tomas Lindqvist", "TL", magenta, None),
                vec![self.body(
                    "Sharing my screen now — second render pass, watch the top-left lane as I \
                     scrub.",
                )],
            ),
            // The typing indicator rides in the log rather than in the
            // composer, exactly where the comp puts it.
            row([
                text("•••").caption().color(vs::DESCRIPTION_FG),
                text("priya.raghavan")
                    .caption()
                    .font_weight(FontWeight::Semibold)
                    .color(vs::DESCRIPTION_FG),
                text("is typing…").caption().color(vs::DESCRIPTION_FG),
            ])
            .gap(tokens::SPACE_1)
            .pl(TIME_COL)
            .width(Size::Fill(1.0))
            .align(Align::Center),
        ])
        // 3px between blocks, not a `SPACE_*` step: the comp's log
        // rhythm is measured at 1px inside a message and 3px between
        // them, which is below the bottom rung of the spacing scale.
        .gap(3.0)
        .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_2))
        // The comp holds the reading measure at ~865px rather than
        // running body text to the window edge — a right gutter, not a
        // `max-width`, because the timestamps stay flush left.
        .pr(72.0)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        // `.scrollable()` makes the node a scroll viewport but does NOT
        // clip its content — `.clip()` is a separate modifier. Without
        // it the scrolled-off head of the log paints straight over the
        // channel header. See the note at the bottom of this file.
        .clip()
        .scrollable()
        .pin_end()
    }

    /// Composer well plus the shortcut hint strip under it.
    fn composer(&self) -> El {
        let well = row([
            icon_button(IconName::Paperclip)
                .key("composer:attach")
                .ghost()
                .tooltip("Attach a file"),
            vertical_separator().height(Size::Fixed(20.0)),
            text_input_with(
                "composer",
                &self.draft,
                &self.selection,
                TextInputOpts::default().placeholder("Message Build Room"),
            )
            .fill(COMPOSER_BG)
            .stroke(COMPOSER_BG)
            .width(Size::Fill(1.0)),
            icon_button(IconName::Smile)
                .key("composer:emoji")
                .ghost()
                .tooltip("Emoji"),
            button_with_icon(IconName::Send, "Send")
                .key("composer:send")
                .primary(),
        ])
        .gap(tokens::SPACE_1)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .fill(COMPOSER_BG)
        .stroke(vs::INPUT_BORDER)
        .radius(vs::RADIUS)
        .width(Size::Fill(1.0))
        .align(Align::Center);

        // Each hint is its own tight row so the groups space apart while
        // `Shift`+`Enter` stays welded, exactly as the comp reads.
        let hint = |keys: Vec<El>, label: &str| {
            let mut children = keys;
            children.push(text(label).caption().color(vs::DESCRIPTION_FG));
            row(children).gap(tokens::SPACE_1).align(Align::Center)
        };

        let hints = row([
            hint(vec![kbd("Enter")], "send"),
            hint(
                vec![
                    row([
                        kbd("Shift"),
                        text("+").caption().color(vs::DESCRIPTION_FG),
                        kbd("Enter"),
                    ])
                    .gap(1.0)
                    .align(Align::Center),
                ],
                "newline",
            ),
            hint(vec![kbd("/")], "commands"),
            spacer(),
            icon(IconName::Mic)
                .icon_size(tokens::ICON_XS)
                .color(vs::EDITOR_GUTTER_ADDED_BG),
            text("Push-to-talk held ·")
                .caption()
                .color(vs::EDITOR_GUTTER_ADDED_BG),
            kbd("F9"),
        ])
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .align(Align::Center);

        column([well, hints])
            .gap(tokens::SPACE_2)
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
            .width(Size::Fill(1.0))
            .border_t()
            .border_color(vs::PANEL_BORDER)
            .align(Align::Stretch)
    }

    fn conversation(&self) -> El {
        column([self.channel_header(), self.message_log(), self.composer()])
            .gap(0.0)
            .fill(vs::EDITOR_BG)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
    }

    // -----------------------------------------------------------------
    // Status bar
    // -----------------------------------------------------------------

    fn status(&self) -> El {
        let sep = || vertical_separator().height(Size::Fixed(12.0));
        let dim = |s: &str| text(s).caption().color(vs::DESCRIPTION_FG);
        let mono_dim = |s: &str| {
            text(s)
                .mono()
                .caption()
                .font_size(11.0)
                .tabular_numerals()
                .color(vs::DESCRIPTION_FG)
        };

        status_bar(
            [
                row([
                    icon(IconName::Wifi)
                        .icon_size(tokens::ICON_XS)
                        .color(vs::EDITOR_GUTTER_ADDED_BG),
                    dim("Connected — voice.example.com · 48 ms"),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
                sep(),
                row([
                    icon(IconName::Activity)
                        .icon_size(tokens::ICON_XS)
                        .color(vs::DESCRIPTION_FG),
                    dim("Build Room"),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
                sep(),
                mono_dim("0.4 % loss"),
                mono_dim("jitter 3.1 ms"),
            ],
            [
                row([
                    mono_dim("IN"),
                    meter(10, 8, 7, 2.0, 9.0),
                    mono_dim("-18 dB"),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
                sep(),
                row([
                    mono_dim("OUT"),
                    meter(10, 9, 7, 2.0, 9.0),
                    mono_dim("-6 dB"),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
                sep(),
                mono_dim("Opus 96 kb/s · 48 kHz"),
                sep(),
                row([
                    icon(IconName::Mic)
                        .icon_size(tokens::ICON_XS)
                        .color(vs::DESCRIPTION_FG),
                    mono_dim("Scarlett 2i2"),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
                row([
                    icon(IconName::Headphones)
                        .icon_size(tokens::ICON_XS)
                        .color(vs::DESCRIPTION_FG),
                    mono_dim("HD 6XX"),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
                icon(IconName::Bell)
                    .icon_size(tokens::ICON_XS)
                    .color(vs::DESCRIPTION_FG),
            ],
        )
    }
}

impl App for Banter {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column, not `page()` — an application shell is
        // full-bleed, and window padding would float the instrument on a
        // margin. `overlays(...)` because the shell carries tooltips; the
        // tooltip layer mounts on an `Axis::Overlay` root and the first
        // hover panics without one.
        let shell = column([
            self.title_strip(),
            row([self.activity_rail(), self.side_bar(), self.conversation()])
                .gap(0.0)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            self.status(),
        ])
        .gap(0.0)
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
