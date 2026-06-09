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
    /// Vertical bar-chart columns.
    BarChart,
    /// A notification bell.
    Bell,
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
    /// The command (⌘) key symbol.
    Command,
    /// A download arrow.
    Download,
    /// A text-document outline.
    FileText,
    /// A folder outline.
    Folder,
    /// A git branch fork.
    GitBranch,
    /// A git commit dot on a line.
    GitCommit,
    /// An information "i" in a circle.
    Info,
    /// A dashboard grid of panels.
    LayoutDashboard,
    /// Three stacked lines (hamburger menu).
    Menu,
    /// A horizontal ellipsis for overflow/"more" actions.
    MoreHorizontal,
    /// A plus sign; the stock editor-tabs add-tab button.
    Plus,
    /// Clockwise refresh arrows.
    RefreshCw,
    /// A magnifying glass.
    Search,
    /// A gear.
    Settings,
    /// An upload arrow.
    Upload,
    /// A group of people silhouettes.
    Users,
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
            "bar-chart" | "chart-bar" => Some(Self::BarChart),
            "bell" => Some(Self::Bell),
            "check" => Some(Self::Check),
            "chevron-down" => Some(Self::ChevronDown),
            "chevron-left" => Some(Self::ChevronLeft),
            "chevron-right" => Some(Self::ChevronRight),
            "chevron-up" => Some(Self::ChevronUp),
            "command" => Some(Self::Command),
            "download" => Some(Self::Download),
            "file-text" | "file" => Some(Self::FileText),
            "folder" => Some(Self::Folder),
            "git-branch" => Some(Self::GitBranch),
            "git-commit" => Some(Self::GitCommit),
            "info" => Some(Self::Info),
            "layout-dashboard" | "dashboard" => Some(Self::LayoutDashboard),
            "menu" => Some(Self::Menu),
            "more-horizontal" | "more" => Some(Self::MoreHorizontal),
            "plus" => Some(Self::Plus),
            "refresh-cw" | "refresh" => Some(Self::RefreshCw),
            "search" => Some(Self::Search),
            "settings" => Some(Self::Settings),
            "upload" => Some(Self::Upload),
            "users" => Some(Self::Users),
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
            Self::BarChart => "bar-chart",
            Self::Bell => "bell",
            Self::Check => "check",
            Self::ChevronDown => "chevron-down",
            Self::ChevronLeft => "chevron-left",
            Self::ChevronRight => "chevron-right",
            Self::ChevronUp => "chevron-up",
            Self::Command => "command",
            Self::Download => "download",
            Self::FileText => "file-text",
            Self::Folder => "folder",
            Self::GitBranch => "git-branch",
            Self::GitCommit => "git-commit",
            Self::Info => "info",
            Self::LayoutDashboard => "layout-dashboard",
            Self::Menu => "menu",
            Self::MoreHorizontal => "more-horizontal",
            Self::Plus => "plus",
            Self::RefreshCw => "refresh-cw",
            Self::Search => "search",
            Self::Settings => "settings",
            Self::Upload => "upload",
            Self::Users => "users",
            Self::X => "x",
        }
    }

    /// A single-character text stand-in for the icon, used where the
    /// vector glyph cannot be drawn (e.g. text-only rendering paths).
    pub fn fallback_glyph(self) -> &'static str {
        match self {
            Self::Activity => "~",
            Self::AlertCircle => "!",
            Self::BarChart => "▮",
            Self::Bell => "•",
            Self::Check => "✓",
            Self::ChevronDown => "⌄",
            Self::ChevronLeft => "‹",
            Self::ChevronRight => "›",
            Self::ChevronUp => "⌃",
            Self::Command => "⌘",
            Self::Download => "↓",
            Self::FileText => "□",
            Self::Folder => "▱",
            Self::GitBranch => "⑂",
            Self::GitCommit => "⊙",
            Self::Info => "i",
            Self::LayoutDashboard => "▦",
            Self::Menu => "☰",
            Self::MoreHorizontal => "…",
            Self::Plus => "+",
            Self::RefreshCw => "↻",
            Self::Search => "⌕",
            Self::Settings => "⚙",
            Self::Upload => "↑",
            Self::Users => "●",
            Self::X => "×",
        }
    }
}
