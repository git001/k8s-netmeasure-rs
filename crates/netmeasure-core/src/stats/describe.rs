//! One side's quantile, described (design §10.1): the type-1 estimate with censoring,
//! the distribution-free order-statistic interval, and the tail minimum.
//!
//! The order-statistic interval **assumes independent samples**; under
//! autocorrelation it is too narrow. It is shown for orientation only and never
//! decides a verdict (the block bootstrap does, §10.2).

use super::binomial;
use super::quantile::type1;
use super::series::CENSORED;

/// Minimum expected exceedances above the quantile (§10.1): relative standard error
/// of their count 1/√40 ≈ 16 %.
pub const TAIL_MINIMUM: u64 = 40;

/// A quantile value: measured, or censored by the connect timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estimate {
    Ns(u64),
    /// The order statistic is a timeout: the quantile is at least the timeout.
    AtLeast(u64),
}

impl Estimate {
    fn of(value: u64, timeout_ns: u64) -> Self {
        if value == CENSORED {
            Estimate::AtLeast(timeout_ns)
        } else {
            Estimate::Ns(value)
        }
    }
}

/// Order-statistic interval with its exact binomial coverage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderInterval {
    pub lower: Estimate,
    pub upper: Estimate,
    pub coverage: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Described {
    pub p: f64,
    pub n: u64,
    pub estimate: Option<Estimate>,
    /// Expected samples above the quantile, `n × (1 − p)`.
    pub exceedances: f64,
    /// `None` below the tail minimum, or when no symmetric interval exists at `n`.
    pub interval: Option<OrderInterval>,
    /// Samples needed for the tail minimum, `⌈m / (1 − p)⌉`; set when `n` is short.
    pub required_n: Option<u64>,
}

/// `sorted` holds one side's latencies ascending, timeouts as [`CENSORED`].
pub fn describe(sorted: &[u64], p: f64, timeout_ns: u64, level: f64) -> Described {
    let n = sorted.len() as u64;
    let exceedances = n as f64 * (1.0 - p);
    let short = exceedances < TAIL_MINIMUM as f64;
    Described {
        p,
        n,
        estimate: type1(sorted, p).map(|v| Estimate::of(v, timeout_ns)),
        exceedances,
        interval: if short {
            None
        } else {
            order_interval(sorted, p, level, timeout_ns)
        },
        required_n: short.then(|| (TAIL_MINIMUM as f64 / (1.0 - p)).ceil() as u64),
    }
}

/// Ranks `(l, u)` (1-based) with `P(l ≤ B ≤ u − 1) ≥ level`, `B ~ Bin(n, p)`, each
/// tail ≤ (1 − level)/2: `x_(l) ≤ q_p ≤ x_(u)` with at least that coverage.
pub fn order_interval(
    sorted: &[u64],
    p: f64,
    level: f64,
    timeout_ns: u64,
) -> Option<OrderInterval> {
    let n = sorted.len() as u64;
    let cdf = binomial::cdf(n, p)?;
    let tail = (1.0 - level) / 2.0;
    // F(l − 1) ≤ tail, l as large as possible.
    let l = (1..=n)
        .take_while(|&l| cdf[(l - 1) as usize] <= tail)
        .last()?;
    // F(u − 1) ≥ 1 − tail, u as small as possible.
    let u = (1..=n).find(|&u| cdf[(u - 1) as usize] >= 1.0 - tail)?;
    Some(OrderInterval {
        lower: Estimate::of(sorted[(l - 1) as usize], timeout_ns),
        upper: Estimate::of(sorted[(u - 1) as usize], timeout_ns),
        coverage: cdf[(u - 1) as usize] - cdf[(l - 1) as usize],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_brackets_the_estimate_with_its_coverage() {
        let x: Vec<u64> = (1..=5000).collect();
        let d = describe(&x, 0.99, 5, 0.95);
        assert_eq!(d.estimate, Some(Estimate::Ns(4950)));
        let i = d.interval.unwrap();
        let (Estimate::Ns(lo), Estimate::Ns(hi)) = (i.lower, i.upper) else {
            panic!("{i:?}")
        };
        assert!(lo < 4950 && 4950 < hi, "{lo}..{hi}");
        // ±2 sd of Bin(5000, 0.99) ≈ ±14 ranks.
        assert!(
            (4930..4945).contains(&lo) && (4955..4970).contains(&hi),
            "{lo}..{hi}"
        );
        assert!(i.coverage >= 0.95 && i.coverage < 0.97, "{}", i.coverage);
        assert_eq!(d.required_n, None);
    }

    #[test]
    fn short_tail_gives_required_n_instead_of_an_interval() {
        let x: Vec<u64> = (1..=1000).collect();
        let d = describe(&x, 0.99, 5, 0.95);
        assert_eq!(d.interval, None);
        assert_eq!(d.required_n, Some(4000));
        assert_eq!(d.estimate, Some(Estimate::Ns(990)));
    }

    #[test]
    fn censored_order_statistics_read_at_least_timeout() {
        let mut x: Vec<u64> = (1..=4900).collect();
        x.extend(std::iter::repeat_n(CENSORED, 100)); // 2 % timeouts
        let d = describe(&x, 0.99, 5_000_000_000, 0.95);
        assert_eq!(d.estimate, Some(Estimate::AtLeast(5_000_000_000)));
        let i = d.interval.unwrap();
        assert_eq!(i.upper, Estimate::AtLeast(5_000_000_000));
        assert_eq!(i.lower, Estimate::AtLeast(5_000_000_000));
        let p90 = describe(&x, 0.9, 5_000_000_000, 0.95);
        assert_eq!(p90.estimate, Some(Estimate::Ns(4500)));
    }

    #[test]
    fn too_few_samples_for_any_interval() {
        assert_eq!(order_interval(&[1, 2, 3], 0.5, 0.95, 1), None);
        assert_eq!(describe(&[], 0.5, 1, 0.95).estimate, None);
    }
}
