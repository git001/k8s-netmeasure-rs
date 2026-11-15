//! Rates per connect (design §10.4): failures (`timeout` + `refused` + `net_error`)
//! and SYN retransmissions. Their difference and ratio go through the same
//! hierarchical block bootstrap as the latency quantiles. Below [`MIN_EVENTS`] events
//! on both sides together the result is descriptive only, with the exact
//! Clopper–Pearson interval per side, which **assumes independence**.

use super::binomial;
use super::verdict::{Levels, Verdict, interval};

/// Events needed on both sides together before an interval or verdict (§10.4).
pub const MIN_EVENTS: u64 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct RateSide {
    pub events: u64,
    pub attempts: u64,
    pub rate: f64,
    /// Clopper–Pearson at 95 %; descriptive, assumes independent connects.
    pub exact: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RateComparison {
    pub reference: RateSide,
    pub after: RateSide,
    /// Bootstrap intervals of `after − reference` and `after / reference`; `None`
    /// below [`MIN_EVENTS`].
    pub difference: Option<(f64, f64)>,
    pub ratio: Option<(f64, f64)>,
    /// Failures only; SYN retransmissions are diagnostics and never get one.
    pub verdict: Option<Verdict>,
}

pub fn side(events: u64, attempts: u64) -> RateSide {
    RateSide {
        events,
        attempts,
        rate: if attempts == 0 {
            0.0
        } else {
            events as f64 / attempts as f64
        },
        exact: clopper_pearson(events, attempts, 0.95),
    }
}

/// `(events, attempts)` of one side.
pub type Counts = (u64, u64);

/// Compares two sides from their observed counts and bootstrap replicates
/// (`(events, attempts)` per replicate). `with_verdict` for the failure family.
pub fn compare(
    reference: Counts,
    after: Counts,
    replicates: &[(Counts, Counts)],
    levels: Levels,
    with_verdict: bool,
) -> RateComparison {
    let enough = reference.0 + after.0 >= MIN_EVENTS;
    let r = |(e, n): (u64, u64)| {
        if n == 0 {
            f64::NAN
        } else {
            e as f64 / n as f64
        }
    };
    let diffs: Vec<f64> = replicates.iter().map(|&(a, b)| r(b) - r(a)).collect();
    let ratios: Vec<f64> = replicates
        .iter()
        .map(|&(a, b)| match (r(a), r(b)) {
            (0.0, 0.0) => 1.0,
            (0.0, rb) => f64::INFINITY.copysign(rb),
            (ra, rb) => rb / ra,
        })
        .collect();
    let difference = enough.then(|| interval(&diffs, levels.two_sided)).flatten();
    let verdict = (with_verdict && enough).then_some(match difference {
        Some((lo, _)) if lo > 0.0 => Verdict::Worse,
        Some((_, hi)) if hi < 0.0 => Verdict::Better,
        _ => Verdict::Inconclusive { censored: false },
    });
    RateComparison {
        reference: side(reference.0, reference.1),
        after: side(after.0, after.1),
        difference,
        ratio: enough
            .then(|| interval(&ratios, levels.two_sided))
            .flatten(),
        verdict,
    }
}

/// Exact two-sided interval for a binomial proportion, by bisection on the CDF.
pub fn clopper_pearson(events: u64, attempts: u64, level: f64) -> Option<(f64, f64)> {
    if attempts == 0 || events > attempts {
        return None;
    }
    let tail = (1.0 - level) / 2.0;
    // P(X ≥ events | p) = tail for the lower bound, P(X ≤ events | p) = tail upper.
    let lower = if events == 0 {
        0.0
    } else {
        bisect(|p| 1.0 - cdf_at(attempts, p, events - 1) - tail)
    };
    let upper = if events == attempts {
        1.0
    } else {
        bisect(|p| tail - cdf_at(attempts, p, events))
    };
    Some((lower, upper))
}

fn cdf_at(n: u64, p: f64, k: u64) -> f64 {
    binomial::cdf(n, p).map_or(0.0, |c| c[k as usize])
}

/// Root of an increasing `f` on (0, 1).
fn bisect(f: impl Fn(f64) -> f64) -> f64 {
    let (mut lo, mut hi) = (1e-12, 1.0 - 1e-12);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if f(mid) < 0.0 { lo = mid } else { hi = mid }
    }
    (lo + hi) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clopper_pearson_known_values() {
        // 0 of 10: upper bound 1 − 0.025^(1/10) ≈ 0.3085.
        let (lo, hi) = clopper_pearson(0, 10, 0.95).unwrap();
        assert_eq!(lo, 0.0);
        assert!((hi - 0.3085).abs() < 1e-3, "{hi}");
        // 5 of 10: (0.1871, 0.8129).
        let (lo, hi) = clopper_pearson(5, 10, 0.95).unwrap();
        assert!(
            (lo - 0.1871).abs() < 1e-3 && (hi - 0.8129).abs() < 1e-3,
            "{lo} {hi}"
        );
        assert_eq!(clopper_pearson(1, 0, 0.95), None);
        assert_eq!(clopper_pearson(10, 10, 0.95).unwrap().1, 1.0);
    }

    #[test]
    fn few_events_are_descriptive_only() {
        let reps = vec![((1, 1000), (2, 1000)); 50];
        let c = compare((1, 1000), (2, 1000), &reps, Levels::bonferroni(1), true);
        assert_eq!((c.difference, c.ratio, c.verdict), (None, None, None));
        assert!(c.reference.exact.is_some());
    }

    #[test]
    fn enough_events_give_a_verdict() {
        let reps: Vec<_> = (0..100)
            .map(|i| ((5 + i % 3, 1000), (40 + i % 5, 1000)))
            .collect();
        let c = compare((6, 1000), (42, 1000), &reps, Levels::bonferroni(1), true);
        assert_eq!(c.verdict, Some(Verdict::Worse));
        let (lo, hi) = c.ratio.unwrap();
        assert!(lo > 1.0 && hi.is_finite());
        let diag = compare((6, 1000), (42, 1000), &reps, Levels::bonferroni(1), false);
        assert_eq!(diag.verdict, None);
        assert!(diag.difference.is_some());
    }
}
