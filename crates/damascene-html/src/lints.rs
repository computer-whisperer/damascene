//! Lint findings emitted while transforming HTML.
//!
//! The transformer's stated contract is: render the HTML subset that
//! maps cleanly onto Damascene's widget vocabulary, and **surface
//! everything else as a lint finding** so authors and downstream tools
//! know what was dropped. A doc that emits twenty findings tells the
//! author the renderer can't do their layout — without claiming
//! success.
//!
//! Findings carry no severity; the embedder decides whether each kind
//! is fatal, a warning, or informational. The transformer never
//! produces duplicates for the same node + kind, but does emit one
//! finding per occurrence across the document.
//!
//! Access via [`crate::html_with_lints`] or [`crate::html_blocks_with_lints`].
//! The lint-free entry points ([`crate::html`], [`crate::html_with_options`])
//! collect internally and discard, preserving the v1 signature.

use std::cell::RefCell;

/// What was dropped or coerced, in a single dimension. The transformer
/// emits at most one finding per (node, kind) tuple, so a single
/// element with three unsupported properties produces three findings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// A CSS declaration was recognised but its target has no Damascene
    /// equivalent — `position: absolute`, `float: left`, `display:
    /// grid`, `vh`/`vw`/`fr` units, etc. The author sees the
    /// declaration was dropped, not silently honoured.
    DroppedDeclaration,
    /// A `<style>`-block selector form is outside the supported subset
    /// (descendant / child / sibling combinators, pseudo-classes,
    /// attribute selectors, namespace prefixes) and the whole rule was
    /// ignored. Reported by the selector parser at stylesheet-collect
    /// time.
    UnsupportedSelector,
    /// Author-set `margin-top` / `margin-bottom` on siblings disagreed
    /// across pairs and was flattened to a single parent `gap`. The
    /// detail string spells out the values that collided.
    MarginAsymmetryFlattened,
    /// A tag (or a whole foreign-namespace subtree like inline `<svg>`
    /// / MathML `<math>`) was dropped because it has no equivalent and
    /// is not in the security-stripped set either — `<video>`,
    /// `<audio>`, `<canvas>`, `<dialog>`, `<colgroup>`, etc. Where the
    /// element can carry text (e.g. `<video>` fallback content), the
    /// contents are still flattened into the output; `<form>` /
    /// `<fieldset>` / `<legend>` are *not* in this set — they render
    /// as generic containers.
    UnsupportedTag,
    /// An attribute was recognised but can't be honoured — `colspan` /
    /// `rowspan` on table cells (cells render unmerged). The element
    /// itself still renders.
    UnsupportedAttribute,
    /// Block-level content appeared somewhere Damascene only renders
    /// inline runs (table cells), so its block structure was flattened
    /// — text survives, paragraph breaks and nested lists don't.
    FlattenedContent,
    /// A `<style>` block or inline `style="..."` attribute was dropped
    /// because [`crate::HtmlOptions`]'s `sanitize_styles` is set. Lets
    /// a sanitizing embedder see that the input tried to style itself.
    SanitizedStyle,
}

/// One lint occurrence. `detail` is a human-readable hint: the dropped
/// property name + value, the collided margin pair, the unsupported
/// selector text, and so on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub detail: String,
}

impl Finding {
    pub(crate) fn new(kind: FindingKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

/// Walker-internal collector. Threaded through `WalkCx` via shared
/// borrow + interior mutability so the dispatch functions can `push`
/// without a `&mut` plumb-through.
#[derive(Default, Debug)]
pub(crate) struct Lints {
    findings: RefCell<Vec<Finding>>,
}

impl Lints {
    pub(crate) fn push(&self, kind: FindingKind, detail: impl Into<String>) {
        self.findings.borrow_mut().push(Finding::new(kind, detail));
    }

    pub(crate) fn into_vec(self) -> Vec<Finding> {
        self.findings.into_inner()
    }
}
