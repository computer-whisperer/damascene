//! Thin wrapper around `html5ever` + `markup5ever_rcdom`.
//!
//! The transformer walks a parsed RcDom tree rather than driving
//! `TreeSink` directly: the DOM is small enough for the scrap-sized
//! inputs this crate is aimed at, and the recursion is much easier to
//! reason about. If profiling shows the intermediate DOM matters we
//! can move to a direct `TreeSink` later.

use html5ever::driver::ParseOpts;
use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, parse_fragment};
// `namespace_url` is the trait the `ns!()` macro consumes via fully-
// qualified path; without it in scope the macro expands to an empty
// atom rather than the real HTML namespace.
#[allow(unused_imports)]
use html5ever::namespace_url;
use html5ever::{LocalName, QualName, ns};
use markup5ever_rcdom::{Handle, RcDom};

/// Parse a full HTML document and return its root (`Document`) handle.
/// `html5ever` wraps the input in the usual `<html><head></head><body>...
/// </body></html>` boilerplate; the transformer walks straight to the
/// body during `html(...)`.
pub(crate) fn parse_document_dom(input: &str) -> Handle {
    let dom: RcDom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .one(input.as_bytes());
    dom.document
}

/// Parse an HTML fragment in `<body>` context — what the markdown
/// transformer hands over when it folds an `Event::InlineHtml` or
/// `Event::Html` buffer. Returns the synthetic root whose children are
/// the fragment's top-level nodes.
pub(crate) fn parse_fragment_dom(input: &str) -> Handle {
    let context = QualName::new(None, ns!(html), LocalName::from("body"));
    let dom: RcDom = parse_fragment(
        RcDom::default(),
        ParseOpts::default(),
        context,
        Vec::new(),
        // scripting_enabled = false — we never run scripts and the
        // parser shouldn't apply scripting-only HTML5 quirks.
        false,
    )
    .from_utf8()
    .one(input.as_bytes());
    dom.document
}
