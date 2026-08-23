//! Down-sampling an over-dense series to the pixel budget — the
//! library-side half of the data-density story (the plan's decision 5).
//!
//! A **virtual** app resamples its own source and never needs this; a
//! **dump-everything** app hands the whole series and opts into a
//! [`Decimation`] so the plot stays fast. [`minmax`] keeps the visual
//! envelope (spikes survive) by emitting the min and max sample of each
//! pixel-column bucket — the right default for monitoring / TSDB data, where
//! a dropped spike is a dropped incident.

#![warn(missing_docs)]

use crate::plot::series::Sample;

/// How the plot reduces an over-dense series to the pixel budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decimation {
    /// Min/max-per-column envelope: two samples per bucket (the lowest and
    /// highest `y`), so peaks and troughs are never smoothed away. A step
    /// [`Curve`](crate::plot::Curve) mark automatically upgrades to the
    /// [`m4`] variant (first/min/max/last per bucket) so the level entering
    /// and leaving each column survives too.
    MinMax,
}

/// Reduce `samples` (assumed ascending in `x`, as a time series is) to at
/// most `2 * buckets + 2` points across the visible `x_window`, keeping the
/// min and max `y` of each column plus one **bracketing sample** just beyond
/// each window edge (see `brackets`), so the segment entering or leaving
/// the window still draws instead of the line starting at the first interior
/// sample. Other out-of-window samples are dropped. Returns the input
/// unchanged when it is already within budget or the window is degenerate.
pub fn minmax(samples: &[Sample], x_window: (f64, f64), buckets: usize) -> Vec<Sample> {
    let lo = x_window.0.min(x_window.1);
    let hi = x_window.0.max(x_window.1);
    let span = hi - lo;
    if buckets == 0 || !span.is_finite() || span <= 0.0 || samples.len() <= buckets * 2 {
        return samples.to_vec();
    }

    // (min-y, max-y) sample per column, columns already in x order.
    let mut cols: Vec<Option<(Sample, Sample)>> = vec![None; buckets];
    for &s in samples {
        if !s.x.is_finite() || !s.y.is_finite() || s.x < lo || s.x > hi {
            continue;
        }
        let frac = (s.x - lo) / span;
        let b = ((frac * buckets as f64) as usize).min(buckets - 1);
        match &mut cols[b] {
            None => cols[b] = Some((s, s)),
            Some((mn, mx)) => {
                if s.y < mn.y {
                    *mn = s;
                }
                if s.y > mx.y {
                    *mx = s;
                }
            }
        }
    }

    let mut out = Vec::with_capacity(buckets * 2);
    for (mn, mx) in cols.into_iter().flatten() {
        // Emit the two envelope points in x order so the polyline stays
        // monotonic in x; collapse to one when they coincide.
        let (first, second) = if mn.x <= mx.x { (mn, mx) } else { (mx, mn) };
        out.push(first);
        if second.x != first.x || second.y != first.y {
            out.push(second);
        }
    }
    with_brackets(samples, lo, hi, out)
}

/// The samples immediately bracketing the window — the last strictly left
/// of `lo` and the first strictly right of `hi` (ascending-`x` assumption;
/// non-finite candidates are dropped).
fn brackets(samples: &[Sample], lo: f64, hi: f64) -> (Option<Sample>, Option<Sample>) {
    let finite = |s: &Sample| s.x.is_finite() && s.y.is_finite();
    let l = samples.partition_point(|s| s.x < lo);
    let left = l.checked_sub(1).map(|i| samples[i]).filter(finite);
    let r = samples.partition_point(|s| s.x <= hi);
    let right = samples.get(r).copied().filter(finite);
    (left, right)
}

/// Wrap a decimated column emission in its window [`brackets`]. When the
/// window contains no samples at all, the brackets are kept only if both
/// exist (a segment spanning the whole window) — a one-sided bracket alone
/// would draw nothing inside the window.
fn with_brackets(samples: &[Sample], lo: f64, hi: f64, mut cols: Vec<Sample>) -> Vec<Sample> {
    let (left, right) = brackets(samples, lo, hi);
    if cols.is_empty() && (left.is_none() || right.is_none()) {
        return cols;
    }
    let mut out = Vec::with_capacity(cols.len() + 2);
    out.extend(left);
    out.append(&mut cols);
    out.extend(right);
    out
}

/// Reduce `samples` (assumed ascending in `x`) to at most `4 * buckets + 2`
/// points across the visible `x_window`, keeping the **first, min-y, max-y,
/// and last** sample of each column (the M4 aggregation) plus the window
/// `brackets`, emitted in `x` order. This is the step-curve variant of
/// [`minmax`]: keeping each column's first and last sample preserves the
/// level a square-edged trace enters and leaves the column with, so a
/// zoomed-out digital signal renders as a faithful activity band instead of
/// levels reordered into a zigzag. Returns the input unchanged when it is
/// already within budget or the window is degenerate.
pub fn m4(samples: &[Sample], x_window: (f64, f64), buckets: usize) -> Vec<Sample> {
    let lo = x_window.0.min(x_window.1);
    let hi = x_window.0.max(x_window.1);
    let span = hi - lo;
    if buckets == 0 || !span.is_finite() || span <= 0.0 || samples.len() <= buckets * 4 {
        return samples.to_vec();
    }

    // (first, min-y, max-y, last) sample per column, columns in x order.
    let mut cols: Vec<Option<(Sample, Sample, Sample, Sample)>> = vec![None; buckets];
    for &s in samples {
        if !s.x.is_finite() || !s.y.is_finite() || s.x < lo || s.x > hi {
            continue;
        }
        let frac = (s.x - lo) / span;
        let b = ((frac * buckets as f64) as usize).min(buckets - 1);
        match &mut cols[b] {
            None => cols[b] = Some((s, s, s, s)),
            Some((_, mn, mx, last)) => {
                if s.y < mn.y {
                    *mn = s;
                }
                if s.y > mx.y {
                    *mx = s;
                }
                *last = s; // ascending x: the latest seen is the column's last
            }
        }
    }

    let mut out = Vec::with_capacity(buckets * 4);
    for (first, mn, mx, last) in cols.into_iter().flatten() {
        // Emit in x order — chronological rank as the equal-x tiebreak, so
        // duplicate-x samples (hand-rolled risers) keep their input order —
        // skipping duplicates (an extreme that is also the first/last).
        let mut col = [(first, 0_u8), (mn, 1), (mx, 2), (last, 3)];
        col.sort_unstable_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.1.cmp(&b.1)));
        for (s, _) in col {
            if out.last() != Some(&s) {
                out.push(s);
            }
        }
    }
    with_brackets(samples, lo, hi, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(n: usize) -> Vec<Sample> {
        (0..n)
            .map(|i| Sample::new(i as f64, (i as f64 * 0.1).sin()))
            .collect()
    }

    #[test]
    fn within_budget_is_unchanged() {
        let s = series(10);
        assert_eq!(minmax(&s, (0.0, 9.0), 8), s);
    }

    #[test]
    fn reduces_to_envelope_within_budget() {
        let s = series(10_000);
        let out = minmax(&s, (0.0, 9_999.0), 100);
        assert!(out.len() <= 200, "≤ 2 per bucket, got {}", out.len());
        assert!(
            out.len() > 100,
            "keeps a useful envelope, got {}",
            out.len()
        );
        // x stays ascending.
        assert!(out.windows(2).all(|w| w[0].x <= w[1].x));
    }

    #[test]
    fn preserves_spikes() {
        // A flat line with one tall spike: decimation must keep the spike.
        let mut s: Vec<Sample> = (0..1000).map(|i| Sample::new(i as f64, 0.0)).collect();
        s[500].y = 99.0;
        let out = minmax(&s, (0.0, 999.0), 50);
        assert!(
            out.iter().any(|p| p.y == 99.0),
            "the spike must survive decimation"
        );
    }

    #[test]
    fn keeps_one_bracketing_sample_beyond_each_edge() {
        let s = series(1000);
        let out = minmax(&s, (400.0, 600.0), 50);
        // Exactly one sample beyond each edge anchors the segment entering /
        // leaving the window; everything between stays in-window.
        assert_eq!(out.first().map(|p| p.x), Some(399.0));
        assert_eq!(out.last().map(|p| p.x), Some(601.0));
        assert!(
            out[1..out.len() - 1]
                .iter()
                .all(|p| p.x >= 400.0 && p.x <= 600.0)
        );
    }

    #[test]
    fn window_inside_a_sample_gap_keeps_both_brackets() {
        // Dense data on both sides of a long gap, the window inside the gap:
        // the two brackets keep the spanning segment drawable. (A one-sided
        // bracket with nothing visible stays dropped — nothing would draw.)
        let mut s: Vec<Sample> = (0..3000).map(|i| Sample::new(i as f64, 1.0)).collect();
        s.extend((0..3000).map(|i| Sample::new(10_000.0 + i as f64, 2.0)));
        let out = minmax(&s, (5000.0, 6000.0), 50);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].x, out[1].x), (2999.0, 10_000.0));

        let past_the_end = minmax(&s, (20_000.0, 21_000.0), 50);
        assert!(past_the_end.is_empty(), "one-sided bracket alone drops");
    }

    #[test]
    fn degenerate_window_returns_input() {
        let s = series(1000);
        assert_eq!(minmax(&s, (5.0, 5.0), 50), s);
    }

    #[test]
    fn m4_within_budget_is_unchanged() {
        let s = series(16);
        assert_eq!(m4(&s, (0.0, 15.0), 4), s); // 16 ≤ 4·4
    }

    #[test]
    fn m4_keeps_first_min_max_last_per_bucket() {
        let s = vec![
            Sample::new(0.0, 3.0),  // first
            Sample::new(1.0, -9.0), // min
            Sample::new(2.0, 9.0),  // max
            Sample::new(3.0, 0.0),  // (interior, dropped)
            Sample::new(4.0, 1.0),  // last
        ];
        let out = m4(&s, (0.0, 4.0), 1);
        assert_eq!(out, vec![s[0], s[1], s[2], s[4]]);
    }

    #[test]
    fn m4_equal_x_samples_keep_a_deterministic_order() {
        // A duplicate-x riser pair inside one bucket: emission order is
        // first/min/max/last rank at equal x, independent of sort internals.
        let s = vec![
            Sample::new(0.0, 3.0),
            Sample::new(1.0, 9.0),
            Sample::new(1.0, -9.0),
            Sample::new(2.0, 5.0),
            Sample::new(4.0, 1.0),
        ];
        let out = m4(&s, (0.0, 4.0), 1);
        assert_eq!(out, vec![s[0], s[2], s[1], s[4]]);
    }

    #[test]
    fn m4_preserves_entry_exit_levels_and_spikes() {
        // A dense square wave: the global first/last levels survive (minmax
        // only keeps per-bucket extremes), as does a lone spike.
        let mut s: Vec<Sample> = (0..10_000)
            .map(|i| Sample::new(i as f64, if (i / 7) % 2 == 0 { 0.0 } else { 1.0 }))
            .collect();
        s[5000].y = 99.0;
        let out = m4(&s, (0.0, 9_999.0), 100);
        assert!(out.len() <= 400, "≤ 4 per bucket, got {}", out.len());
        assert_eq!(out.first(), s.first());
        assert_eq!(out.last(), s.last());
        assert!(out.iter().any(|p| p.y == 99.0), "spike survives");
        assert!(out.windows(2).all(|w| w[0].x <= w[1].x), "x ascending");
    }
}
