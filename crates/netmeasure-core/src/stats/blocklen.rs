//! Mean block length of the stationary bootstrap (design §10.2): Politis & White
//! (2004) with the correction of Patton, Politis & White (2009), applied to the
//! exceedance indicator `1{x_t > q̂_p}` of one target's series. Autocovariances are
//! pooled over the segments (cycles) and never cross a segment boundary.
//! Then the floor (wall time, an assumption about disturbance time scales) and the
//! cap (`n / 10`, at least ten blocks per run) are applied.

/// Which rule set the applied block length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Determined {
    Data,
    Floor,
    Cap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockChoice {
    /// The Politis–White estimate in samples.
    pub raw: f64,
    /// The value used, in samples (≥ 1).
    pub applied: f64,
    pub determined_by: Determined,
    /// Floor > cap: the run is too short for the floor assumption (report warning).
    pub floor_cap_conflict: bool,
    /// Autocorrelation of the indicator, lags 1..=`ACF_LAGS` (transparency).
    pub acf: Vec<f64>,
}

/// Lags printed for transparency (§10.2).
pub const ACF_LAGS: usize = 10;

/// Pooled autocovariance at `lag` of the centred indicator, over all segments.
fn autocov(segments: &[Vec<f64>], mean: f64, n: usize, lag: usize) -> f64 {
    let sum: f64 = segments
        .iter()
        .filter(|s| s.len() > lag)
        .map(|s| {
            (0..s.len() - lag)
                .map(|t| (s[t] - mean) * (s[t + lag] - mean))
                .sum::<f64>()
        })
        .sum();
    sum / n as f64
}

/// Politis–White estimate for the stationary bootstrap, in samples (≥ 1).
pub fn politis_white(indicator: &[Vec<f64>]) -> f64 {
    let n: usize = indicator.iter().map(Vec::len).sum();
    if n < 2 {
        return 1.0;
    }
    let mean = indicator.iter().flatten().sum::<f64>() / n as f64;
    let r0 = autocov(indicator, mean, n, 0);
    if r0 <= 0.0 {
        return 1.0; // constant indicator: no dependence to measure
    }
    let max_lag = (n - 1).min(((n as f64).sqrt().ceil() as usize).max(5) * 3);
    let r: Vec<f64> = (0..=max_lag)
        .map(|k| autocov(indicator, mean, n, k))
        .collect();
    let rho = |k: usize| r[k] / r0;
    // m̂: first lag after which K_N consecutive autocorrelations are insignificant.
    let kn = ((n as f64).log10().sqrt().ceil() as usize).max(5);
    let bound = 2.0 * ((n as f64).log10() / n as f64).sqrt();
    let m_hat = (0..=max_lag.saturating_sub(kn))
        .find(|&m| (1..=kn).all(|k| rho(m + k).abs() < bound))
        .unwrap_or(max_lag.saturating_sub(kn));
    let big_m = (2 * m_hat).clamp(1, max_lag);
    // Flat-top lag window.
    let lambda = |t: f64| match t.abs() {
        a if a <= 0.5 => 1.0,
        a if a <= 1.0 => 2.0 * (1.0 - a),
        _ => 0.0,
    };
    let (mut g, mut sigma2) = (0.0, r0);
    for (k, rk) in r.iter().enumerate().take(big_m + 1).skip(1) {
        let w = lambda(k as f64 / big_m as f64);
        g += 2.0 * w * k as f64 * rk;
        sigma2 += 2.0 * w * rk;
    }
    // Stationary bootstrap, PPW 2009: D_SB = 2 ĝ(0)², b = (2 Ĝ² / D_SB)^(1/3) n^(1/3).
    let d_sb = 2.0 * sigma2 * sigma2;
    if d_sb <= 0.0 {
        return 1.0;
    }
    ((2.0 * g * g / d_sb).cbrt() * (n as f64).cbrt()).max(1.0)
}

/// Default floor (§10.2): an assumption about the time scale of network
/// disturbances, not a statistical constant.
pub const FLOOR_NS: u64 = 5_000_000_000;

/// Mean start-to-start spacing of one target's samples: real targets share the
/// non-loopback slots round-robin, the loopback control gets every
/// `loopback_every`-th slot (§3.4).
pub fn spacing_ns(interval_ns: u64, real_targets: u64, loopback_every: u64, loopback: bool) -> f64 {
    let every = loopback_every.max(2) as f64;
    if loopback {
        interval_ns as f64 * every
    } else {
        interval_ns as f64 * real_targets.max(1) as f64 * every / (every - 1.0)
    }
}

/// The floor in samples of a series with the given spacing.
pub fn floor_samples(floor_ns: u64, spacing_ns: f64) -> f64 {
    (floor_ns as f64 / spacing_ns.max(1.0)).max(1.0)
}

/// The indicator `1{x > q}` per segment.
pub fn indicator(segments: &[&[u64]], q: u64) -> Vec<Vec<f64>> {
    segments
        .iter()
        .map(|s| s.iter().map(|&x| f64::from(u8::from(x > q))).collect())
        .collect()
}

/// Estimate, floor and cap (§10.2). `floor` is the number of samples spanning the
/// floor's wall time; the cap is `n / 10`.
pub fn choose(indicator: &[Vec<f64>], floor: f64, override_len: Option<f64>) -> BlockChoice {
    let n: usize = indicator.iter().map(Vec::len).sum();
    let raw = politis_white(indicator);
    let cap = (n as f64 / 10.0).max(1.0);
    let conflict = floor > cap;
    let (applied, determined_by) = match override_len {
        Some(len) => (len.max(1.0), Determined::Data),
        None if raw > cap || conflict => (cap, Determined::Cap),
        None if raw < floor => (floor, Determined::Floor),
        None => (raw, Determined::Data),
    };
    let acf = acf(indicator, n);
    BlockChoice {
        raw,
        applied,
        determined_by,
        floor_cap_conflict: conflict,
        acf,
    }
}

fn acf(indicator: &[Vec<f64>], n: usize) -> Vec<f64> {
    if n < 2 {
        return Vec::new();
    }
    let mean = indicator.iter().flatten().sum::<f64>() / n as f64;
    let r0 = autocov(indicator, mean, n, 0);
    (1..=ACF_LAGS.min(n - 1))
        .map(|k| {
            if r0 > 0.0 {
                autocov(indicator, mean, n, k) / r0
            } else {
                0.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngExt, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    /// Indicator of a Markov chain that stays in its state with probability `stay`.
    fn markov(n: usize, stay: f64, seed: u64) -> Vec<f64> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut state = 0.0;
        (0..n)
            .map(|_| {
                if rng.random::<f64>() > stay {
                    state = 1.0 - state;
                }
                state
            })
            .collect()
    }

    #[test]
    fn independent_data_gives_short_blocks_dependent_data_long_ones() {
        let iid = politis_white(&[markov(5000, 0.5, 1)]);
        let dep = politis_white(&[markov(5000, 0.98, 2)]);
        assert!(iid < 3.0, "{iid}");
        assert!(dep > 10.0 * iid, "{dep} vs {iid}");
    }

    #[test]
    fn segments_do_not_leak_dependence_across_boundaries() {
        // Each segment constant on its own; pooled acf at lag 1 stays defined.
        let segs = vec![vec![0.0; 50], vec![1.0; 50]];
        let c = choose(&segs, 1.0, None);
        assert!(c.raw >= 1.0);
        assert_eq!(c.acf.len(), ACF_LAGS);
    }

    #[test]
    fn floor_cap_and_override() {
        let data = vec![markov(1000, 0.5, 3)];
        let floored = choose(&data, 20.0, None);
        assert_eq!(
            (floored.applied, floored.determined_by),
            (20.0, Determined::Floor)
        );
        let conflict = choose(&data, 500.0, None);
        assert_eq!(
            (conflict.applied, conflict.determined_by),
            (100.0, Determined::Cap)
        );
        assert!(conflict.floor_cap_conflict);
        let fixed = choose(&data, 20.0, Some(7.0));
        assert_eq!(fixed.applied, 7.0);
        assert_eq!(politis_white(&[vec![1.0; 10]]), 1.0);
        assert_eq!(politis_white(&[]), 1.0);
    }

    #[test]
    fn floor_in_samples_follows_the_slot_schedule() {
        // Two targets, 10 ms, loopback every 10th slot: 22.2 ms per target sample.
        let s = spacing_ns(10_000_000, 2, 10, false);
        assert!((s - 22_222_222.2).abs() < 1.0, "{s}");
        assert!((floor_samples(FLOOR_NS, s) - 225.0).abs() < 0.01);
        assert_eq!(spacing_ns(10_000_000, 2, 10, true), 100_000_000.0);
        assert_eq!(floor_samples(FLOOR_NS, 0.0), FLOOR_NS as f64);
    }

    #[test]
    fn indicator_marks_exceedances() {
        let a = [1u64, 5, 9];
        assert_eq!(indicator(&[&a], 5), vec![vec![0.0, 0.0, 1.0]]);
    }
}
