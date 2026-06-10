//! Structured element keys — building and routing `prefix:index` keys.
//!
//! Keys are plain strings, and per-item widgets conventionally encode
//! the item's identity into them with a `:` separator: a photo grid
//! keys its cells `thumb:0`, `thumb:1`, …; a per-row dropdown keys its
//! triggers `profile:7`; the stock select widget routes option clicks
//! as `{key}:option:{value}`. This module owns the two halves of that
//! convention so apps stop hand-rolling them:
//!
//! - **Building**: [`indexed`] formats `prefix:index`.
//! - **Routing**: [`suffix`] / [`index`] take a routed key apart again
//!   — and unlike the hand-rolled `strip_prefix` + `parse().ok()`
//!   chain, a prefix match whose index fails to parse logs a warning
//!   instead of silently swallowing the event. A dropped click that
//!   "does nothing" almost always means a stale option list or a
//!   key-shape mismatch; the log line says so.
//!
//! [`UiEvent::route_suffix`](crate::UiEvent::route_suffix) and
//! [`UiEvent::route_index`](crate::UiEvent::route_index) wrap these
//! for the common dispatch shape:
//!
//! ```
//! use damascene_core::prelude::*;
//! use damascene_core::key;
//!
//! // Build: one keyed cell per item.
//! fn cell(i: usize) -> El {
//!     button("Open").key(key::indexed("thumb", i))
//! }
//!
//! // Event: route the click back to the item.
//! fn on_event(event: &UiEvent) -> Option<usize> {
//!     event.route_index::<usize>("thumb")
//! }
//!
//! let event = UiEvent::synthetic_click(key::indexed("thumb", 42));
//! assert_eq!(on_event(&event), Some(42));
//! ```

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::fmt::Display;
use std::str::FromStr;

/// Build the key `prefix:index` — the conventional shape for per-item
/// element keys (`thumb:42`, `row:7`). The index can be anything
/// `Display` (integer ids, uuids, enum tokens); route it back with
/// [`suffix`] / [`index`].
pub fn indexed(prefix: &str, index: impl Display) -> String {
    format!("{prefix}:{index}")
}

/// If `key` is `prefix` followed by a `:` separator, the rest of the
/// key; otherwise `None`. The separator is required — `suffix("thumbnail:3",
/// "thumb")` does *not* match, so sibling prefixes sharing a stem
/// can't shadow each other.
///
/// The suffix may itself contain further `:` segments (the select
/// widget routes `profile:7:option:3`); use [`index`] to read just the
/// first segment as a typed value.
pub fn suffix<'a>(key: &'a str, prefix: &str) -> Option<&'a str> {
    key.strip_prefix(prefix)?.strip_prefix(':')
}

/// Parse the first segment after `prefix:` as a `T` — `index::<u32>("profile:7:option:3",
/// "profile")` is `Some(7)`, `index::<usize>("thumb:42", "thumb")` is
/// `Some(42)`.
///
/// Returns `None` when the prefix doesn't match (the normal "not my
/// event" case, no logging) **or** when the segment fails to parse —
/// but the parse failure also logs a warning, because a prefix match
/// with an unparseable index is a bug shape, not a routing miss: a
/// stale option list, a key built with a different id type, or two
/// widgets sharing a prefix. The hand-rolled `strip_prefix` +
/// `parse().ok()` chain this replaces dropped those events silently.
pub fn index<T: FromStr>(key: &str, prefix: &str) -> Option<T> {
    let rest = suffix(key, prefix)?;
    let segment = rest.split(':').next().unwrap_or(rest);
    match segment.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            log::warn!(
                "damascene: key {key:?} matches prefix {prefix:?} but its index \
                 segment {segment:?} does not parse as {}; the event will be \
                 ignored (stale option list, or a key built with a different \
                 id type?)",
                std::any::type_name::<T>(),
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_builds_the_conventional_shape() {
        assert_eq!(indexed("thumb", 42), "thumb:42");
        assert_eq!(indexed("row", "uuid-7"), "row:uuid-7");
    }

    #[test]
    fn suffix_requires_the_separator() {
        assert_eq!(suffix("thumb:42", "thumb"), Some("42"));
        assert_eq!(suffix("thumb:42:more", "thumb"), Some("42:more"));
        // Stem sharing must not shadow.
        assert_eq!(suffix("thumbnail:3", "thumb"), None);
        // Bare prefix with no separator/index.
        assert_eq!(suffix("thumb", "thumb"), None);
        assert_eq!(suffix("other:42", "thumb"), None);
    }

    #[test]
    fn index_parses_the_first_segment_only() {
        assert_eq!(index::<usize>("thumb:42", "thumb"), Some(42));
        // Trailing routed segments (select's `:option:` shape) don't
        // break the leading id.
        assert_eq!(index::<u32>("profile:7:option:3", "profile"), Some(7));
        // Prefix miss: quiet None.
        assert_eq!(index::<usize>("other:42", "thumb"), None);
        // Prefix hit, parse failure: None (and a warn, not asserted here).
        assert_eq!(index::<usize>("thumb:abc", "thumb"), None);
        // Round-trip with the builder.
        assert_eq!(index::<u64>(&indexed("row", 9_u64), "row"), Some(9));
    }
}
