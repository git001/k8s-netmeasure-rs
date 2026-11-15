//! Verdicts (design §10.4): `better` / `worse` from the two-sided interval, `equivalent`
//! by TOST inside the margin, `inconclusive` otherwise; the censoring rules; the
//! Bonferroni levels across `k` independent verdicts. Higher latency is worse.

use super::segment::CENSORED;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Better,
    Worse,
    /// TOST inside the margin; `significant` when the two-sided interval also
    /// excludes "no change" ("statistically significant, practically irrelevant").
    Equivalent {
        significant: bool,
    },
    Inconclusive {
        censored: bool,
    },
}

/// The compared quantity: `after − reference` in ns, or `after / reference`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Margin {
    AbsoluteNs(u64),
    /// Relative margin `m`: equivalence bounds `[1 − m, 1 + m]` on the ratio.
    Relative(f64),
}

impl Margin {
    /// "No change" and the equivalence bounds on the compared quantity.
    fn bounds(self) -> (f64, (f64, f64)) {
        match self {
            Margin::AbsoluteNs(m) => (0.0, (-(m as f64), m as f64)),
            Margin::Relative(m) => (1.0, (1.0 - m, 1.0 + m)),
        }
    }
}

/// Two-sided levels for one verdict among `k` (§10.4): `1 − 0.05/k` for
/// better/worse, `1 − 2·0.05/k` for the TOST interval (90 % at k = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    pub k: u32,
    pub two_sided: f64,
    pub tost: f64,
}

impl Levels {
    pub fn bonferroni(k: u32) -> Self {
        let alpha = 0.05 / f64::from(k.max(1));
        Levels {
            k: k.max(1),
            two_sided: 1.0 - alpha,
            tost: 1.0 - 2.0 * alpha,
        }
    }
}

/// `after − reference` or `after / reference` as `f64`; a censored side is +∞ (the
/// true value is only larger), both censored is undefined (NaN).
pub fn effect(reference: u64, after: u64, ratio: bool) -> f64 {
    let (r, a) = (as_f64(reference), as_f64(after));
    match (ratio, r.is_infinite(), a.is_infinite()) {
        (_, true, true) => f64::NAN,
        (false, ..) => a - r,
        (true, _, true) => f64::INFINITY,
        (true, true, false) => 0.0,
        (true, false, false) if r == 0.0 => f64::NAN,
        (true, false, false) => a / r,
    }
}

fn as_f64(v: u64) -> f64 {
    if v == CENSORED {
        f64::INFINITY
    } else {
        v as f64
    }
}

/// Percentile interval of effect replicates; `None` when any replicate is undefined.
pub fn interval(replicates: &[f64], level: f64) -> Option<(f64, f64)> {
    if replicates.is_empty() || replicates.iter().any(|x| x.is_nan()) {
        return None;
    }
    let mut r = replicates.to_vec();
    r.sort_by(f64::total_cmp);
    let at = |q: f64| r[((r.len() as f64 * q).ceil() as usize).clamp(1, r.len()) - 1];
    let tail = (1.0 - level) / 2.0;
    Some((at(tail), at(1.0 - tail)))
}

/// The verdict from the two-sided interval and the TOST interval (§10.4).
pub fn decide(
    two_sided: Option<(f64, f64)>,
    tost: Option<(f64, f64)>,
    margin: Option<Margin>,
    ratio: bool,
) -> Verdict {
    let null = if ratio { 1.0 } else { 0.0 };
    let Some((lo, hi)) = two_sided else {
        return Verdict::Inconclusive { censored: true };
    };
    if let (Some(m), Some((tlo, thi))) = (margin, tost) {
        let (_, (blo, bhi)) = m.bounds();
        if tlo.is_finite() && thi.is_finite() && blo <= tlo && thi <= bhi {
            let significant = (lo > null) || (hi < null);
            return Verdict::Equivalent { significant };
        }
    }
    if lo.is_finite() && lo > null {
        Verdict::Worse
    } else if hi.is_finite() && hi < null {
        Verdict::Better
    } else {
        Verdict::Inconclusive {
            censored: lo.is_infinite() || hi.is_infinite(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_bonferroni() {
        let one = Levels::bonferroni(1);
        assert!((one.two_sided - 0.95).abs() < 1e-12 && (one.tost - 0.90).abs() < 1e-12);
        let five = Levels::bonferroni(5);
        assert!((five.two_sided - 0.99).abs() < 1e-12 && (five.tost - 0.98).abs() < 1e-12);
        assert_eq!(Levels::bonferroni(0).k, 1);
    }

    #[test]
    fn effects_with_censoring() {
        assert_eq!(effect(100, 150, false), 50.0);
        assert_eq!(effect(100, 150, true), 1.5);
        assert_eq!(effect(100, CENSORED, false), f64::INFINITY);
        assert_eq!(effect(CENSORED, 100, false), f64::NEG_INFINITY);
        assert_eq!(effect(CENSORED, 100, true), 0.0);
        assert!(effect(CENSORED, CENSORED, true).is_nan());
        assert!(effect(0, 5, true).is_nan());
    }

    #[test]
    fn verdict_rules() {
        let m = Some(Margin::AbsoluteNs(100));
        let d = |two, tost, margin| decide(Some(two), Some(tost), margin, false);
        assert_eq!(
            d((10.0, 50.0), (15.0, 45.0), m),
            Verdict::Equivalent { significant: true }
        );
        assert_eq!(
            d((-20.0, 50.0), (-10.0, 40.0), m),
            Verdict::Equivalent { significant: false }
        );
        assert_eq!(d((150.0, 300.0), (160.0, 290.0), m), Verdict::Worse);
        assert_eq!(d((-300.0, -150.0), (-290.0, -160.0), m), Verdict::Better);
        assert_eq!(
            d((-50.0, 200.0), (-40.0, 190.0), m),
            Verdict::Inconclusive { censored: false }
        );
        // Without a margin there is never `equivalent`.
        assert_eq!(d((10.0, 50.0), (15.0, 45.0), None), Verdict::Worse);
        assert_eq!(
            d((-5.0, 5.0), (-4.0, 4.0), None),
            Verdict::Inconclusive { censored: false }
        );
    }

    #[test]
    fn censoring_rules() {
        let m = Some(Margin::AbsoluteNs(100));
        // Lower bound finite and positive: worse even though the upper bound is ∞.
        let worse = decide(
            Some((500.0, f64::INFINITY)),
            Some((600.0, f64::INFINITY)),
            m,
            false,
        );
        assert_eq!(worse, Verdict::Worse);
        // Never equivalent with an infinite TOST bound.
        let open = decide(
            Some((-10.0, f64::INFINITY)),
            Some((-5.0, f64::INFINITY)),
            m,
            false,
        );
        assert_eq!(open, Verdict::Inconclusive { censored: true });
        assert_eq!(
            decide(None, None, m, true),
            Verdict::Inconclusive { censored: true }
        );
        assert_eq!(interval(&[1.0, f64::NAN], 0.95), None);
        let r: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(interval(&r, 0.9), Some((5.0, 95.0)));
    }

    #[test]
    fn relative_margin_uses_the_ratio() {
        let m = Some(Margin::Relative(0.1));
        let eq = decide(Some((0.97, 1.05)), Some((0.98, 1.04)), m, true);
        assert_eq!(eq, Verdict::Equivalent { significant: false });
        let worse = decide(Some((1.12, 1.3)), Some((1.13, 1.28)), m, true);
        assert_eq!(worse, Verdict::Worse);
    }
}
