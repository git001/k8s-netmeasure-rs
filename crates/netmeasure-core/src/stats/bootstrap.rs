//! Stationary bootstrap (Politis & Romano 1994, design §10.2) within one run's
//! segments: geometric block lengths with mean `L`, uniform block starts, circular
//! wrap **inside** the segment; a block never crosses a segment (cycle) or run
//! boundary. Every replicate has the same segment lengths as the original.

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

/// Fills `out` with one replicate of `segments` (concatenated).
pub fn resample<T: Copy>(
    segments: &[&[T]],
    mean_block: f64,
    rng: &mut ChaCha8Rng,
    out: &mut Vec<T>,
) {
    out.clear();
    append(segments, mean_block, rng, out);
}

/// Appends one replicate of `segments` to `out` (several runs into one pool).
pub fn append<T: Copy>(segments: &[&[T]], mean_block: f64, rng: &mut ChaCha8Rng, out: &mut Vec<T>) {
    let restart = 1.0 / mean_block.max(1.0);
    for seg in segments.iter().filter(|s| !s.is_empty()) {
        let len = seg.len();
        let mut pos = rng.random_range(0..len);
        for i in 0..len {
            if i > 0 && rng.random::<f64>() < restart {
                pos = rng.random_range(0..len);
            }
            out.push(seg[pos]);
            pos = (pos + 1) % len;
        }
    }
}

/// Type-1 quantile of `values` by selection (no full sort); `values` is reordered.
pub fn select_type1(values: &mut [u64], p: f64) -> Option<u64> {
    if values.is_empty() || !(p > 0.0 && p <= 1.0) {
        return None;
    }
    let n = values.len();
    let rank = ((n as f64 * p).ceil() as usize).clamp(1, n) - 1;
    Some(*values.select_nth_unstable(rank).1)
}

/// `b` bootstrap replicates of the type-1 `p`-quantile of one run's segments.
pub fn quantile_replicates(
    segments: &[&[u64]],
    p: f64,
    mean_block: f64,
    b: usize,
    rng: &mut ChaCha8Rng,
) -> Vec<u64> {
    let mut buf = Vec::new();
    (0..b)
        .filter_map(|_| {
            resample(segments, mean_block, rng, &mut buf);
            select_type1(&mut buf, p)
        })
        .collect()
}

/// Percentile interval of replicates at `level` (two-sided), type-1 quantiles of the
/// sorted replicates. Censored values stay at the top, so a bound can be censored.
pub fn percentile_interval(replicates: &mut [u64], level: f64) -> Option<(u64, u64)> {
    let tail = (1.0 - level) / 2.0;
    replicates.sort_unstable();
    let lo = super::quantile::type1(replicates, tail.max(f64::MIN_POSITIVE))?;
    let hi = super::quantile::type1(replicates, 1.0 - tail)?;
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn replicate_keeps_segment_lengths_and_membership() {
        let a: Vec<u64> = (0..100).collect();
        let b: Vec<u64> = (1000..1050).collect();
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let mut out = Vec::new();
        resample(&[&a, &b], 5.0, &mut rng, &mut out);
        assert_eq!(out.len(), 150);
        assert!(out[..100].iter().all(|v| *v < 100));
        assert!(out[100..].iter().all(|v| (1000..1050).contains(v)));
    }

    #[test]
    fn mean_block_length_is_respected() {
        let a: Vec<u64> = (0..100_000).collect();
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let mut out = Vec::new();
        resample(&[&a], 20.0, &mut rng, &mut out);
        let breaks = out
            .windows(2)
            .filter(|w| w[1] != (w[0] + 1) % 100_000)
            .count();
        let mean = out.len() as f64 / (breaks + 1) as f64;
        assert!((17.0..23.0).contains(&mean), "{mean}");
    }

    #[test]
    fn same_seed_same_replicates() {
        let a: Vec<u64> = (0..500).collect();
        let run =
            |seed| quantile_replicates(&[&a], 0.9, 10.0, 50, &mut ChaCha8Rng::seed_from_u64(seed));
        assert_eq!(run(3), run(3));
        assert_ne!(run(3), run(4));
    }

    #[test]
    fn interval_covers_the_sample_quantile() {
        let a: Vec<u64> = (1..=5000).collect();
        let mut reps =
            quantile_replicates(&[&a], 0.99, 1.0, 2000, &mut ChaCha8Rng::seed_from_u64(9));
        let (lo, hi) = percentile_interval(&mut reps, 0.95).unwrap();
        assert!(lo < 4950 && 4950 < hi, "{lo}..{hi}");
        assert!(hi - lo < 60, "{lo}..{hi}");
        let mut v = vec![5, 1, 3];
        assert_eq!(select_type1(&mut v, 0.5), Some(3));
        assert_eq!(select_type1(&mut [], 0.5), None);
    }
}
