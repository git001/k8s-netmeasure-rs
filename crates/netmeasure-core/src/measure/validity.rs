//! Cycle validity (design §3.6) from facts the binary collected. Reasons invalidate a
//! cycle; annotations only explain it. Snapshot-based rules (CPU, listener drops,
//! NIC path) follow in milestone 3c2.

use crate::schema::result::Validity;
use crate::stats::quantile::type1;

/// Pacing thresholds of the annotate-only rules.
pub const JITTER_P99_NS: u64 = 1_000_000;
pub const JITTER_MAX_NS: u64 = 10_000_000;
pub const OVERRUN_FRACTION: f64 = 0.05;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleFacts {
    pub planned_slots: u64,
    pub recorded: u64,
    /// `local_error` + `anomaly` samples.
    pub invalid_samples: u64,
    pub overruns: u64,
    /// Wake-up lateness of every recorded slot, sorted.
    pub lateness_sorted: Vec<u64>,
    /// Shutdown ended the cycle early.
    pub interrupted: bool,
}

pub fn evaluate(f: &CycleFacts) -> Validity {
    let mut reasons = Vec::new();
    let mut annotations = Vec::new();
    if f.interrupted {
        reasons.push("interrupted".to_owned());
    }
    if f.invalid_samples > 0 || f.recorded < f.planned_slots {
        reasons.push("sample_count_short".to_owned());
    }
    let p99 = type1(&f.lateness_sorted, 0.99).unwrap_or(0);
    let max = f.lateness_sorted.last().copied().unwrap_or(0);
    if p99 > JITTER_P99_NS || max > JITTER_MAX_NS {
        annotations.push("scheduling_jitter".to_owned());
    }
    if f.recorded > 0 && f.overruns as f64 / f.recorded as f64 > OVERRUN_FRACTION {
        annotations.push("pacing_overrun".to_owned());
    }
    Validity {
        valid: reasons.is_empty(),
        reasons,
        annotations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> CycleFacts {
        CycleFacts {
            planned_slots: 100,
            recorded: 100,
            invalid_samples: 0,
            overruns: 0,
            lateness_sorted: vec![10_000; 100],
            interrupted: false,
        }
    }

    #[test]
    fn clean_cycle_is_valid() {
        assert_eq!(
            evaluate(&facts()),
            Validity {
                valid: true,
                reasons: vec![],
                annotations: vec![]
            }
        );
    }

    #[test]
    fn short_or_interrupted_is_invalid() {
        let short = CycleFacts {
            invalid_samples: 1,
            ..facts()
        };
        assert_eq!(evaluate(&short).reasons, vec!["sample_count_short"]);
        let cut = CycleFacts {
            recorded: 40,
            interrupted: true,
            ..facts()
        };
        assert_eq!(
            evaluate(&cut).reasons,
            vec!["interrupted", "sample_count_short"]
        );
    }

    #[test]
    fn jitter_and_overrun_only_annotate() {
        let mut late = vec![10_000; 99];
        late.push(11_000_000);
        let f = CycleFacts {
            lateness_sorted: late,
            overruns: 6,
            ..facts()
        };
        let v = evaluate(&f);
        assert!(v.valid);
        assert_eq!(v.annotations, vec!["scheduling_jitter", "pacing_overrun"]);
    }
}
