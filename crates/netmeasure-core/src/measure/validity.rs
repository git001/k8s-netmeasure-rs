//! Cycle validity (design §3.6) from facts the binary collected around the cycle.
//! Reasons invalidate a cycle; annotations only explain it. Target-side rules
//! (`target_set_changed`, target listener drops) follow in milestone 3d.

use crate::schema::nic::NicPath;
use crate::schema::result::Validity;
use crate::schema::sourced::Sourced;
use crate::stats::quantile::type1;

/// Pacing thresholds of the annotate-only rules.
pub const JITTER_P99_NS: u64 = 1_000_000;
pub const JITTER_MAX_NS: u64 = 10_000_000;
pub const OVERRUN_FRACTION: f64 = 0.05;

/// Counters of the measuring side, read before and after a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SideCounters {
    /// Own cgroup `cpu.stat` `throttled_usec`.
    pub throttled_usec: Option<u64>,
    /// Sum of `throttled_usec` over the visible ancestors with a quota (§3.7).
    pub ancestors_throttled_usec: Option<u64>,
    /// `SK_MEMINFO_DROPS` of the own loopback control listener.
    pub sk_drops: Option<u32>,
    pub netns_listen_drops: Option<u64>,
    pub netns_listen_overflows: Option<u64>,
}

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
    pub before: SideCounters,
    pub after: SideCounters,
    /// `--no-cgroup-check` was given: throttling is not checked (§3.7).
    pub cgroup_unchecked: bool,
    pub nic_before: Sourced<NicPath>,
    pub nic_after: Sourced<NicPath>,
    /// The NIC path of the run's first cycle.
    pub nic_first: Option<NicPath>,
}

/// `after − before`; `None` if either is unknown or the counter went backwards.
pub fn delta<T: Into<u64> + Copy>(before: Option<T>, after: Option<T>) -> Option<u64> {
    after?.into().checked_sub(before?.into())
}

pub fn evaluate(f: &CycleFacts) -> Validity {
    let mut reasons = Vec::new();
    let mut notes = Vec::new();
    let (b, a) = (&f.before, &f.after);
    if !f.cgroup_unchecked
        && (delta(b.throttled_usec, a.throttled_usec) > Some(0)
            || delta(b.ancestors_throttled_usec, a.ancestors_throttled_usec) > Some(0))
    {
        reasons.push("cpu_throttled");
    }
    if f.cgroup_unchecked {
        notes.push("cpu_throttle_unchecked");
    }
    let netns = delta(b.netns_listen_drops, a.netns_listen_drops).unwrap_or(0)
        + delta(b.netns_listen_overflows, a.netns_listen_overflows).unwrap_or(0);
    match delta(b.sk_drops, a.sk_drops) {
        Some(d) if d > 0 => reasons.push("listener_drops"),
        Some(_) if netns > 0 => notes.push("netns_listen_drops"),
        Some(_) => {}
        None if netns > 0 => reasons.push("listener_drops_suspected"),
        None => {}
    }
    if f.recorded < f.planned_slots || f.invalid_samples > 0 {
        reasons.push("sample_count_short");
    }
    match (&f.nic_before.value, &f.nic_after.value) {
        (Some(before), Some(after)) if before != after || f.nic_first.as_ref() != Some(before) => {
            reasons.push("nic_changed");
        }
        (Some(_), Some(_)) => {}
        _ => notes.push("nic_path_unknown"),
    }
    if f.interrupted {
        reasons.push("interrupted");
    }
    let p99 = type1(&f.lateness_sorted, 0.99).unwrap_or(0);
    let max = f.lateness_sorted.last().copied().unwrap_or(0);
    if p99 > JITTER_P99_NS || max > JITTER_MAX_NS {
        notes.push("scheduling_jitter");
    }
    if f.recorded > 0 && f.overruns as f64 / f.recorded as f64 > OVERRUN_FRACTION {
        notes.push("pacing_overrun");
    }
    Validity {
        valid: reasons.is_empty(),
        reasons: reasons.into_iter().map(str::to_owned).collect(),
        annotations: notes.into_iter().map(str::to_owned).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::nic::Interface;

    fn nic(driver: &str) -> NicPath {
        NicPath {
            default_route_interface: Some("eth0".into()),
            interfaces: vec![Interface {
                name: "eth0".into(),
                driver: Some(driver.into()),
                rings: None,
                rings_errno: None,
            }],
        }
    }

    fn counters(throttled: u64, drops: u32, netns: u64) -> SideCounters {
        SideCounters {
            throttled_usec: Some(throttled),
            ancestors_throttled_usec: None,
            sk_drops: Some(drops),
            netns_listen_drops: Some(netns),
            netns_listen_overflows: Some(netns),
        }
    }

    fn facts() -> CycleFacts {
        CycleFacts {
            planned_slots: 100,
            recorded: 100,
            invalid_samples: 0,
            overruns: 0,
            lateness_sorted: vec![10_000; 100],
            interrupted: false,
            before: counters(5, 1, 7),
            after: counters(5, 1, 7),
            cgroup_unchecked: false,
            nic_before: Sourced::observed(Some(nic("a"))),
            nic_after: Sourced::observed(Some(nic("a"))),
            nic_first: Some(nic("a")),
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
            vec!["sample_count_short", "interrupted"]
        );
    }

    #[test]
    fn cpu_rules() {
        let throttled = CycleFacts {
            after: counters(6, 1, 7),
            ..facts()
        };
        assert_eq!(evaluate(&throttled).reasons, vec!["cpu_throttled"]);
        let mut anc = facts();
        anc.before.ancestors_throttled_usec = Some(0);
        anc.after.ancestors_throttled_usec = Some(3);
        assert_eq!(evaluate(&anc).reasons, vec!["cpu_throttled"]);
        let unchecked = CycleFacts {
            cgroup_unchecked: true,
            ..throttled
        };
        let v = evaluate(&unchecked);
        assert!(v.valid);
        assert_eq!(v.annotations, vec!["cpu_throttle_unchecked"]);
    }

    #[test]
    fn listener_drop_rules() {
        let own = CycleFacts {
            after: counters(5, 2, 8),
            ..facts()
        };
        assert_eq!(evaluate(&own).reasons, vec!["listener_drops"]);
        let other = CycleFacts {
            after: counters(5, 1, 8),
            ..facts()
        };
        assert_eq!(evaluate(&other).annotations, vec!["netns_listen_drops"]);
        let mut suspected = other.clone();
        suspected.before.sk_drops = None;
        assert_eq!(
            evaluate(&suspected).reasons,
            vec!["listener_drops_suspected"]
        );
    }

    #[test]
    fn nic_rules() {
        let changed = CycleFacts {
            nic_after: Sourced::observed(Some(nic("b"))),
            ..facts()
        };
        assert_eq!(evaluate(&changed).reasons, vec!["nic_changed"]);
        let drifted = CycleFacts {
            nic_first: Some(nic("b")),
            ..facts()
        };
        assert_eq!(
            evaluate(&drifted).reasons,
            vec!["nic_changed"],
            "differs from the first cycle"
        );
        let unknown = CycleFacts {
            nic_before: Sourced::unknown(),
            ..facts()
        };
        assert_eq!(evaluate(&unknown).annotations, vec!["nic_path_unknown"]);
        assert!(evaluate(&unknown).valid);
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

    #[test]
    fn delta_needs_both_and_no_wrap() {
        assert_eq!(delta(Some(3u64), Some(5)), Some(2));
        assert_eq!(delta(Some(5u64), Some(3)), None);
        assert_eq!(delta::<u64>(None, Some(3)), None);
    }
}
