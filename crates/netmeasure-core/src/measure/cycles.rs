//! Number of cycles per run (design §3.5): `--cycles` (default 10) or `--cycle
//! <duration>`, then clamped so that one cycle lasts between 2 s and 60 s at the
//! planned pace. The effective count is recorded in the result file.

pub const MIN_CYCLE_NS: u64 = 2_000_000_000;
pub const MAX_CYCLE_NS: u64 = 60_000_000_000;

/// What the operator asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Count(u64),
    /// Fixed cycle length in ns.
    Length(u64),
}

/// The effective cycle count for a run of `run_ns` planned wall time (slots ×
/// interval, without overruns) and `samples` counted samples per target.
pub fn effective(request: Request, run_ns: u64, samples: u64) -> u64 {
    let wanted = match request {
        Request::Count(n) => n,
        Request::Length(len) => run_ns.div_ceil(len.max(1)),
    };
    let fewest = run_ns.div_ceil(MAX_CYCLE_NS).max(1);
    let most = (run_ns / MIN_CYCLE_NS).max(1);
    wanted
        .clamp(fewest, most.max(fewest))
        .clamp(1, samples.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    #[test]
    fn defaults_stay_inside_the_bounds() {
        // pair mode at defaults: 5611 slots × 10 ms ≈ 56 s → 10 cycles of 5.6 s.
        assert_eq!(effective(Request::Count(10), 56 * S, 5000), 10);
    }

    #[test]
    fn short_and_long_runs_are_clamped() {
        assert_eq!(
            effective(Request::Count(10), 5 * S, 500),
            2,
            "≥ 2 s per cycle"
        );
        assert_eq!(effective(Request::Count(10), S, 100), 1, "shorter than 2 s");
        assert_eq!(
            effective(Request::Count(10), 1200 * S, 100_000),
            20,
            "≤ 60 s"
        );
        assert_eq!(effective(Request::Count(50), 60 * S, 20), 20, "≤ samples");
    }

    #[test]
    fn fixed_length() {
        assert_eq!(effective(Request::Length(20 * S), 56 * S, 5000), 3);
        assert_eq!(
            effective(Request::Length(S), 56 * S, 5000),
            28,
            "clamped to 2 s"
        );
    }
}
