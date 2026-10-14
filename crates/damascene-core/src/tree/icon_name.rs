//! Built-in icon-name vocabulary.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

/// Built-in icon names. The string forms intentionally mirror common
/// lucide/shadcn names so agents can reach for familiar labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum IconName {
    /// A pulse/activity waveform.
    Activity,
    /// An exclamation mark in a circle; also rendered as the fallback for unknown string icon names.
    AlertCircle,
    /// A downward arrow with a full shaft (distinct from the bare [`IconName::ChevronDown`]).
    ArrowDown,
    /// A leftward arrow with a full shaft.
    ArrowLeft,
    /// A rightward arrow with a full shaft.
    ArrowRight,
    /// An upward arrow with a full shaft.
    ArrowUp,
    /// Vertical bar-chart columns.
    BarChart,
    /// A notification bell.
    Bell,
    /// An isometric closed carton; packages, builds, artifacts, 3D objects.
    Box,
    /// A camera body with a lens; photo capture / snapshot actions.
    Camera,
    /// A checkmark; the stock checkbox's checked indicator.
    Check,
    /// A downward chevron; the stock select's dropdown indicator.
    ChevronDown,
    /// A leftward chevron; the stock calendar's previous-month button.
    ChevronLeft,
    /// A rightward chevron; the stock accordion's expand indicator and the calendar's next-month button.
    ChevronRight,
    /// An upward chevron.
    ChevronUp,
    /// Angle brackets; source code / developer views.
    Code,
    /// The command (⌘) key symbol.
    Command,
    /// A half-filled circle; contrast / appearance settings.
    Contrast,
    /// A download arrow.
    Download,
    /// A box with an arrow leaving it; opens a link outside the app.
    ExternalLink,
    /// An eye with a pupil; visibility toggles, preview, "show".
    Eye,
    /// A text-document outline.
    FileText,
    /// Two brackets around a dashed axis; mirror across the vertical axis.
    FlipHorizontal,
    /// A folder outline.
    Folder,
    /// A git branch fork.
    GitBranch,
    /// A git commit dot on a line.
    GitCommit,
    /// A globe with meridians; locale/language or network reach.
    Globe,
    /// Headphones struck through; output muted / deafened.
    HeadphoneOff,
    /// An over-ear headphone band; audio output devices.
    Headphones,
    /// An information "i" in a circle.
    Info,
    /// A keyboard with keys; shortcuts and input settings.
    Keyboard,
    /// A dashboard grid of panels.
    LayoutDashboard,
    /// A closed padlock; secured / private / read-only.
    Lock,
    /// A door with an arrow leaving it; sign out or leave a session.
    LogOut,
    /// Three stacked lines (hamburger menu).
    Menu,
    /// A square speech bubble; chat and comment threads.
    MessageSquare,
    /// A microphone capsule on a stand; audio input.
    Mic,
    /// A microphone struck through; audio input muted.
    MicOff,
    /// A horizontal ellipsis for overflow/"more" actions.
    MoreHorizontal,
    /// A four-way arrow cross; translate / drag a viewport or object.
    Move,
    /// A paperclip; attach a file.
    Paperclip,
    /// A plus sign; the stock editor-tabs add-tab button.
    Plus,
    /// Clockwise refresh arrows.
    RefreshCw,
    /// A counter-clockwise rotation arrow.
    RotateCcw,
    /// A clockwise rotation arrow.
    RotateCw,
    /// A diagonal ruler with tick marks; measure tools and dimensions.
    Ruler,
    /// A box with a diagonal resize arrow; scale / resize an object.
    Scaling,
    /// A monitor with an arrow leaving it; share this screen.
    ScreenShare,
    /// A magnifying glass.
    Search,
    /// A paper plane; submit a message.
    Send,
    /// A gear.
    Settings,
    /// A smiling face; emoji picker / reactions.
    Smile,
    /// A shell prompt chevron above a caret line; terminal / console.
    Terminal,
    /// An upload arrow.
    Upload,
    /// A group of people silhouettes.
    Users,
    /// A speaker with two waves; audio output level.
    Volume2,
    /// A speaker with a cross; audio output muted.
    VolumeX,
    /// Concentric signal arcs over a dot; wireless connectivity.
    Wifi,
    /// An "x" cross; the stock close button (e.g. editor-tab close).
    X,
}

impl IconName {
    /// Resolve a lucide/shadcn-style string name (e.g. `"chevron-down"`,
    /// with a few aliases like `"close"` for `X`) to its variant, or
    /// `None` if the name is not in the built-in vocabulary.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "activity" => Some(Self::Activity),
            "alert-circle" | "alert" => Some(Self::AlertCircle),
            "arrow-down" => Some(Self::ArrowDown),
            "arrow-left" => Some(Self::ArrowLeft),
            "arrow-right" => Some(Self::ArrowRight),
            "arrow-up" => Some(Self::ArrowUp),
            "bar-chart" | "chart-bar" => Some(Self::BarChart),
            "bell" => Some(Self::Bell),
            "box" => Some(Self::Box),
            "camera" => Some(Self::Camera),
            "check" => Some(Self::Check),
            "chevron-down" => Some(Self::ChevronDown),
            "chevron-left" => Some(Self::ChevronLeft),
            "chevron-right" => Some(Self::ChevronRight),
            "chevron-up" => Some(Self::ChevronUp),
            "code" => Some(Self::Code),
            "command" => Some(Self::Command),
            "contrast" => Some(Self::Contrast),
            "download" => Some(Self::Download),
            "external-link" => Some(Self::ExternalLink),
            "eye" => Some(Self::Eye),
            "file-text" | "file" => Some(Self::FileText),
            "flip-horizontal" => Some(Self::FlipHorizontal),
            "folder" => Some(Self::Folder),
            "git-branch" => Some(Self::GitBranch),
            "git-commit" => Some(Self::GitCommit),
            "globe" => Some(Self::Globe),
            "headphone-off" => Some(Self::HeadphoneOff),
            "headphones" => Some(Self::Headphones),
            "info" => Some(Self::Info),
            "keyboard" => Some(Self::Keyboard),
            "layout-dashboard" | "dashboard" => Some(Self::LayoutDashboard),
            "lock" => Some(Self::Lock),
            "log-out" | "logout" => Some(Self::LogOut),
            "menu" => Some(Self::Menu),
            "message-square" | "message" => Some(Self::MessageSquare),
            "mic" | "microphone" => Some(Self::Mic),
            "mic-off" => Some(Self::MicOff),
            "more-horizontal" | "more" => Some(Self::MoreHorizontal),
            "move" => Some(Self::Move),
            "paperclip" => Some(Self::Paperclip),
            "plus" => Some(Self::Plus),
            "refresh-cw" | "refresh" => Some(Self::RefreshCw),
            "rotate-ccw" => Some(Self::RotateCcw),
            "rotate-cw" | "rotate" => Some(Self::RotateCw),
            "ruler" => Some(Self::Ruler),
            "scaling" => Some(Self::Scaling),
            "screen-share" => Some(Self::ScreenShare),
            "search" => Some(Self::Search),
            "send" => Some(Self::Send),
            "settings" => Some(Self::Settings),
            "smile" => Some(Self::Smile),
            "terminal" => Some(Self::Terminal),
            "upload" => Some(Self::Upload),
            "users" => Some(Self::Users),
            "volume-2" | "volume" => Some(Self::Volume2),
            "volume-x" | "mute" => Some(Self::VolumeX),
            "wifi" => Some(Self::Wifi),
            "x" | "close" => Some(Self::X),
            _ => None,
        }
    }

    /// The canonical lucide-style string name (the inverse of [`IconName::parse`],
    /// modulo aliases).
    pub fn name(self) -> &'static str {
        match self {
            Self::Activity => "activity",
            Self::AlertCircle => "alert-circle",
            Self::ArrowDown => "arrow-down",
            Self::ArrowLeft => "arrow-left",
            Self::ArrowRight => "arrow-right",
            Self::ArrowUp => "arrow-up",
            Self::BarChart => "bar-chart",
            Self::Bell => "bell",
            Self::Box => "box",
            Self::Camera => "camera",
            Self::Check => "check",
            Self::ChevronDown => "chevron-down",
            Self::ChevronLeft => "chevron-left",
            Self::ChevronRight => "chevron-right",
            Self::ChevronUp => "chevron-up",
            Self::Code => "code",
            Self::Command => "command",
            Self::Contrast => "contrast",
            Self::Download => "download",
            Self::ExternalLink => "external-link",
            Self::Eye => "eye",
            Self::FileText => "file-text",
            Self::FlipHorizontal => "flip-horizontal",
            Self::Folder => "folder",
            Self::GitBranch => "git-branch",
            Self::GitCommit => "git-commit",
            Self::Globe => "globe",
            Self::HeadphoneOff => "headphone-off",
            Self::Headphones => "headphones",
            Self::Info => "info",
            Self::Keyboard => "keyboard",
            Self::LayoutDashboard => "layout-dashboard",
            Self::Lock => "lock",
            Self::LogOut => "log-out",
            Self::Menu => "menu",
            Self::MessageSquare => "message-square",
            Self::Mic => "mic",
            Self::MicOff => "mic-off",
            Self::MoreHorizontal => "more-horizontal",
            Self::Move => "move",
            Self::Paperclip => "paperclip",
            Self::Plus => "plus",
            Self::RefreshCw => "refresh-cw",
            Self::RotateCcw => "rotate-ccw",
            Self::RotateCw => "rotate-cw",
            Self::Ruler => "ruler",
            Self::Scaling => "scaling",
            Self::ScreenShare => "screen-share",
            Self::Search => "search",
            Self::Send => "send",
            Self::Settings => "settings",
            Self::Smile => "smile",
            Self::Terminal => "terminal",
            Self::Upload => "upload",
            Self::Users => "users",
            Self::Volume2 => "volume-2",
            Self::VolumeX => "volume-x",
            Self::Wifi => "wifi",
            Self::X => "x",
        }
    }

    /// A single-character text stand-in for the icon, used where the
    /// vector glyph cannot be drawn (e.g. text-only rendering paths).
    pub fn fallback_glyph(self) -> &'static str {
        match self {
            Self::Activity => "~",
            Self::AlertCircle => "!",
            Self::ArrowDown => "↓",
            Self::ArrowLeft => "←",
            Self::ArrowRight => "→",
            Self::ArrowUp => "↑",
            Self::BarChart => "▮",
            Self::Bell => "•",
            Self::Box => "⬡",
            Self::Camera => "▣",
            Self::Check => "✓",
            Self::ChevronDown => "⌄",
            Self::ChevronLeft => "‹",
            Self::ChevronRight => "›",
            Self::ChevronUp => "⌃",
            Self::Code => "⟨",
            Self::Command => "⌘",
            Self::Contrast => "◐",
            Self::Download => "↓",
            Self::ExternalLink => "↗",
            Self::Eye => "◎",
            Self::FileText => "□",
            Self::FlipHorizontal => "⇄",
            Self::Folder => "▱",
            Self::GitBranch => "⑂",
            Self::GitCommit => "⊙",
            Self::Globe => "⊕",
            Self::HeadphoneOff => "⊖",
            Self::Headphones => "∩",
            Self::Info => "i",
            Self::Keyboard => "⌨",
            Self::LayoutDashboard => "▦",
            Self::Lock => "⚿",
            Self::LogOut => "⇥",
            Self::Menu => "☰",
            Self::MessageSquare => "▭",
            Self::Mic => "◉",
            Self::MicOff => "⊘",
            Self::MoreHorizontal => "…",
            Self::Move => "✥",
            Self::Paperclip => "⌇",
            Self::Plus => "+",
            Self::RefreshCw => "↻",
            Self::RotateCcw => "⟲",
            Self::RotateCw => "⟳",
            Self::Ruler => "⊿",
            Self::Scaling => "⤢",
            Self::ScreenShare => "⧉",
            Self::Search => "⌕",
            Self::Send => "➤",
            Self::Settings => "⚙",
            Self::Smile => "☺",
            Self::Terminal => "❯",
            Self::Upload => "↑",
            Self::Users => "●",
            Self::Volume2 => "♪",
            Self::VolumeX => "⊗",
            Self::Wifi => "≋",
            Self::X => "×",
        }
    }
}
