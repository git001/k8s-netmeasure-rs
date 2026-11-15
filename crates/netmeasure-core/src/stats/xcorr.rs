//! Cross-group correlation check of the concurrent design (design §10.3): resampling
//! the two groups independently drops `−2·Cov(A, B)`, which is conservative only if
//! the covariance is ≥ 0. This computes the correlation of the two groups' exceedance
//! indicators in aligned wall-clock windows of one block length. Aligning runs in
//! wall-clock time is the report's job (milestone 5); this is the pure computation.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correlation {
    /// Pearson correlation of the per-window exceedance fractions.
    pub r: f64,
    pub windows: usize,
    /// One-sided Fisher-z test at 5 %: `atanh(r)·√(windows − 3) < −1.645`.
    pub significantly_negative: bool,
}

/// `a` and `b`: `(wall-clock ns, exceeded)` per sample. Windows present in both are
/// paired; `None` with fewer than 4 such windows or without variance.
pub fn window_correlation(
    a: &[(i128, bool)],
    b: &[(i128, bool)],
    window_ns: i128,
) -> Option<Correlation> {
    let fractions = |s: &[(i128, bool)]| {
        let mut m = std::collections::BTreeMap::<i128, (f64, f64)>::new();
        for &(t, x) in s {
            let e = m.entry(t.div_euclid(window_ns.max(1))).or_default();
            *e = (e.0 + f64::from(u8::from(x)), e.1 + 1.0);
        }
        m
    };
    let (fa, fb) = (fractions(a), fractions(b));
    let pairs: Vec<(f64, f64)> = fa
        .iter()
        .filter_map(|(w, (xa, na))| fb.get(w).map(|(xb, nb)| (xa / na, xb / nb)))
        .collect();
    let n = pairs.len();
    if n < 4 {
        return None;
    }
    let mean = |f: fn(&(f64, f64)) -> f64| pairs.iter().map(f).sum::<f64>() / n as f64;
    let (ma, mb) = (mean(|p| p.0), mean(|p| p.1));
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in &pairs {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    if saa == 0.0 || sbb == 0.0 {
        return None;
    }
    let r = (sab / (saa * sbb).sqrt()).clamp(-0.999_999, 0.999_999);
    Some(Correlation {
        r,
        windows: n,
        significantly_negative: r.atanh() * ((n - 3) as f64).sqrt() < -1.645,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample per ns; window `w` exceeds when `pattern(w)`.
    fn group(windows: i128, per: i128, pattern: impl Fn(i128) -> bool) -> Vec<(i128, bool)> {
        (0..windows * per)
            .map(|t| (t, pattern(t / per) && t % per == 0))
            .collect()
    }

    #[test]
    fn opposite_groups_are_significantly_negative() {
        let a = group(40, 10, |w| w % 2 == 0);
        let b = group(40, 10, |w| w % 2 == 1);
        let c = window_correlation(&a, &b, 10).unwrap();
        assert!(c.r < -0.99 && c.significantly_negative, "{c:?}");
        let same = window_correlation(&a, &a, 10).unwrap();
        assert!(same.r > 0.99 && !same.significantly_negative);
    }

    #[test]
    fn too_few_windows_or_no_variance() {
        let a = group(3, 10, |w| w == 0);
        assert_eq!(window_correlation(&a, &a, 10), None);
        let flat = group(10, 10, |_| false);
        assert_eq!(window_correlation(&flat, &flat, 10), None);
    }
}
