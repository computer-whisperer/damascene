//! CSS subset for tier-2A: per-element `style="..."` parsing and
//! application.
//!
//! Scope:
//!
//! - Visual properties — `color`, `background` / `background-color`,
//!   `padding` (shorthand + per-side), `border` / `border-color` /
//!   `border-width`, `border-radius`, `opacity`.
//! - Layout sizing — `width`, `height`, `min/max-width/height`.
//! - Text properties — `text-align`, `font-size`, `font-weight`,
//!   `font-style`, `text-decoration`.
//!
//! Out of scope for this slice (deferred to follow-ups in tier-2B / D):
//!
//! - `<style>` block parsing + tag / class / id selectors.
//! - `display`, `flex-direction`, `align-items`, `justify-content`,
//!   `overflow` — properties that change layout structure rather than
//!   style.
//! - `margin` — needs the layout-reconciliation pass that turns
//!   per-child margins into parent gap.
//! - `box-shadow`, `font-family` — best-effort mappings whose right
//!   answers depend on the wider theme conversation.
//!
//! Values are parsed best-effort: malformed declarations are silently
//! dropped, the surrounding declarations apply normally. There is no
//! error reporting yet — the bundle pipeline's lint pass is the
//! natural channel and will land with the layout-reconciliation slice.

use aetna_core::prelude::*;
use markup5ever_rcdom::{Handle, NodeData};

use crate::sanitize::is_blocked_attr;

/// Typed CSS-shaped size value: pixels, percentage-of-parent, or
/// `auto`. Converts to Aetna [`Size`] at apply time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CssSize {
    /// Concrete logical pixels (`200px`, `1.5rem`, `12pt`).
    Px(f32),
    /// Fraction of parent extent (`50%` → 0.5). Maps to `Size::Fill`.
    Percent(f32),
    /// Intrinsic size (`auto`). Maps to `Size::Hug`.
    Auto,
}

impl CssSize {
    fn into_aetna(self) -> Size {
        match self {
            CssSize::Px(v) => Size::Fixed(v),
            CssSize::Percent(frac) => Size::Fill(frac),
            CssSize::Auto => Size::Hug,
        }
    }
}

/// Flattened style for one element. Every field is `Option` so the
/// applier only touches fields the source declared — defaults from
/// the El's constructor stay intact for unspecified properties.
///
/// Authors mutate via the parser ([`parse_inline_style`]); consumers
/// project through [`apply_block_style`] or fold into [`crate::transform::InlineState`].
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct ComputedStyle {
    // Visual
    pub text_color: Option<Color>,
    pub background: Option<Color>,
    pub padding: Option<Sides>,
    pub border_color: Option<Color>,
    pub border_width: Option<f32>,
    pub border_radius: Option<f32>,
    pub opacity: Option<f32>,

    // Layout sizing
    pub width: Option<CssSize>,
    pub height: Option<CssSize>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,

    // Text
    pub text_align: Option<TextAlign>,
    pub font_size: Option<f32>,
    pub font_weight: Option<FontWeight>,
    /// `Some(true)` ↔ `font-style: italic` / `oblique`; `Some(false)`
    /// ↔ explicit `font-style: normal`. Unset stays unset so the
    /// applier doesn't clobber an outer `<em>`'s italic flag.
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
}

impl ComputedStyle {
    /// `true` when no declaration on the source element produced any
    /// value. Block builders use this to skip the wrap-in-column step
    /// for generic containers (`<div>`, `<section>`, …) whose only
    /// purpose was structural grouping.
    pub(crate) fn is_empty(&self) -> bool {
        *self == ComputedStyle::default()
    }

    /// Layer `other` on top of `self`: every field `other` declares
    /// (`Some(...)`) overwrites the corresponding field on `self`;
    /// fields `other` leaves unset stay as they were. The cascade
    /// engine calls this once per matching rule, ordered from lowest
    /// to highest priority, so the highest-priority declaration is
    /// the last to land.
    pub(crate) fn merge(&mut self, other: &ComputedStyle) {
        if other.text_color.is_some() {
            self.text_color = other.text_color;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.padding.is_some() {
            self.padding = other.padding;
        }
        if other.border_color.is_some() {
            self.border_color = other.border_color;
        }
        if other.border_width.is_some() {
            self.border_width = other.border_width;
        }
        if other.border_radius.is_some() {
            self.border_radius = other.border_radius;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        if other.width.is_some() {
            self.width = other.width;
        }
        if other.height.is_some() {
            self.height = other.height;
        }
        if other.min_width.is_some() {
            self.min_width = other.min_width;
        }
        if other.max_width.is_some() {
            self.max_width = other.max_width;
        }
        if other.min_height.is_some() {
            self.min_height = other.min_height;
        }
        if other.max_height.is_some() {
            self.max_height = other.max_height;
        }
        if other.text_align.is_some() {
            self.text_align = other.text_align;
        }
        if other.font_size.is_some() {
            self.font_size = other.font_size;
        }
        if other.font_weight.is_some() {
            self.font_weight = other.font_weight;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strikethrough.is_some() {
            self.strikethrough = other.strikethrough;
        }
    }

    /// Apply the visual + layout sizing fields to a block-level El.
    /// Text fields are applied too, for elements (`<p>`, `<h1>`, etc.)
    /// whose body is a single text leaf — they're a no-op on `Inlines`
    /// containers (where text fields ride on the per-run state
    /// instead).
    pub(crate) fn apply_to_block(&self, mut el: El) -> El {
        if let Some(c) = self.text_color {
            el = el.text_color(c);
        }
        if let Some(c) = self.background {
            el = el.fill(c);
        }
        if let Some(p) = self.padding {
            el = el.padding(p);
        }
        if let Some(w) = self.border_width {
            el = el.stroke_width(w);
        }
        if let Some(c) = self.border_color {
            el = el.stroke(c);
        }
        if let Some(r) = self.border_radius {
            el = el.radius(r);
        }
        if let Some(o) = self.opacity {
            el = el.opacity(o.clamp(0.0, 1.0));
        }
        if let Some(w) = self.width {
            el = el.width(w.into_aetna());
        }
        if let Some(h) = self.height {
            el = el.height(h.into_aetna());
        }
        if let Some(v) = self.min_width {
            el = el.min_width(v);
        }
        if let Some(v) = self.max_width {
            el = el.max_width(v);
        }
        if let Some(v) = self.min_height {
            el = el.min_height(v);
        }
        if let Some(v) = self.max_height {
            el = el.max_height(v);
        }
        if let Some(a) = self.text_align {
            el = el.text_align(a);
        }
        if let Some(s) = self.font_size {
            el = el.font_size(s);
        }
        if let Some(w) = self.font_weight {
            el = el.font_weight(w);
        }
        if let Some(true) = self.italic {
            el = el.italic();
        }
        if let Some(true) = self.underline {
            el = el.underline();
        }
        if let Some(true) = self.strikethrough {
            el = el.strikethrough();
        }
        el
    }
}

/// Read an element's `style="..."` attribute and parse it. Returns a
/// default-empty [`ComputedStyle`] when the attribute is absent.
pub(crate) fn read_inline_style(node: &Handle) -> ComputedStyle {
    let Some(raw) = element_style_attr(node) else {
        return ComputedStyle::default();
    };
    parse_inline_style(&raw)
}

fn element_style_attr(node: &Handle) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    for a in attrs.borrow().iter() {
        let name = a.name.local.as_ref();
        if name.eq_ignore_ascii_case("style") && !is_blocked_attr(name) {
            return Some(a.value.to_string());
        }
    }
    None
}

/// Parse a `style="..."` value into a [`ComputedStyle`]. Unknown
/// properties and malformed values are silently dropped — known
/// declarations on the same element still apply.
pub(crate) fn parse_inline_style(input: &str) -> ComputedStyle {
    let mut style = ComputedStyle::default();
    for decl in split_declarations(input) {
        let Some((prop, value)) = split_declaration(decl) else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        apply_declaration(&mut style, &prop, value);
    }
    style
}

/// Split a declaration list on top-level `;`, honouring nested
/// parens (`rgb(0, 0, 0)`) and quoted strings.
fn split_declarations(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut depth: i32 = 0;
    let mut quote: Option<u8> = None;
    let mut start = 0;
    for i in 0..bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                if start < i {
                    out.push(&input[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < bytes.len() {
        out.push(&input[start..]);
    }
    out
}

fn split_declaration(decl: &str) -> Option<(&str, &str)> {
    let colon = decl.find(':')?;
    let (prop, rest) = decl.split_at(colon);
    Some((prop, &rest[1..]))
}

fn apply_declaration(style: &mut ComputedStyle, prop: &str, value: &str) {
    match prop {
        "color" => {
            if let Some(c) = parse_color(value) {
                style.text_color = Some(c);
            }
        }
        "background" | "background-color" => {
            if let Some(c) = parse_color(value) {
                style.background = Some(c);
            }
        }
        "padding" => {
            if let Some(p) = parse_sides_shorthand(value) {
                style.padding = Some(p);
            }
        }
        "padding-top" => with_side(style, value, |s, v| s.top = v),
        "padding-right" => with_side(style, value, |s, v| s.right = v),
        "padding-bottom" => with_side(style, value, |s, v| s.bottom = v),
        "padding-left" => with_side(style, value, |s, v| s.left = v),
        "border" => {
            let (width, color) = parse_border_shorthand(value);
            if let Some(w) = width {
                style.border_width = Some(w);
            }
            if let Some(c) = color {
                style.border_color = Some(c);
            }
        }
        "border-width" => {
            if let Some(w) = parse_length_px(value) {
                style.border_width = Some(w);
            }
        }
        "border-color" => {
            if let Some(c) = parse_color(value) {
                style.border_color = Some(c);
            }
        }
        "border-radius" => {
            if let Some(r) = parse_length_px(value) {
                style.border_radius = Some(r);
            }
        }
        "opacity" => {
            if let Ok(v) = value.parse::<f32>() {
                style.opacity = Some(v);
            }
        }
        "width" => {
            if let Some(s) = parse_css_size(value) {
                style.width = Some(s);
            }
        }
        "height" => {
            if let Some(s) = parse_css_size(value) {
                style.height = Some(s);
            }
        }
        "min-width" => {
            if let Some(px) = parse_length_px(value) {
                style.min_width = Some(px);
            }
        }
        "max-width" => {
            if let Some(px) = parse_length_px(value) {
                style.max_width = Some(px);
            }
        }
        "min-height" => {
            if let Some(px) = parse_length_px(value) {
                style.min_height = Some(px);
            }
        }
        "max-height" => {
            if let Some(px) = parse_length_px(value) {
                style.max_height = Some(px);
            }
        }
        "text-align" => {
            if let Some(a) = parse_text_align(value) {
                style.text_align = Some(a);
            }
        }
        "font-size" => {
            if let Some(px) = parse_length_px(value) {
                style.font_size = Some(px);
            }
        }
        "font-weight" => {
            if let Some(w) = parse_font_weight(value) {
                style.font_weight = Some(w);
            }
        }
        "font-style" => match value.to_ascii_lowercase().as_str() {
            "italic" | "oblique" => style.italic = Some(true),
            "normal" => style.italic = Some(false),
            _ => {}
        },
        "text-decoration" | "text-decoration-line" => {
            // Multi-value declaration: `text-decoration: underline
            // dotted` etc. We only honour the line kind.
            for token in value.split_ascii_whitespace() {
                match token.to_ascii_lowercase().as_str() {
                    "underline" => style.underline = Some(true),
                    "line-through" => style.strikethrough = Some(true),
                    "none" => {
                        style.underline = Some(false);
                        style.strikethrough = Some(false);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn with_side(style: &mut ComputedStyle, value: &str, mutate: impl FnOnce(&mut Sides, f32)) {
    let Some(px) = parse_length_px(value) else {
        return;
    };
    let mut sides = style.padding.unwrap_or(Sides::zero());
    mutate(&mut sides, px);
    style.padding = Some(sides);
}

// ---------- Value parsers ----------

/// Parse a CSS `<color>` value: hex (`#rgb`, `#rrggbb`, `#rrggbbaa`),
/// functional (`rgb(r,g,b)`, `rgba(r,g,b,a)`), the small named subset
/// in [`named_color`], or `transparent`.
pub(crate) fn parse_color(input: &str) -> Option<Color> {
    let s = input.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return Some(Color::rgba(0, 0, 0, 0));
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(rest) = s.strip_prefix(|c: char| matches!(c, 'r' | 'R')) {
        if let Some(args) = rest
            .strip_prefix(|c: char| matches!(c, 'g' | 'G'))
            .and_then(|r| r.strip_prefix(|c: char| matches!(c, 'b' | 'B')))
        {
            return parse_rgb_function(args);
        }
    }
    named_color(s)
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    fn nibble(c: char) -> Option<u8> {
        c.to_digit(16).map(|d| d as u8)
    }
    let chars: Vec<char> = hex.chars().collect();
    match chars.len() {
        3 => {
            let r = nibble(chars[0])?;
            let g = nibble(chars[1])?;
            let b = nibble(chars[2])?;
            Some(Color::rgb(r * 17, g * 17, b * 17))
        }
        4 => {
            let r = nibble(chars[0])?;
            let g = nibble(chars[1])?;
            let b = nibble(chars[2])?;
            let a = nibble(chars[3])?;
            Some(Color::rgba(r * 17, g * 17, b * 17, a * 17))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::rgb(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_function(after_rgb: &str) -> Option<Color> {
    // After-rgb form: `a(...)` for rgb(...) or `(...)` for raw — but
    // we already stripped `rgb`/`rgba` letters in `parse_color`, so
    // expect an optional `a` then a paren-wrapped arg list.
    let (has_alpha, rest) =
        if let Some(rest) = after_rgb.strip_prefix(|c: char| matches!(c, 'a' | 'A')) {
            (true, rest)
        } else {
            (false, after_rgb)
        };
    let rest = rest.trim();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split([',', '/']).map(str::trim).collect();
    let need = if has_alpha { 4 } else { 3 };
    if parts.len() < need {
        return None;
    }
    let r = parse_rgb_channel(parts[0])?;
    let g = parse_rgb_channel(parts[1])?;
    let b = parse_rgb_channel(parts[2])?;
    if has_alpha {
        let a = parse_alpha_channel(parts[3])?;
        Some(Color::rgba(r, g, b, a))
    } else {
        Some(Color::rgb(r, g, b))
    }
}

fn parse_rgb_channel(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        let pct: f32 = num.trim().parse().ok()?;
        return Some(((pct / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let n: f32 = s.parse().ok()?;
    Some(n.clamp(0.0, 255.0) as u8)
}

fn parse_alpha_channel(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        let pct: f32 = num.trim().parse().ok()?;
        return Some(((pct / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let n: f32 = s.parse().ok()?;
    Some((n.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Small CSS named-color subset. Not the full 147-name list; covers
/// the half-dozen names that show up regularly in authored scraps.
fn named_color(name: &str) -> Option<Color> {
    let n = name.to_ascii_lowercase();
    let (r, g, b) = match n.as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "silver" => (192, 192, 192),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "pink" => (255, 192, 203),
        "brown" => (165, 42, 42),
        _ => return None,
    };
    Some(Color::rgb(r, g, b))
}

/// Parse a CSS length into logical pixels. Supports `px`, `pt`, `rem`
/// (= 16px), `em` (= 16px in this slice; no parent-context lookup
/// yet), and bare `0`.
pub(crate) fn parse_length_px(input: &str) -> Option<f32> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s == "0" {
        return Some(0.0);
    }
    let (num, unit) = split_number_unit(s)?;
    let n: f32 = num.parse().ok()?;
    let px = match unit.to_ascii_lowercase().as_str() {
        "" => return None, // bare numbers (non-zero) need units in CSS
        "px" => n,
        "pt" => n * 96.0 / 72.0,
        "rem" => n * 16.0,
        "em" => n * 16.0, // no parent-font-size context yet
        _ => return None,
    };
    Some(px)
}

/// Parse a CSS `<size>` value: `<length>`, `<percent>`, or `auto`.
pub(crate) fn parse_css_size(input: &str) -> Option<CssSize> {
    let s = input.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(CssSize::Auto);
    }
    if let Some(rest) = s.strip_suffix('%') {
        let n: f32 = rest.trim().parse().ok()?;
        return Some(CssSize::Percent((n / 100.0).max(0.0)));
    }
    parse_length_px(s).map(CssSize::Px)
}

fn split_number_unit(s: &str) -> Option<(&str, &str)> {
    let end = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some((&s[..end], &s[end..]))
}

/// Parse a CSS `padding` / `margin` shorthand: 1, 2, 3, or 4 lengths.
/// Matches CSS order — `top right bottom left`, with the 2/3-value
/// forms mirroring the missing sides.
pub(crate) fn parse_sides_shorthand(input: &str) -> Option<Sides> {
    let parts: Vec<&str> = input.split_ascii_whitespace().collect();
    let px: Vec<f32> = parts
        .iter()
        .map(|p| parse_length_px(p))
        .collect::<Option<Vec<_>>>()?;
    let sides = match px.len() {
        1 => Sides::all(px[0]),
        2 => Sides {
            top: px[0],
            right: px[1],
            bottom: px[0],
            left: px[1],
        },
        3 => Sides {
            top: px[0],
            right: px[1],
            bottom: px[2],
            left: px[1],
        },
        4 => Sides {
            top: px[0],
            right: px[1],
            bottom: px[2],
            left: px[3],
        },
        _ => return None,
    };
    Some(sides)
}

/// Parse a `border` shorthand into `(width, color)`. The spec form is
/// `<width> <style> <color>` in any order; we pick out the first
/// length-shaped token as the width and the first colour-shaped token
/// as the colour, ignoring the `<style>` (Aetna only paints solid
/// borders).
pub(crate) fn parse_border_shorthand(input: &str) -> (Option<f32>, Option<Color>) {
    let mut width = None;
    let mut color = None;
    for token in input.split_ascii_whitespace() {
        if width.is_none()
            && let Some(px) = parse_length_px(token)
        {
            width = Some(px);
            continue;
        }
        if color.is_none()
            && let Some(c) = parse_color(token)
        {
            color = Some(c);
            continue;
        }
    }
    (width, color)
}

fn parse_text_align(value: &str) -> Option<TextAlign> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "start" => Some(TextAlign::Start),
        "center" => Some(TextAlign::Center),
        "right" | "end" => Some(TextAlign::End),
        _ => None,
    }
}

fn parse_font_weight(value: &str) -> Option<FontWeight> {
    match value.to_ascii_lowercase().as_str() {
        "normal" | "400" => Some(FontWeight::Regular),
        "500" => Some(FontWeight::Medium),
        "semibold" | "demibold" | "600" => Some(FontWeight::Semibold),
        "bold" | "700" | "bolder" | "800" | "900" => Some(FontWeight::Bold),
        "100" | "200" | "300" | "lighter" => Some(FontWeight::Regular),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_3_4_6_8_all_parse() {
        assert_eq!(parse_color("#000"), Some(Color::rgb(0, 0, 0)));
        assert_eq!(parse_color("#fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(parse_color("#abc"), Some(Color::rgb(170, 187, 204)));
        assert_eq!(parse_color("#1234"), Some(Color::rgba(17, 34, 51, 68)));
        assert_eq!(parse_color("#ff8800"), Some(Color::rgb(255, 136, 0)));
        assert_eq!(
            parse_color("#ff8800ff"),
            Some(Color::rgba(255, 136, 0, 255))
        );
    }

    #[test]
    fn rgb_and_rgba_functional_forms() {
        assert_eq!(parse_color("rgb(10, 20, 30)"), Some(Color::rgb(10, 20, 30)));
        assert_eq!(
            parse_color("rgba(10, 20, 30, 0.5)"),
            Some(Color::rgba(10, 20, 30, 128))
        );
        assert_eq!(
            parse_color("rgb(100%, 0%, 50%)"),
            Some(Color::rgb(255, 0, 128))
        );
    }

    #[test]
    fn named_colors_parse() {
        assert_eq!(parse_color("red"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(parse_color("RED"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(parse_color("transparent"), Some(Color::rgba(0, 0, 0, 0)));
        assert_eq!(parse_color("not-a-color"), None);
    }

    #[test]
    fn lengths_in_supported_units() {
        assert_eq!(parse_length_px("12px"), Some(12.0));
        assert_eq!(parse_length_px("1rem"), Some(16.0));
        assert_eq!(parse_length_px("2em"), Some(32.0));
        assert_eq!(parse_length_px("0"), Some(0.0));
        assert!((parse_length_px("12pt").unwrap() - 16.0).abs() < 0.01);
        assert_eq!(parse_length_px("12"), None);
        assert_eq!(parse_length_px("12vw"), None);
    }

    #[test]
    fn css_size_handles_px_percent_auto() {
        assert_eq!(parse_css_size("200px"), Some(CssSize::Px(200.0)));
        assert_eq!(parse_css_size("50%"), Some(CssSize::Percent(0.5)));
        assert_eq!(parse_css_size("auto"), Some(CssSize::Auto));
        assert_eq!(parse_css_size("AUTO"), Some(CssSize::Auto));
    }

    #[test]
    fn padding_shorthand_one_two_three_four() {
        assert_eq!(parse_sides_shorthand("8px"), Some(Sides::all(8.0)));
        assert_eq!(
            parse_sides_shorthand("8px 16px"),
            Some(Sides {
                top: 8.0,
                right: 16.0,
                bottom: 8.0,
                left: 16.0,
            })
        );
        assert_eq!(
            parse_sides_shorthand("8px 16px 24px"),
            Some(Sides {
                top: 8.0,
                right: 16.0,
                bottom: 24.0,
                left: 16.0,
            })
        );
        assert_eq!(
            parse_sides_shorthand("1px 2px 3px 4px"),
            Some(Sides {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            })
        );
    }

    #[test]
    fn border_shorthand_extracts_width_and_color_in_any_order() {
        let (w, c) = parse_border_shorthand("1px solid #f00");
        assert_eq!(w, Some(1.0));
        assert_eq!(c, Some(Color::rgb(255, 0, 0)));

        let (w, c) = parse_border_shorthand("red 2px");
        assert_eq!(w, Some(2.0));
        assert_eq!(c, Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn font_weight_named_and_numeric() {
        assert_eq!(parse_font_weight("bold"), Some(FontWeight::Bold));
        assert_eq!(parse_font_weight("700"), Some(FontWeight::Bold));
        assert_eq!(parse_font_weight("500"), Some(FontWeight::Medium));
        assert_eq!(parse_font_weight("normal"), Some(FontWeight::Regular));
        assert_eq!(parse_font_weight("400"), Some(FontWeight::Regular));
        assert_eq!(parse_font_weight("garbage"), None);
    }

    #[test]
    fn inline_style_round_trip() {
        let style = parse_inline_style(
            "color: #ff0000; background: rgb(0, 255, 0); padding: 8px 16px; \
             font-size: 14px; font-weight: bold; text-align: center; \
             border-radius: 4px; opacity: 0.5",
        );
        assert_eq!(style.text_color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(style.background, Some(Color::rgb(0, 255, 0)));
        assert_eq!(
            style.padding,
            Some(Sides {
                top: 8.0,
                right: 16.0,
                bottom: 8.0,
                left: 16.0,
            })
        );
        assert_eq!(style.font_size, Some(14.0));
        assert_eq!(style.font_weight, Some(FontWeight::Bold));
        assert_eq!(style.text_align, Some(TextAlign::Center));
        assert_eq!(style.border_radius, Some(4.0));
        assert_eq!(style.opacity, Some(0.5));
    }

    #[test]
    fn inline_style_ignores_unknown_props_and_malformed_values() {
        let style =
            parse_inline_style("color: red; foo: bar; padding: not-a-length; font-size: 18px");
        assert_eq!(style.text_color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(style.padding, None);
        assert_eq!(style.font_size, Some(18.0));
    }

    #[test]
    fn split_declarations_respects_parens_and_quotes() {
        let decls =
            split_declarations("color: rgb(0, 0, 0); background: \"hi; there\"; padding: 8px");
        assert_eq!(decls.len(), 3);
        assert!(decls[0].contains("rgb(0, 0, 0)"));
        assert!(decls[1].contains("hi; there"));
        assert!(decls[2].contains("padding"));
    }
}
