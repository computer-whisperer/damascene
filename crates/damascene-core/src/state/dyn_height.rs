//! Incremental row-height index for **append-only** `virtual_list_dyn`.
//!
//! The general dynamic-virtual path rebuilds a full `Vec<f32>` of every
//! row's height (and materializes every row key) on every layout pass —
//! several O(n) walks per frame regardless of how many rows are visible
//! (issue #107). For the common append-at-bottom feed (chat logs), the
//! row sequence only ever changes by *appending at the tail* and/or
//! *dropping a contiguous prefix at the head* (a capped ring buffer).
//! Under that contract the per-frame work can be made O(visible) by
//! keeping the heights persistent across frames and reconciling them
//! incrementally.
//!
//! This module is the persistent structure behind that fast path. It is
//! a deque of per-row heights (a `Vec` with a logical `base` offset so
//! head-trim is O(t) without reindexing the tail) plus a √n-block
//! partial-sum index so `row_top` / `visible_range` answer in O(√n)
//! instead of an O(n) walk from row 0 — which is exactly the cost that
//! bites the stick-to-bottom case where the scroll position sits near
//! the end. A `key → index` map serves anchor restoration and
//! `ScrollRequest::ToRowKey` in O(1).
//!
//! The contract is the caller's to uphold (declared via
//! `VirtualMode::Dynamic { append_only: true }`); when an incoming frame
//! cannot be reconciled as trim-then-append (a reorder, a mid-list
//! insert, a re-key), [`DynHeightIndex::reconcile`] reports failure and
//! the caller falls back to a cold rebuild — correct geometry, just O(n)
//! for that frame. Debug builds additionally assert the invariant so a
//! violation is loud in tests rather than a silent perf cliff.

use rustc_hash::FxHashMap;

/// Block size for the √n-decomposition partial sums. Fixed rather than
/// `isqrt(len)` for simplicity: at 100k rows this is ~195 blocks of ≤512,
/// so a prefix sum touches at most ~700 elements — three orders of
/// magnitude under the from-zero walk it replaces, and well inside a
/// 120Hz frame budget. If profiling ever shows this short the internals
/// can move to a Fenwick tree without touching callers.
const BLOCK: usize = 512;

/// Persistent per-row height index for one append-only dynamic virtual
/// list, keyed in [`crate::state::ScrollState`] by the list's
/// `computed_id`. See the module docs for the access-pattern rationale.
#[derive(Clone, Debug)]
pub(crate) struct DynHeightIndex {
    /// Layout-width bucket these heights were measured at. A change
    /// (horizontal resize) invalidates every cached height, forcing a
    /// cold rebuild.
    width_bucket: u32,
    /// Placeholder height for not-yet-measured rows. A change forces a
    /// cold rebuild (it shifts every unmeasured row's contribution).
    estimated_row_height: f32,
    /// Physical index of logical row 0. The live window is the physical
    /// range `[base, heights.len())`; head-trim advances `base` instead
    /// of shifting the tail. Reclaimed by [`Self::maybe_compact`].
    base: usize,
    /// Per-row heights, physically indexed. `heights[base + i]` is the
    /// height of logical row `i`. The dead prefix `[0, base)` is retained
    /// (it stays counted in `block_sums`, and cancels out via
    /// `base_prefix`) until compaction reclaims it.
    heights: Vec<f32>,
    /// Stable row key per physical slot, parallel to `heights`. Needed to
    /// evict trimmed keys from `key_to_phys` and to spot-check the
    /// append-only invariant in debug builds.
    keys: Vec<String>,
    /// `block_sums[b]` = Σ `heights[b*BLOCK .. min(len, (b+1)*BLOCK)]`,
    /// over *physical* indices. Maintained on append / point-update;
    /// rebuilt only on compaction or cold build.
    block_sums: Vec<f32>,
    /// Σ `heights[base..]` — the live content's summed height, cached so
    /// total-height / max-offset are O(1).
    live_total: f32,
    /// `prefix_phys(base)` = Σ `heights[0..base]`, the dead-prefix sum
    /// subtracted out of every live prefix query. Changes only on trim
    /// (grows by the trimmed heights) and compaction (resets to 0).
    base_prefix: f32,
    /// Logical index of each live key. Stored as a *physical* index;
    /// subtract `base` for the logical index. Trimmed keys are removed.
    key_to_phys: FxHashMap<String, usize>,
}

impl DynHeightIndex {
    /// Number of live rows.
    pub(crate) fn count(&self) -> usize {
        self.heights.len() - self.base
    }

    /// Height of logical row `i`.
    pub(crate) fn height(&self, i: usize) -> f32 {
        self.heights[self.base + i]
    }

    /// Σ of all live row heights (no gaps).
    pub(crate) fn heights_sum(&self) -> f32 {
        self.live_total
    }

    /// Top edge (y) of logical row `i`: Σ heights below it plus the
    /// inter-row gaps. Matches the general path's `dynamic_row_top`
    /// (`Σ_{j<i}(h_j + gap)`).
    pub(crate) fn row_top(&self, i: usize, gap: f32) -> f32 {
        self.prefix_live(i) + i as f32 * gap
    }

    /// Logical index of a row by stable key, or `None` if not live.
    pub(crate) fn index_for_key(&self, key: &str) -> Option<usize> {
        self.key_to_phys.get(key).map(|phys| phys - self.base)
    }

    /// Σ `heights[0..p]` over physical indices, O(√n) via the block sums.
    fn prefix_phys(&self, p: usize) -> f32 {
        let full_blocks = p / BLOCK;
        let mut sum = 0.0;
        for b in 0..full_blocks {
            sum += self.block_sums[b];
        }
        for h in &self.heights[full_blocks * BLOCK..p] {
            sum += *h;
        }
        sum
    }

    /// Σ heights of logical rows `[0, i)`.
    fn prefix_live(&self, i: usize) -> f32 {
        self.prefix_phys(self.base + i) - self.base_prefix
    }

    /// Visible-range query, semantically identical to the general path's
    /// `dynamic_visible_range`: returns `(start, start_y, end)` where
    /// `start` is the first row whose bottom edge is below `offset`,
    /// `start_y` is that row's top, and `end` is the half-open index past
    /// the last row intersecting the viewport. `start` is found by an
    /// O(√n) binary search over the block sums; the forward walk to
    /// `end` is O(visible).
    pub(crate) fn visible_range(
        &self,
        gap: f32,
        offset: f32,
        viewport_h: f32,
    ) -> (usize, f32, usize) {
        let count = self.count();
        // First row not fully above the offset — partition point of the
        // monotonically increasing predicate `bottom(k) <= offset`.
        let mut lo = 0;
        let mut hi = count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let bottom = self.row_top(mid, gap) + self.height(mid);
            if bottom <= offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let start = lo;
        if start >= count {
            return (count, self.row_top(count, gap), count);
        }
        let start_y = self.row_top(start, gap);

        let mut end = start;
        let mut cursor = start_y;
        let viewport_bottom = offset + viewport_h;
        while end < count && cursor < viewport_bottom {
            cursor += self.height(end) + gap;
            end += 1;
        }
        (start, start_y, end)
    }

    /// Cold build over `0..count`. `row` yields each row's `(key, height)`
    /// — the caller resolves the height from the measurement cache or the
    /// estimate. O(n); used on first frame, width-bucket change, or when
    /// an incoming frame can't be reconciled incrementally.
    pub(crate) fn build(
        width_bucket: u32,
        estimated_row_height: f32,
        count: usize,
        mut row: impl FnMut(usize) -> (String, f32),
    ) -> Self {
        let mut heights = Vec::with_capacity(count);
        let mut keys = Vec::with_capacity(count);
        let mut key_to_phys = FxHashMap::with_capacity_and_hasher(count, Default::default());
        let mut live_total = 0.0;
        for i in 0..count {
            let (key, h) = row(i);
            key_to_phys.insert(key.clone(), i);
            keys.push(key);
            heights.push(h);
            live_total += h;
        }
        let block_sums = Self::build_block_sums(&heights);
        DynHeightIndex {
            width_bucket,
            estimated_row_height,
            base: 0,
            heights,
            keys,
            block_sums,
            live_total,
            base_prefix: 0.0,
            key_to_phys,
        }
    }

    fn build_block_sums(heights: &[f32]) -> Vec<f32> {
        let n_blocks = heights.len().div_ceil(BLOCK).max(1);
        let mut sums = vec![0.0; n_blocks];
        for (i, h) in heights.iter().enumerate() {
            sums[i / BLOCK] += *h;
        }
        sums
    }

    /// Update logical row `i`'s height to `h` (a remeasure as the row
    /// enters the viewport). O(1).
    pub(crate) fn set_height(&mut self, i: usize, h: f32) {
        let phys = self.base + i;
        let delta = h - self.heights[phys];
        if delta == 0.0 {
            return;
        }
        self.heights[phys] = h;
        self.block_sums[phys / BLOCK] += delta;
        self.live_total += delta;
    }

    /// Append a row at the tail. O(1) amortized.
    fn push(&mut self, key: String, h: f32) {
        let phys = self.heights.len();
        let block = phys / BLOCK;
        if block == self.block_sums.len() {
            self.block_sums.push(0.0);
        }
        self.block_sums[block] += h;
        self.live_total += h;
        self.key_to_phys.insert(key.clone(), phys);
        self.keys.push(key);
        self.heights.push(h);
    }

    /// Drop the first `t` logical rows (head trim). O(t). The trimmed
    /// keys are moved into `trimmed` so the caller can evict their
    /// measurement-cache entries (the general path prunes those with an
    /// O(n) set-build every frame; here it's O(trimmed)).
    fn trim_front(&mut self, t: usize, trimmed: &mut Vec<String>) {
        for phys in self.base..self.base + t {
            // The slot joins the dead prefix; nothing reads `keys[phys]`
            // again until compaction drains it, so move the string out.
            let key = std::mem::take(&mut self.keys[phys]);
            self.key_to_phys.remove(&key);
            trimmed.push(key);
            self.base_prefix += self.heights[phys];
            self.live_total -= self.heights[phys];
        }
        self.base += t;
    }

    /// Reclaim the dead prefix once it dominates, keeping physical
    /// storage bounded over a long-running trimmed feed. Amortized O(1)
    /// per trimmed row (we only pay the O(live) rebuild when `base` has
    /// grown past half the storage).
    fn maybe_compact(&mut self) {
        if self.base <= self.heights.len() / 2 || self.base <= 2 * BLOCK {
            return;
        }
        self.heights.drain(0..self.base);
        self.keys.drain(0..self.base);
        for phys in self.key_to_phys.values_mut() {
            *phys -= self.base;
        }
        self.block_sums = Self::build_block_sums(&self.heights);
        self.base_prefix = 0.0;
        self.base = 0;
    }

    /// Reconcile the index with the next frame, which under the
    /// append-only contract differs from the current state only by a
    /// head-trim of some prefix followed by a tail-append of some suffix.
    ///
    /// `head_key` is `row_key(0)` for the new frame; `key_of(i)` yields
    /// the new frame's key at logical index `i`; `height_of(i, key)`
    /// resolves the measured-or-estimated height for a newly appended
    /// row. Returns `true` on success. Returns `false` — leaving `self`
    /// untouched — when the frame can't be expressed as trim-then-append
    /// (width/estimate change, head key not in the current window, or a
    /// tail shorter than the surviving prefix); the caller then cold
    /// rebuilds.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reconcile(
        &mut self,
        width_bucket: u32,
        estimated_row_height: f32,
        count: usize,
        head_key: &str,
        mut key_of: impl FnMut(usize) -> String,
        mut height_of: impl FnMut(usize, &str) -> f32,
        trimmed: &mut Vec<String>,
    ) -> bool {
        if width_bucket != self.width_bucket
            || estimated_row_height != self.estimated_row_height
            || self.count() == 0
            || count == 0
        {
            return false;
        }
        let old_count = self.count();
        // Where does the new head sit in the current window? That offset
        // is exactly the trim count.
        let Some(&head_phys) = self.key_to_phys.get(head_key) else {
            return false;
        };
        let t = head_phys - self.base;
        // new_count = old_count - t + a  ⇒  a = new_count - (old_count - t)
        let surviving = old_count - t;
        if count < surviving {
            // Tail shrank below the surviving prefix — not a pure
            // trim-then-append. Contract violation; cold rebuild.
            debug_assert!(
                false,
                "append-only virtual list lost tail rows (count {count} < surviving \
                 prefix {surviving}); reconcile falling back to rebuild"
            );
            return false;
        }
        let appended = count - surviving;

        // Spot-check the surviving prefix kept its identity (catches
        // reorders / mid-list inserts the head probe alone would miss).
        #[cfg(debug_assertions)]
        if surviving > 0 {
            let mid = t + (surviving - 1) / 2;
            debug_assert_eq!(
                self.keys[self.base + mid],
                key_of(mid - t),
                "append-only virtual list reordered a surviving row; reconcile invalid"
            );
            debug_assert_eq!(
                self.keys[self.base + old_count - 1],
                key_of(surviving - 1),
                "append-only virtual list mutated its tail before the append point"
            );
        }

        self.trim_front(t, trimmed);
        self.maybe_compact();
        for j in 0..appended {
            let logical = surviving + j;
            let key = key_of(logical);
            let h = height_of(logical, &key);
            self.push(key, h);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Brute-force oracle: the logical rows as a flat list, with prefix
    /// sums computed naively. Heights are integer-valued so f32 addition
    /// is exact regardless of summation order, letting the tests compare
    /// against the block-sum structure with `==`.
    struct Oracle {
        rows: Vec<(String, f32)>,
    }

    impl Oracle {
        fn row_top(&self, i: usize, gap: f32) -> f32 {
            self.rows[..i].iter().map(|(_, h)| *h).sum::<f32>() + i as f32 * gap
        }
        fn heights_sum(&self) -> f32 {
            self.rows.iter().map(|(_, h)| *h).sum()
        }
        fn visible_range(&self, gap: f32, offset: f32, vh: f32) -> (usize, f32, usize) {
            let count = self.rows.len();
            let mut start = 0;
            let mut y = 0.0_f32;
            while start < count {
                let h = self.rows[start].1;
                if y + h > offset {
                    break;
                }
                y += h + gap;
                start += 1;
            }
            let mut end = start;
            let mut cursor = y;
            let bottom = offset + vh;
            while end < count && cursor < bottom {
                cursor += self.rows[end].1 + gap;
                end += 1;
            }
            (start, y, end)
        }
    }

    /// Deterministic LCG so the op sequence is reproducible without
    /// `rand` (and without the banned `Math.random`).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 16
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn assert_matches(idx: &DynHeightIndex, oracle: &Oracle, gap: f32) {
        assert_eq!(idx.count(), oracle.rows.len(), "count");
        assert_eq!(idx.heights_sum(), oracle.heights_sum(), "heights_sum");
        for i in 0..oracle.rows.len() {
            assert_eq!(idx.height(i), oracle.rows[i].1, "height[{i}]");
            assert_eq!(idx.row_top(i, gap), oracle.row_top(i, gap), "row_top[{i}]");
            assert_eq!(
                idx.index_for_key(&oracle.rows[i].0),
                Some(i),
                "index_for_key[{i}]"
            );
        }
        // Probe visible_range across the whole scroll range plus past
        // the ends.
        let total = oracle.heights_sum() + gap * oracle.rows.len().saturating_sub(1) as f32;
        for step in 0..20 {
            let offset = total * step as f32 / 19.0 - 50.0;
            assert_eq!(
                idx.visible_range(gap, offset, 300.0),
                oracle.visible_range(gap, offset, 300.0),
                "visible_range at offset {offset}"
            );
        }
    }

    fn build_pair(count: usize, est: f32, gap: f32) -> (DynHeightIndex, Oracle) {
        let mut rng = Lcg(0xC0FFEE);
        let mut rows = Vec::new();
        for i in 0..count {
            // Mix measured and estimate-valued rows.
            let h = if rng.below(3) == 0 {
                est
            } else {
                (10 + rng.below(90)) as f32
            };
            rows.push((format!("k{i}"), h));
        }
        let oracle = Oracle { rows: rows.clone() };
        let idx = DynHeightIndex::build(7, est, count, |i| rows[i].clone());
        let _ = gap;
        (idx, oracle)
    }

    #[test]
    fn cold_build_matches_oracle_across_blocks() {
        let gap = 4.0;
        let (idx, oracle) = build_pair(2000, 20.0, gap);
        assert_matches(&idx, &oracle, gap);
    }

    #[test]
    fn set_height_matches_oracle() {
        let gap = 3.0;
        let (mut idx, mut oracle) = build_pair(1500, 20.0, gap);
        let mut rng = Lcg(42);
        for _ in 0..400 {
            let i = rng.below(oracle.rows.len() as u64) as usize;
            let h = (10 + rng.below(120)) as f32;
            idx.set_height(i, h);
            oracle.rows[i].1 = h;
        }
        assert_matches(&idx, &oracle, gap);
    }

    /// The core append-only workload: repeated trim-then-append with
    /// interleaved remeasures, reconciled incrementally and checked
    /// against the oracle every round.
    #[test]
    fn reconcile_trim_then_append_matches_oracle() {
        let gap = 5.0;
        let est = 20.0;
        let (mut idx, mut oracle) = build_pair(3000, est, gap);
        let mut rng = Lcg(0xABCD);
        let mut next_key = 3000usize;

        for round in 0..60 {
            let trim = rng.below(40) as usize;
            let trim = trim.min(oracle.rows.len().saturating_sub(1));
            let append = rng.below(40) as usize;

            // Build the next frame's logical rows: drop `trim` from the
            // front, append `append` fresh keys at the back.
            let mut next: Vec<(String, f32)> = oracle.rows[trim..].to_vec();
            for _ in 0..append {
                let h = if rng.below(3) == 0 {
                    est
                } else {
                    (10 + rng.below(90)) as f32
                };
                next.push((format!("k{next_key}"), h));
                next_key += 1;
            }

            let count = next.len();
            let head_key = next[0].0.clone();
            let next_for_key = next.clone();
            let next_for_h = next.clone();
            let mut trimmed = Vec::new();
            let ok = idx.reconcile(
                7,
                est,
                count,
                &head_key,
                |i| next_for_key[i].0.clone(),
                |i, _k| next_for_h[i].1,
                &mut trimmed,
            );
            assert!(
                ok,
                "reconcile should succeed for trim-then-append (round {round})"
            );
            // The reported trimmed keys are exactly the rows dropped off
            // the head this round, in order.
            assert_eq!(
                trimmed,
                oracle.rows[..trim]
                    .iter()
                    .map(|(k, _)| k.clone())
                    .collect::<Vec<_>>(),
                "trimmed keys (round {round})"
            );

            oracle.rows = next;

            // Occasionally remeasure a few realized rows after the frame.
            for _ in 0..rng.below(5) {
                if oracle.rows.is_empty() {
                    break;
                }
                let i = rng.below(oracle.rows.len() as u64) as usize;
                let h = (10 + rng.below(120)) as f32;
                idx.set_height(i, h);
                oracle.rows[i].1 = h;
            }

            assert_matches(&idx, &oracle, gap);
        }
        // Compaction keeps the dead prefix bounded: it never exceeds
        // half the physical storage (modulo the small-list floor), so
        // storage stays within a constant factor of the live count over
        // an unbounded trimmed feed.
        assert!(
            idx.base <= idx.heights.len() / 2 || idx.base <= 2 * BLOCK,
            "compaction kept the dead prefix bounded: base {} vs phys {}",
            idx.base,
            idx.heights.len()
        );
    }

    /// Trim-dominated feed: the dead prefix must be reclaimed by
    /// compaction, and queries stay correct across the physical
    /// re-layout it performs.
    #[test]
    fn reconcile_trim_dominated_compacts_and_stays_correct() {
        let gap = 4.0;
        let est = 20.0;
        let (mut idx, mut oracle) = build_pair(3000, est, gap);
        let mut rng = Lcg(0x5EED);
        let mut next_key = 3000usize;
        let mut compacted = false;

        // Trim hard, append little: the dead prefix grows until it
        // dominates physical storage and compaction must reclaim it
        // (crosses base > len/2 around round 27 here).
        for _ in 0..35 {
            let trim = 60usize.min(oracle.rows.len().saturating_sub(1));
            let append = 5usize;
            let before_base = idx.base;

            let mut next: Vec<(String, f32)> = oracle.rows[trim..].to_vec();
            for _ in 0..append {
                next.push((format!("k{next_key}"), (10 + rng.below(90)) as f32));
                next_key += 1;
            }
            let head = next[0].0.clone();
            let nfk = next.clone();
            let nfh = next.clone();
            assert!(idx.reconcile(
                7,
                est,
                next.len(),
                &head,
                |i| nfk[i].0.clone(),
                |i, _| nfh[i].1,
                &mut Vec::new()
            ));
            oracle.rows = next;
            // A drop in base (after a trim that grew it) means compaction
            // fired this round.
            if idx.base < before_base {
                compacted = true;
            }
            assert_matches(&idx, &oracle, gap);
        }
        assert!(compacted, "trim-dominated feed should trigger compaction");
    }

    #[test]
    fn reconcile_rejects_width_change() {
        let (mut idx, _oracle) = build_pair(100, 20.0, 4.0);
        let ok = idx.reconcile(
            99,
            20.0,
            100,
            "k0",
            |i| format!("k{i}"),
            |_, _| 20.0,
            &mut Vec::new(),
        );
        assert!(!ok, "width-bucket change must force a cold rebuild");
    }

    #[test]
    fn reconcile_rejects_unknown_head() {
        let (mut idx, _oracle) = build_pair(100, 20.0, 4.0);
        // A head key that was never in the window — e.g. a full reset.
        let ok = idx.reconcile(
            7,
            20.0,
            100,
            "stranger",
            |i| format!("z{i}"),
            |_, _| 20.0,
            &mut Vec::new(),
        );
        assert!(!ok, "unknown head key must force a cold rebuild");
    }

    #[test]
    fn reconcile_pure_append_no_trim() {
        let gap = 2.0;
        let est = 15.0;
        let (mut idx, mut oracle) = build_pair(50, est, gap);
        let mut next = oracle.rows.clone();
        for i in 50..70 {
            next.push((format!("k{i}"), (i % 30 + 10) as f32));
        }
        let head = next[0].0.clone();
        let nfk = next.clone();
        let nfh = next.clone();
        assert!(idx.reconcile(
            7,
            est,
            next.len(),
            &head,
            |i| nfk[i].0.clone(),
            |i, _| nfh[i].1,
            &mut Vec::new()
        ));
        oracle.rows = next;
        assert_matches(&idx, &oracle, gap);
    }
}
