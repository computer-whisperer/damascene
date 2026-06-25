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
    /// highest `y`), so peaks and troughs are never smoothed away.
    MinMax,
}

/// Reduce `samples` (assumed ascending in `x`, as a time series is) to at
/// most `2 * buckets` points across the visible `x_window`, keeping the
/// min and max `y` of each column. Samples outside the window are dropped.
/// Returns the input unchanged when it is already within budget or the
/// window is degenerate.
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
    out
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
        assert!(out.len() > 100, "keeps a useful envelope, got {}", out.len());
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
    fn drops_out_of_window_samples() {
        let s = series(1000);
        let out = minmax(&s, (400.0, 600.0), 50);
        assert!(out.iter().all(|p| p.x >= 400.0 && p.x <= 600.0));
    }

    #[test]
    fn degenerate_window_returns_input() {
        let s = series(1000);
        assert_eq!(minmax(&s, (5.0, 5.0), 50), s);
    }
}
