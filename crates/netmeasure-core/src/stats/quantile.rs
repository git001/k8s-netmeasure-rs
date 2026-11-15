//! Quantile estimator (design §10.1): inverse ECDF, Hyndman–Fan type 1,
//! `x_(⌈np⌉)`, no interpolation. Censoring is handled in [`super::describe`].

/// Type-1 quantile of **sorted** data; `None` for empty input or `p` outside `(0, 1]`.
pub fn type1(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() || !(p > 0.0 && p <= 1.0) {
        return None;
    }
    let n = sorted.len();
    let rank = ((n as f64) * p).ceil() as usize; // 1-based
    sorted.get(rank.clamp(1, n) - 1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_statistic_without_interpolation() {
        let x: Vec<u64> = (1..=100).collect();
        assert_eq!(type1(&x, 0.5), Some(50));
        assert_eq!(type1(&x, 0.99), Some(99));
        assert_eq!(type1(&x, 0.991), Some(100));
        assert_eq!(type1(&x, 1.0), Some(100));
        assert_eq!(type1(&x, 0.001), Some(1));
        assert_eq!(type1(&[7], 0.5), Some(7));
    }

    #[test]
    fn invalid_input() {
        assert_eq!(type1(&[], 0.5), None);
        assert_eq!(type1(&[1], 0.0), None);
        assert_eq!(type1(&[1], 1.5), None);
        assert_eq!(type1(&[1], f64::NAN), None);
    }
}
