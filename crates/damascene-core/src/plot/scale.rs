//! Axis scales: the continuous, invertible warp from data space to scale
//! space, plus tick generation and value formatting.
//!
//! A [`Scale`] is the 2D-plot analogue of a [D3 scale]: it maps a value on
//! one axis from **data space** (what the app measures — a temperature, a
//! byte count, an epoch timestamp) to **scale space** (the linearized
//! coordinate the plot lays out in) and back. It is a *power-user
//! primitive* — pure, standalone, and usable without ever building an
//! [`El`](crate::tree::El): an app composing a custom chart from the public
//! pieces (see `docs/PLOT2D_PLAN.md`, Layer 3) reaches for `Scale` directly.
//!
//! ## Why "scale space"
//!
//! A plot pans and zooms by changing only a transform *uniform*, never
//! re-uploading the data (see the plan's decision 2). That works because a
//! pan/zoom of the visible window is **affine in scale space** — including
//! for a logarithmic axis, where panning is affine in `log(value)` even
//! though it is not in `value`. So every `Scale` exposes a
//! [`forward`](Scale::forward)/[`inverse`](Scale::inverse) warp; the plot
//! lays out, pans, and zooms in the forward (scale-space) coordinate and
//! only converts back to data space to label ticks and read out the
//! crosshair.
//!
//! [`Scale::Linear`] and [`Scale::Time`] share the identity warp — time is
//! linear in epoch seconds — and differ only in how ticks are chosen and
//! formatted. [`Scale::Log`] warps through `log`.
//!
//! [D3 scale]: https://d3js.org/d3-scale

#![warn(missing_docs)]

/// A continuous, invertible map from an axis's data space to scale space,
/// with tick generation and default value formatting.
///
/// Construct with [`Scale::linear`], [`Scale::time`], or [`Scale::log`] and
/// hand it to a plot axis (`plot(...).x(Scale::time())`). The visible domain
/// is *not* carried here — it lives in the plot's [`PlotView`](crate::plot::PlotView)
/// state, the same way a CSS `transform` is separate from the element it
/// applies to. A `Scale` is just the warp + tick policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// A linear (identity-warp) numeric axis. Ticks are "nice" numbers
    /// (multiples of 1/2/5 × 10ⁿ).
    Linear,
    /// A time axis. Data is **seconds since `epoch`** (a Unix-epoch-seconds
    /// reference applied only when ticks/readouts are labelled, in integer
    /// arithmetic). With `epoch: 0` ([`Scale::time`]) data is plain epoch
    /// seconds — but `f64` epoch seconds quantize at ~0.24 µs for
    /// present-day timestamps, so high-zoom plots (a logic analyzer at µs
    /// windows) should pass a nearby whole-second reference via
    /// [`Scale::time_from`] and plot small relative seconds instead; the
    /// axis still labels wall-clock. The warp is the identity (time is
    /// linear in seconds); ticks land on natural calendar/clock boundaries
    /// — down to decimal sub-second steps — and format as clock time
    /// (fractional seconds as needed) or dates. Labels read UTC unless a
    /// fixed local offset is folded in via [`Scale::utc_offset_secs`].
    Time {
        /// Unix-epoch **whole seconds** that data-space `0.0` corresponds
        /// to. Integer by construction — a whole-second epoch is what keeps
        /// sub-second tick alignment exact ([`Scale::time_from`] floors
        /// fractional input). Consumed *only* when ticks are aligned and
        /// labelled — never by the warp — which is why a fixed UTC offset
        /// ([`Scale::utc_offset_secs`]) can fold into it.
        epoch: i64,
    },
    /// A logarithmic axis. The warp is `log(value)`, so panning and zooming
    /// stay affine in scale space. The domain must stay strictly positive;
    /// non-positive values are clamped to a tiny epsilon by the warp.
    Log {
        /// The logarithm base (e.g. `10.0`). Tick placement uses powers of
        /// this base.
        base: f64,
    },
}

/// The smallest positive value [`Scale::Log`] will take a logarithm of, so
/// the warp never returns `-inf` for a clamped zero/negative input.
const LOG_EPSILON: f64 = 1e-300;

impl Scale {
    /// A linear numeric axis.
    pub fn linear() -> Self {
        Scale::Linear
    }

    /// A time axis over **epoch seconds** (no reference offset).
    pub fn time() -> Self {
        Scale::Time { epoch: 0 }
    }

    /// A time axis over **seconds relative to `epoch`** (Unix epoch
    /// seconds, floored to a whole second). Plotting small relative values
    /// keeps `f64` precision at any zoom — epoch seconds alone quantize at
    /// ~0.24 µs for present-day timestamps — while ticks and readouts still
    /// label absolute wall-clock time.
    pub fn time_from(epoch: f64) -> Self {
        Scale::Time {
            epoch: epoch.floor() as i64,
        }
    }

    /// Shift a time axis's tick alignment and labels by a fixed UTC
    /// `offset` in seconds (positive east of Greenwich, e.g. `-4 * 3600`
    /// for US Eastern daylight time), so day/hour ticks land on **local**
    /// midnight/hour boundaries and tick labels and crosshair readouts
    /// print local wall-clock time. Data stays plain UTC epoch seconds;
    /// the offset folds into the label reference
    /// ([`epoch`](Scale::Time::epoch)) and never touches the warp.
    ///
    /// Resolve the platform's *current* offset yourself (`tm_gmtoff`,
    /// `Date.getTimezoneOffset()`, …) and pass plain seconds: this is
    /// deliberately a fixed offset, not a timezone — a plotted window
    /// spanning a DST transition labels the far side an hour off, and
    /// damascene carries no tz database. Composes with
    /// [`Scale::time_from`] (the precision reference and the offset
    /// simply add). No effect on non-time scales.
    pub fn utc_offset_secs(self, offset: i64) -> Self {
        match self {
            Scale::Time { epoch } => Scale::Time {
                epoch: epoch + offset,
            },
            other => other,
        }
    }

    /// A base-10 logarithmic axis. Use [`Scale::Log`] directly for another
    /// base.
    pub fn log() -> Self {
        Scale::Log { base: 10.0 }
    }

    /// Warp a data-space value into scale space. Identity for
    /// [`Linear`](Scale::Linear)/[`Time`](Scale::Time); `log_base` for
    /// [`Log`](Scale::Log).
    pub fn forward(&self, v: f64) -> f64 {
        match self {
            Scale::Linear | Scale::Time { .. } => v,
            Scale::Log { base } => v.max(LOG_EPSILON).log(*base),
        }
    }

    /// Inverse of [`forward`](Scale::forward): map a scale-space coordinate
    /// back to data space.
    pub fn inverse(&self, u: f64) -> f64 {
        match self {
            Scale::Linear | Scale::Time { .. } => u,
            Scale::Log { base } => base.powf(u),
        }
    }

    /// Map a data value to a GPU-ready scale-space coordinate measured
    /// **relative to `origin`** (a data-space reference subtracted before
    /// the cast to `f32`). Subtracting a per-axis origin is what keeps large
    /// absolute coordinates — epoch timestamps especially — inside `f32`'s
    /// ~7 significant digits on the GPU (the plan's decision 7). Pass the
    /// same `origin` to [`invert`](Scale::invert).
    pub fn map(&self, v: f64, origin: f64) -> f32 {
        (self.forward(v) - self.forward(origin)) as f32
    }

    /// Inverse of [`map`](Scale::map): recover the data value from an
    /// origin-relative scale-space coordinate (e.g. for a crosshair
    /// readout).
    pub fn invert(&self, s: f32, origin: f64) -> f64 {
        self.inverse(self.forward(origin) + s as f64)
    }

    /// Generate axis ticks across the visible data window `(lo, hi)`,
    /// aiming for about `target` of them. Returns ticks in ascending data
    /// order, each with its data value and a default label. `lo`/`hi` are in
    /// data space; an empty or inverted window yields no ticks.
    pub fn ticks(&self, window: (f64, f64), target: usize) -> Vec<Tick> {
        let (lo, hi) = window;
        if !lo.is_finite() || !hi.is_finite() || hi <= lo {
            return Vec::new();
        }
        match self {
            Scale::Linear => linear_ticks(lo, hi, target.max(1))
                .into_iter()
                .map(|v| Tick {
                    value: v,
                    label: format_number(v, linear_tick_step(lo, hi, target.max(1))),
                })
                .collect(),
            Scale::Log { base } => log_ticks(lo, hi, *base, target.max(1))
                .into_iter()
                .map(|v| Tick {
                    value: v,
                    // Pass the tick's own magnitude as the precision context
                    // so sub-1 decades label as "0.1"/"0.01", not "0".
                    label: format_number(v, v.min(1.0)),
                })
                .collect(),
            Scale::Time { epoch } => time_ticks(*epoch, lo, hi, target.max(1)),
        }
    }

    /// Format a single data value the way this scale's ticks would, e.g. for
    /// a crosshair readout. `context` is a neighbouring span (typically the
    /// visible window width) used to choose precision; pass `0.0` to accept
    /// the default.
    pub fn format(&self, v: f64, context: f64) -> String {
        match self {
            Scale::Linear | Scale::Log { .. } => {
                let step = if context > 0.0 { context / 100.0 } else { 0.0 };
                format_number(v, step)
            }
            Scale::Time { epoch } => format_time_at(*epoch, v, context),
        }
    }
}

/// One axis tick: a position in data space and its formatted label.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// The tick's data-space position.
    pub value: f64,
    /// The label to draw at the tick.
    pub label: String,
}

// --- Linear "nice number" ticks -------------------------------------------

/// Round `rough` up to a "nice" step: 1, 2, 5, or 10 times a power of ten.
/// Classic "nice numbers" heuristic (Paul Heckbert, Graphics Gems, 1990).
fn nice_step(rough: f64) -> f64 {
    if !rough.is_finite() || rough <= 0.0 {
        return 1.0;
    }
    let pow = 10f64.powf(rough.log10().floor());
    let frac = rough / pow;
    let nice = if frac < 1.5 {
        1.0
    } else if frac < 3.0 {
        2.0
    } else if frac < 7.0 {
        5.0
    } else {
        10.0
    };
    nice * pow
}

/// The nice step a linear axis would use across `(lo, hi)` for `target`
/// ticks.
fn linear_tick_step(lo: f64, hi: f64, target: usize) -> f64 {
    nice_step((hi - lo) / target as f64)
}

/// Tick *values* for a linear axis across `(lo, hi)`.
fn linear_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    let step = linear_tick_step(lo, hi, target);
    if step <= 0.0 {
        return Vec::new();
    }
    let start = (lo / step).ceil() * step;
    let mut out = Vec::new();
    let mut t = start;
    // Guard the loop against pathological windows.
    let max_ticks = target.saturating_mul(4).max(8);
    while t <= hi + step * 1e-9 && out.len() < max_ticks {
        // Snap values that are a hair off a clean multiple (float drift).
        let snapped = (t / step).round() * step;
        out.push(snapped);
        t += step;
    }
    out
}

/// Format a number for a tick label, choosing decimals from `step`'s
/// magnitude (0 for an unknown step). Trims trailing zeros.
fn format_number(v: f64, step: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v }; // normalize -0.0
    let decimals = if step > 0.0 && step < 1.0 {
        (-step.log10().floor()).clamp(0.0, 12.0) as usize
    } else {
        0
    };
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

// --- Log ticks ------------------------------------------------------------

/// Tick values at integer powers of `base` spanning `(lo, hi)`, thinned to
/// roughly `target` of them by striding whole exponents when the window
/// spans more decades than that (a 60-decade window ticks every 10th decade,
/// not all 60).
fn log_ticks(lo: f64, hi: f64, base: f64, target: usize) -> Vec<f64> {
    let lo = lo.max(LOG_EPSILON);
    if hi <= lo || base <= 1.0 {
        return Vec::new();
    }
    // The in-range exponents: the smallest/largest `e` with `base^e` inside
    // the window.
    let lo_exp = lo.log(base).ceil() as i64;
    let hi_exp = hi.log(base).floor() as i64;
    if hi_exp < lo_exp {
        // The window sits inside a single decade — no power falls in it.
        return Vec::new();
    }
    let count = (hi_exp - lo_exp + 1) as usize;
    let step = count.div_ceil(target.max(1)).max(1) as i64;
    // Align to multiples of `step` so ticks hold still as the window pans.
    let mut e = lo_exp.div_euclid(step) * step;
    if e < lo_exp {
        e += step;
    }
    let mut out = Vec::new();
    while e <= hi_exp && out.len() < 64 {
        let v = base.powi(e as i32);
        // Belt-and-braces range check against float drift at the edges.
        if v >= lo && v <= hi {
            out.push(v);
        }
        e += step;
    }
    out
}

// --- Time ticks -----------------------------------------------------------

/// Seconds per minute/hour/day, for tick interval selection.
const MINUTE: f64 = 60.0;
const HOUR: f64 = 3600.0;
const DAY: f64 = 86_400.0;

/// Candidate tick intervals, in seconds, from one second up to ~a year.
/// Each divides the next cleanly enough that multiples land on natural
/// clock/calendar boundaries (in UTC).
const TIME_INTERVALS: &[f64] = &[
    1.0,
    2.0,
    5.0,
    10.0,
    15.0,
    30.0,
    MINUTE,
    2.0 * MINUTE,
    5.0 * MINUTE,
    10.0 * MINUTE,
    15.0 * MINUTE,
    30.0 * MINUTE,
    HOUR,
    2.0 * HOUR,
    3.0 * HOUR,
    6.0 * HOUR,
    12.0 * HOUR,
    DAY,
    2.0 * DAY,
    7.0 * DAY,
    14.0 * DAY,
    30.0 * DAY,
    90.0 * DAY,
    365.0 * DAY,
];

/// Ticks for a time axis over `(lo, hi)` seconds relative to `epoch` (Unix
/// whole seconds; `0` means the data itself is epoch seconds).
///
/// Sub-second spans get decimal 1/2/5 × 10ⁿ intervals. Because `epoch` is a
/// whole second and every decimal sub-second interval divides one second
/// evenly, aligning ticks in *relative* space keeps their positions exact —
/// aligning in absolute epoch seconds would re-import the ~0.24 µs `f64`
/// quantum this scale exists to avoid.
fn time_ticks(epoch: i64, lo: f64, hi: f64, target: usize) -> Vec<Tick> {
    let span = hi - lo;
    let rough = span / target as f64;
    let interval = if rough < 1.0 {
        nice_step(rough).min(1.0)
    } else {
        TIME_INTERVALS
            .iter()
            .copied()
            .find(|&i| i >= rough)
            .unwrap_or(365.0 * DAY)
    };
    // ≥ 1 s intervals align on absolute boundaries (calendar/clock ticks);
    // sub-second intervals align in relative space (exact, see above).
    let start = if interval >= 1.0 {
        ((epoch as f64 + lo) / interval).ceil() * interval - epoch as f64
    } else {
        (lo / interval).ceil() * interval
    };
    let mut out = Vec::new();
    let max_ticks = target.saturating_mul(4).max(8);
    let mut k = 0u32;
    loop {
        // Multiply out from `start` instead of accumulating `+= interval`,
        // so long runs of an inexact interval (1e-6…) don't drift.
        let t = start + k as f64 * interval;
        if t > hi + interval * 1e-9 || out.len() >= max_ticks {
            break;
        }
        out.push(Tick {
            value: t,
            label: format_time(epoch, t, interval),
        });
        k += 1;
    }
    out
}

/// Format `rel` seconds since the whole-second Unix `epoch` for a tick,
/// with granularity chosen from the tick `interval` (a date for multi-day
/// intervals, clock time otherwise, fractional seconds for sub-second
/// intervals). `epoch + rel` renders as UTC civil time — local-time axes
/// work by folding a fixed offset into `epoch` (`Scale::utc_offset_secs`),
/// not by any conversion here. The split `epoch + rel` arithmetic is
/// integer/small-float so present-day timestamps keep full precision.
fn format_time(epoch: i64, rel: f64, interval: f64) -> String {
    // Sub-second digits wanted: none for interval ≥ 1 s, else enough to
    // distinguish neighbouring ticks (1e-6 → 6, 5e-7 → 7).
    let decimals = if interval >= 1.0 {
        0
    } else {
        (-interval.log10()).ceil().clamp(1.0, 9.0) as u32
    };
    // Split into whole seconds + rounded fractional digits, carrying a
    // round-up across the second boundary.
    let quantum = 10f64.powi(decimals as i32);
    let rel_whole = rel.floor();
    let mut frac_units = ((rel - rel_whole) * quantum).round() as i64;
    let mut secs = epoch + rel_whole as i64;
    if frac_units >= quantum as i64 {
        frac_units = 0;
        secs += 1;
    }
    let days = secs.div_euclid(DAY as i64);
    let tod = secs.rem_euclid(DAY as i64); // seconds since UTC midnight
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, mo, d) = civil_from_days(days);
    if interval >= DAY {
        format!("{y:04}-{mo:02}-{d:02}")
    } else if interval >= MINUTE {
        format!("{h:02}:{m:02}")
    } else if decimals == 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!(
            "{h:02}:{m:02}:{s:02}.{frac_units:0width$}",
            width = decimals as usize
        )
    }
}

/// Format a data value (seconds relative to `epoch`) for the crosshair
/// readout: like a tick label whose interval is `context / 100` (the
/// visible-window span scaled to cursor precision), so deep zooms read out
/// fractional seconds.
fn format_time_at(epoch: i64, rel: f64, context: f64) -> String {
    let interval = if context > 0.0 { context / 100.0 } else { 1.0 };
    format_time(epoch, rel, interval.min(MINUTE))
}

/// Convert a day count relative to the Unix epoch (1970-01-01) to a civil
/// `(year, month, day)`, via Howard Hinnant's `civil_from_days` algorithm
/// (valid across the proleptic Gregorian calendar).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_warp_is_identity_and_invertible() {
        let s = Scale::linear();
        assert_eq!(s.forward(42.0), 42.0);
        assert_eq!(s.inverse(42.0), 42.0);
        // origin-relative map keeps precision near the origin
        assert_eq!(s.map(105.0, 100.0), 5.0);
        assert!((s.invert(5.0, 100.0) - 105.0).abs() < 1e-9);
    }

    #[test]
    fn log_warp_roundtrips_and_pan_is_affine_in_scale_space() {
        let s = Scale::log();
        assert!((s.forward(1000.0) - 3.0).abs() < 1e-12);
        assert!((s.inverse(3.0) - 1000.0).abs() < 1e-9);
        // equal ratios are equal distances in scale space (the whole point)
        let d1 = s.forward(100.0) - s.forward(10.0);
        let d2 = s.forward(1000.0) - s.forward(100.0);
        assert!((d1 - d2).abs() < 1e-12);
    }

    #[test]
    fn log_warp_clamps_nonpositive() {
        let s = Scale::log();
        assert!(s.forward(0.0).is_finite());
        assert!(s.forward(-5.0).is_finite());
    }

    #[test]
    fn linear_ticks_are_nice_and_in_range() {
        let s = Scale::linear();
        let ticks = s.ticks((0.0, 100.0), 5);
        assert!(!ticks.is_empty());
        for t in &ticks {
            assert!(t.value >= 0.0 && t.value <= 100.0);
        }
        // expect a step of 20 → 0,20,40,60,80,100
        assert_eq!(ticks.first().unwrap().value, 0.0);
        assert_eq!(ticks.last().unwrap().value, 100.0);
        assert_eq!(ticks[1].value, 20.0);
    }

    #[test]
    fn linear_ticks_fractional_labels_trim_zeros() {
        let s = Scale::linear();
        let ticks = s.ticks((0.0, 1.0), 5);
        // step 0.2 → labels like "0.2", not "0.200000"
        assert!(ticks.iter().any(|t| t.label == "0.2"));
        assert!(ticks.iter().any(|t| t.label == "0"));
    }

    #[test]
    fn nice_step_picks_1_2_5() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(2.3), 2.0);
        assert_eq!(nice_step(3.5), 5.0);
        assert_eq!(nice_step(0.04), 0.05);
        assert_eq!(nice_step(8.0), 10.0);
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // 2000-03-01 is 11017 days after the epoch
        assert_eq!(civil_from_days(11017), (2000, 3, 1));
        // 2026-06-24
        let days = 20628; // verified below by round-trip
        let (y, m, d) = civil_from_days(days);
        assert_eq!((y, m, d), (2026, 6, 24));
    }

    #[test]
    fn time_ticks_clock_format_for_minutes() {
        let s = Scale::time();
        // 2026-06-24 12:00:00 UTC .. +1h, expect HH:MM labels on clean
        // boundaries
        let base = 20628.0 * DAY + 12.0 * HOUR;
        let ticks = s.ticks((base, base + HOUR), 4);
        assert!(!ticks.is_empty());
        assert!(
            ticks
                .iter()
                .all(|t| t.label.len() == 5 && t.label.contains(':'))
        );
        // first tick at or after 12:00 on a clean 15-minute boundary
        assert!(ticks.iter().any(|t| t.label == "12:15"));
    }

    /// An epoch-relative time scale ticks sub-second windows on exact
    /// decimal boundaries and labels them as wall-clock with fractional
    /// seconds — the deep-zoom mode `f64` epoch seconds cannot reach.
    #[test]
    fn time_ticks_subsecond_with_epoch() {
        // 2026-06-24 12:00:00 UTC as the whole-second reference.
        let epoch = 20628.0 * DAY + 12.0 * HOUR;
        let s = Scale::time_from(epoch);
        // A 4 µs window starting 100 s + 1.2 µs after the reference.
        let lo = 100.0 + 1.2e-6;
        let ticks = s.ticks((lo, lo + 4e-6), 4);
        assert!(ticks.len() >= 3, "got {} ticks", ticks.len());
        // 1 µs boundaries; sub-picosecond placement error in relative space
        // (epoch-seconds data would be stuck on a 0.24 µs grid here).
        assert!((ticks[0].value - (100.0 + 2e-6)).abs() < 1e-12);
        assert!((ticks[1].value - ticks[0].value - 1e-6).abs() < 1e-12);
        // Labels are wall-clock seconds with micro digits.
        assert_eq!(ticks[0].label, "12:01:40.000002");
        // The crosshair readout formats the same way at this zoom.
        assert_eq!(s.format(100.0 + 2.5e-6, 4e-6), "12:01:40.00000250");
    }

    /// Epoch-relative ticks at ≥ 1 s intervals still land on absolute
    /// clock boundaries (the epoch phase is honoured, not ignored).
    #[test]
    fn time_ticks_epoch_relative_aligns_to_clock() {
        // Reference 7 s past the minute: relative ticks must land at
        // rel = 8, 23, 38, 53… (absolute :15/:30/:45/:00), not rel = 0/15/30.
        let epoch = 20628.0 * DAY + 12.0 * HOUR + 7.0;
        let s = Scale::time_from(epoch);
        let ticks = s.ticks((0.0, 60.0), 4);
        assert!(!ticks.is_empty());
        assert_eq!(ticks[0].value, 8.0);
        assert_eq!(ticks[0].label, "12:00:15");
        // Plain Scale::time() keeps its old absolute-seconds behavior.
        let abs = Scale::time().ticks((epoch, epoch + 60.0), 4);
        assert_eq!(abs[0].label, "12:00:15");
    }

    /// A fixed UTC offset shifts day-tick alignment onto *local* midnight
    /// and dates the labels in local time — for whole-hour and
    /// half-hour zones alike.
    #[test]
    fn time_ticks_utc_offset_aligns_days_to_local_midnight() {
        // 2026-06-24 00:00 UTC; data is plain epoch seconds.
        let base = 20628.0 * DAY;
        // US Eastern daylight time, UTC−4.
        let edt = Scale::time().utc_offset_secs(-4 * 3600);
        let ticks = edt.ticks((base, base + 3.0 * DAY), 3);
        assert!(!ticks.is_empty());
        // Local midnight is 04:00 UTC, so the first day tick sits there.
        assert_eq!(ticks[0].value - base, 4.0 * HOUR);
        assert_eq!(ticks[0].label, "2026-06-24");
        // India, UTC+5:30: local midnight is 18:30 UTC the evening before.
        let ist = Scale::time().utc_offset_secs(5 * 3600 + 1800);
        let ticks = ist.ticks((base, base + 3.0 * DAY), 3);
        assert_eq!(ticks[0].value - base, DAY - (5.0 * HOUR + 1800.0));
        assert_eq!(ticks[0].label, "2026-06-25");
    }

    /// Clock-time tick labels and the crosshair readout both print local
    /// wall-clock under a UTC offset (they share `format_time`).
    #[test]
    fn time_labels_and_readout_honor_utc_offset() {
        let s = Scale::time().utc_offset_secs(-4 * 3600);
        // 2026-06-24 12:00–13:00 UTC is 08:00–09:00 local.
        let noon = 20628.0 * DAY + 12.0 * HOUR;
        let ticks = s.ticks((noon, noon + HOUR), 4);
        assert_eq!(ticks[0].value, noon);
        assert_eq!(ticks[0].label, "08:00");
        assert_eq!(s.format(noon, HOUR), "08:00:00");
    }

    /// The offset folds into the epoch reference (composing with
    /// `time_from`) and is a no-op on non-time scales.
    #[test]
    fn utc_offset_folds_into_epoch() {
        let epoch = 20628 * DAY as i64;
        assert_eq!(
            Scale::time_from(epoch as f64).utc_offset_secs(-14_400),
            Scale::Time {
                epoch: epoch - 14_400
            }
        );
        assert_eq!(Scale::linear().utc_offset_secs(3600), Scale::linear());
        assert_eq!(Scale::log().utc_offset_secs(3600), Scale::log());
    }

    /// A fractional-second label rounding up across the second boundary
    /// carries into the wall-clock seconds instead of printing ".1000000".
    #[test]
    fn time_format_carries_round_up() {
        let epoch = 20628.0 * DAY + 12.0 * HOUR;
        let s = Scale::time_from(epoch);
        assert_eq!(s.format(0.999_999_96, 1e-5), "12:00:01.0000000");
    }

    #[test]
    fn time_ticks_date_format_for_multiday() {
        let s = Scale::time();
        let base = 20628.0 * DAY;
        let ticks = s.ticks((base, base + 10.0 * DAY), 5);
        assert!(!ticks.is_empty());
        assert!(ticks.iter().all(|t| t.label.starts_with("2026-")));
    }

    #[test]
    fn log_ticks_are_decades_in_range() {
        let s = Scale::log();
        let ticks = s.ticks((0.574, 114_000.0), 6);
        let values: Vec<f64> = ticks.iter().map(|t| t.value).collect();
        assert_eq!(values, vec![1.0, 10.0, 100.0, 1000.0, 10_000.0, 100_000.0]);
        assert_eq!(ticks[0].label, "1");
        assert_eq!(ticks[5].label, "100000");
    }

    #[test]
    fn log_ticks_label_sub_one_decades() {
        let s = Scale::log();
        let ticks = s.ticks((0.005, 20.0), 6);
        let labels: Vec<&str> = ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec!["0.01", "0.1", "1", "10"]);
    }

    #[test]
    fn log_ticks_thin_wide_windows_toward_target() {
        let s = Scale::log();
        let ticks = s.ticks((1e-30, 1e30), 6);
        assert!(
            (3..=8).contains(&ticks.len()),
            "61 decades thinned to ~target: {}",
            ticks.len()
        );
        // Strided decades stay powers of ten, ascending.
        for w in ticks.windows(2) {
            assert!(w[1].value > w[0].value);
        }
    }

    #[test]
    fn log_window_inside_one_decade_has_no_ticks() {
        let s = Scale::log();
        assert!(s.ticks((2.0, 8.0), 6).is_empty());
    }

    #[test]
    fn empty_or_inverted_window_has_no_ticks() {
        let s = Scale::linear();
        assert!(s.ticks((5.0, 5.0), 5).is_empty());
        assert!(s.ticks((10.0, 0.0), 5).is_empty());
    }
}
