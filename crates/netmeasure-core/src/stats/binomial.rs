//! Exact binomial CDF for the order-statistic interval (design §10.1), computed in
//! log space so that `n` in the tens of thousands and `p` near 1 neither underflow
//! nor lose the tail. No external crate: the interval needs nothing else.

/// `[P(B ≤ 0), …, P(B ≤ n)]` for `B ~ Binomial(n, p)`; `None` unless `0 < p < 1`.
pub fn cdf(n: u64, p: f64) -> Option<Vec<f64>> {
    if !(p > 0.0 && p < 1.0) {
        return None;
    }
    let (lp, lq) = (p.ln(), (1.0 - p).ln());
    let mut log_pmf = n as f64 * lq; // k = 0
    let mut log_cdf = log_pmf;
    let mut out = Vec::with_capacity(n as usize + 1);
    out.push(log_cdf.exp());
    for k in 0..n {
        log_pmf += ((n - k) as f64).ln() - ((k + 1) as f64).ln() + lp - lq;
        log_cdf = log_add(log_cdf, log_pmf);
        out.push(log_cdf.exp().min(1.0));
    }
    Some(out)
}

/// `ln(e^a + e^b)` without overflow.
fn log_add(a: f64, b: f64) -> f64 {
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    if lo == f64::NEG_INFINITY {
        return hi;
    }
    hi + (lo - hi).exp().ln_1p()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }

    #[test]
    fn small_cases_match_closed_forms() {
        let c = cdf(3, 0.5).unwrap();
        for (got, want) in c.iter().zip([0.125, 0.5, 0.875, 1.0]) {
            close(*got, want);
        }
        let c = cdf(10, 0.3).unwrap();
        close(c[0], 0.7f64.powi(10));
        close(c[10], 1.0);
    }

    #[test]
    fn large_n_and_extreme_p_stay_finite() {
        let c = cdf(5000, 0.99).unwrap();
        assert!(c.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)));
        assert!(c.windows(2).all(|w| w[0] <= w[1]));
        // Mean 4950, sd ≈ 7.04: about half the mass at or below the mean.
        assert!((c[4950] - 0.5).abs() < 0.06, "{}", c[4950]);
        close(c[5000], 1.0);
        assert!(c[4900] < 1e-9);
    }

    #[test]
    fn invalid_p() {
        assert!(cdf(5, 0.0).is_none());
        assert!(cdf(5, 1.0).is_none());
        assert!(cdf(5, f64::NAN).is_none());
    }
}
