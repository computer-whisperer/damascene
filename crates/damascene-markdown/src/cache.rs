//! Keyed render cache for repeated [`md`](crate::md) calls — the
//! streaming-chat pattern.
//!
//! A conversation view re-renders its whole backlog every frame; calling
//! [`md`](crate::md) per message re-parses kilobytes of markdown that
//! hasn't changed since the last frame, and during streaming the cost is
//! paid per *delta*. [`MdCache`] memoizes the rendered `El` by source
//! text: stable messages hit the cache (a cheap tree clone), and only
//! the message currently growing re-parses. LRU-bounded so scrolled-away
//! backlogs don't accumulate forever — pair the capacity with your
//! visible window (`BuildCx::visible_range` tells you what's realized).

use std::collections::HashMap;

use damascene_core::El;

use crate::{MarkdownOptions, md_with_options};

/// Default entry cap — generous for a screenful of chat plus
/// scroll-back margin.
const DEFAULT_CAPACITY: usize = 256;

/// A bounded memo of `source text → rendered El` for one
/// [`MarkdownOptions`] configuration.
///
/// ```ignore
/// struct ChatApp {
///     md_cache: RefCell<MdCache>, // interior-mutable: written during build
///     // …
/// }
///
/// // per visible message, every frame:
/// let rendered = self.md_cache.borrow_mut().get(&message.text);
/// ```
///
/// Keys are the full source text (no hash-collision risk); values are
/// `El` subtrees cloned out on hit. A streaming message changes text
/// each delta, so it naturally misses (re-parses) while growing and
/// starts hitting once stable; superseded partial entries age out via
/// LRU.
pub struct MdCache {
    options: MarkdownOptions,
    capacity: usize,
    entries: HashMap<Box<str>, Entry>,
    /// Monotonic access counter for LRU stamps.
    tick: u64,
    /// Total cache misses (= real parses) — exposed for tests and
    /// perf-diagnostics overlays.
    parses: u64,
}

struct Entry {
    rendered: El,
    last_used: u64,
}

impl MdCache {
    /// A cache rendering with `options`, holding up to
    /// [`DEFAULT_CAPACITY`] entries.
    pub fn new(options: MarkdownOptions) -> Self {
        Self::with_capacity(options, DEFAULT_CAPACITY)
    }

    /// A cache with an explicit entry cap. Size it to your visible
    /// window plus scroll-back margin; each entry holds the source
    /// text and the rendered subtree.
    pub fn with_capacity(options: MarkdownOptions, capacity: usize) -> Self {
        Self {
            options,
            capacity: capacity.max(1),
            entries: HashMap::new(),
            tick: 0,
            parses: 0,
        }
    }

    /// The rendered tree for `text` — a clone of the cached subtree on
    /// hit, a fresh parse (then cached) on miss.
    pub fn get(&mut self, text: &str) -> El {
        self.tick += 1;
        let tick = self.tick;
        if let Some(entry) = self.entries.get_mut(text) {
            entry.last_used = tick;
            return entry.rendered.clone();
        }
        self.parses += 1;
        let rendered = md_with_options(text, self.options);
        if self.entries.len() >= self.capacity {
            self.evict_lru();
        }
        self.entries.insert(
            Box::from(text),
            Entry {
                rendered: rendered.clone(),
                last_used: tick,
            },
        );
        rendered
    }

    /// Number of cache misses so far — each one was a real markdown
    /// parse.
    pub fn parses(&self) -> u64 {
        self.parses
    }

    /// Entries currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is cached yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop everything (e.g. on theme change if your message trees bake
    /// theme-derived values — stock `md` output is theme-neutral and
    /// does not need this).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn evict_lru(&mut self) {
        if let Some(oldest) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_skip_the_parse() {
        let mut cache = MdCache::new(MarkdownOptions::default());
        let _ = cache.get("# stable message");
        let _ = cache.get("# stable message");
        let _ = cache.get("# stable message");
        assert_eq!(cache.parses(), 1);
        assert_eq!(cache.len(), 1);

        // A streaming tail misses per delta — each is a real parse.
        let _ = cache.get("partial");
        let _ = cache.get("partial text");
        assert_eq!(cache.parses(), 3);
    }

    #[test]
    fn lru_eviction_keeps_recent_entries() {
        let mut cache = MdCache::with_capacity(MarkdownOptions::default(), 2);
        let _ = cache.get("one");
        let _ = cache.get("two");
        let _ = cache.get("one"); // refresh "one"
        let _ = cache.get("three"); // evicts "two"
        assert_eq!(cache.len(), 2);
        let parses = cache.parses();
        let _ = cache.get("one"); // still cached
        assert_eq!(cache.parses(), parses);
        let _ = cache.get("two"); // evicted → re-parse
        assert_eq!(cache.parses(), parses + 1);
    }
}
