//! Standard normal quantile (Acklam's rational approximation, relative error below
//! 1.2e-9) and the sample-size cost of a larger verdict family (design §10.7).

/// `z` with `Φ(z) = p`, for `0 < p < 1`; NaN outside.
pub fn quantile(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const LOW: f64 = 0.024_25;
    if !(p > 0.0 && p < 1.0) {
        return f64::NAN;
    }
    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    if p < LOW {
        tail((-2.0 * p.ln()).sqrt())
    } else if p > 1.0 - LOW {
        -tail((-2.0 * (1.0 - p).ln()).sqrt())
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    }
}

/// How many times the samples of `k = 1` the same TOST power needs at family size
/// `k` (each one-sided test at `0.05/k`): `(z(1 − 0.05/k) / z(0.95))²`.
pub fn sample_factor(k: u32) -> f64 {
    let z = |alpha: f64| quantile(1.0 - alpha);
    (z(0.05 / f64::from(k.max(1))) / z(0.05)).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_quantiles() {
        for (p, z) in [
            (0.5, 0.0),
            (0.95, 1.644_853_627),
            (0.975, 1.959_963_985),
            (0.9875, 2.241_402_728),
            (0.001, -3.090_232_306),
            (0.999_9, 3.719_016_485),
        ] {
            assert!((quantile(p) - z).abs() < 1e-7, "p {p}: {}", quantile(p));
        }
        assert!(quantile(0.0).is_nan() && quantile(1.0).is_nan());
    }

    #[test]
    fn four_strata_need_about_1_85_times_the_samples() {
        assert!((sample_factor(1) - 1.0).abs() < 1e-12);
        assert!(
            (sample_factor(4) - 1.857).abs() < 0.01,
            "{}",
            sample_factor(4)
        );
        assert!(sample_factor(8) > sample_factor(4));
    }
}
