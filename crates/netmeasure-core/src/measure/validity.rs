//! Cycle validity (design §3.6) from facts the binary collected around the cycle.
//! Reasons invalidate a cycle; annotations only explain it. The listener-drop and
//! NIC-path rules apply to the measuring side and to every target with a control
//! port alike.

pub use super::facts::{CycleFacts, NodeListener, SideCounters, TargetCycle};
use crate::schema::nic::NicPath;
use crate::schema::result::Validity;
use crate::schema::sourced::Sourced;
use crate::stats::quantile::type1;

/// Pacing thresholds of the annotate-only rules.
pub const JITTER_P99_NS: u64 = 1_000_000;
pub const JITTER_MAX_NS: u64 = 10_000_000;
pub const OVERRUN_FRACTION: f64 = 0.05;

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
    listener_rule(b, a, &mut reasons, &mut notes);
    for t in &f.targets {
        match (&t.before, &t.after) {
            (Some(tb), Some(ta)) if t.same_identity => {
                listener_rule(tb, ta, &mut reasons, &mut notes)
            }
            _ => reasons.push("target_set_changed"),
        }
        nic_rule(
            &t.nic_before,
            &t.nic_after,
            t.nic_first.as_ref(),
            &mut reasons,
            &mut notes,
        );
    }
    if f.targets_unchecked {
        notes.push("target_unchecked");
    }
    for state in f.node_listener {
        match state {
            NodeListener::Unavailable => notes.push("node_context_unavailable"),
            NodeListener::Mismatch => notes.push("node_listener_mismatch"),
            NodeListener::NotUsed | NodeListener::Ok => {}
        }
    }
    if f.recorded < f.planned_slots || f.invalid_samples > 0 {
        reasons.push("sample_count_short");
    }
    nic_rule(
        &f.nic_before,
        &f.nic_after,
        f.nic_first.as_ref(),
        &mut reasons,
        &mut notes,
    );
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
    reasons.dedup_unsorted();
    notes.dedup_unsorted();
    Validity {
        valid: reasons.is_empty(),
        reasons: reasons.into_iter().map(str::to_owned).collect(),
        annotations: notes.into_iter().map(str::to_owned).collect(),
    }
}

/// A listener dropped a SYN (design §3.6, revised after the first cluster run).
///
/// Every SYN-related drop of a listener goes through the kernel's `tcp_listendrop()`,
/// which raises both the socket's `SK_MEMINFO_DROPS` and the namespace's
/// `TcpExt.ListenDrops`. `sk_drops` alone also rises for an embryonic reset (the
/// client's RST reaching the request socket before its ACK), which loses no SYN;
/// so the cycle is invalid only if both rose. In a hostNetwork namespace the netns
/// counters cover the whole node: a coincidental drop of another listener can still
/// invalidate, a real drop of ours is never missed.
fn listener_rule(
    b: &SideCounters,
    a: &SideCounters,
    reasons: &mut Vec<&str>,
    notes: &mut Vec<&str>,
) {
    let drops = delta(b.netns_listen_drops, a.netns_listen_drops);
    let overflows = delta(b.netns_listen_overflows, a.netns_listen_overflows);
    let netns = match (drops, overflows) {
        (None, None) => None,
        (d, o) => Some(d.unwrap_or(0) + o.unwrap_or(0)),
    };
    match (delta(b.sk_drops, a.sk_drops), netns) {
        (Some(d), Some(0)) if d > 0 => notes.push("listener_drops_not_syn"),
        (Some(d), _) if d > 0 => reasons.push("listener_drops"),
        (Some(_), Some(n)) if n > 0 => notes.push("netns_listen_drops"),
        (None, Some(n)) if n > 0 => reasons.push("listener_drops_suspected"),
        _ => {}
    }
}

/// Changed between before and after, or against the run's first cycle; never
/// "unchanged" when unobserved.
fn nic_rule(
    before: &Sourced<NicPath>,
    after: &Sourced<NicPath>,
    first: Option<&NicPath>,
    reasons: &mut Vec<&str>,
    notes: &mut Vec<&str>,
) {
    match (&before.value, &after.value) {
        (Some(b), Some(a)) if b != a || first != Some(b) => reasons.push("nic_changed"),
        (Some(_), Some(_)) => {}
        _ => notes.push("nic_path_unknown"),
    }
}

/// Keeps the first occurrence of each entry, in order.
trait DedupUnsorted {
    fn dedup_unsorted(&mut self);
}

impl DedupUnsorted for Vec<&str> {
    fn dedup_unsorted(&mut self) {
        let mut seen = Vec::new();
        self.retain(|s| {
            let new = !seen.contains(s);
            seen.push(*s);
            new
        });
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
            targets: vec![],
            targets_unchecked: false,
            node_listener: [NodeListener::Ok; 2],
        }
    }

    #[test]
    fn node_listener_problems_annotate() {
        let f = CycleFacts {
            node_listener: [NodeListener::Unavailable, NodeListener::Mismatch],
            nic_after: Sourced::unknown(),
            ..facts()
        };
        let v = evaluate(&f);
        assert!(v.valid);
        assert_eq!(
            v.annotations,
            vec![
                "node_context_unavailable",
                "node_listener_mismatch",
                "nic_path_unknown"
            ]
        );
    }

    fn target() -> TargetCycle {
        TargetCycle {
            before: Some(counters(0, 0, 0)),
            after: Some(counters(0, 0, 0)),
            same_identity: true,
            nic_before: Sourced::unknown(),
            nic_after: Sourced::unknown(),
            nic_first: None,
        }
    }

    #[test]
    fn target_rules() {
        let ok = CycleFacts {
            targets: vec![target(), target()],
            ..facts()
        };
        let v = evaluate(&ok);
        assert!(v.valid);
        assert_eq!(
            v.annotations,
            vec!["nic_path_unknown"],
            "listed once for two targets"
        );
        let restarted = CycleFacts {
            targets: vec![TargetCycle {
                same_identity: false,
                ..target()
            }],
            ..facts()
        };
        assert_eq!(evaluate(&restarted).reasons, vec!["target_set_changed"]);
        let gone = CycleFacts {
            targets: vec![TargetCycle {
                after: None,
                ..target()
            }],
            ..facts()
        };
        assert_eq!(evaluate(&gone).reasons, vec!["target_set_changed"]);
        let drops = CycleFacts {
            targets: vec![TargetCycle {
                after: Some(counters(0, 1, 1)),
                ..target()
            }],
            ..facts()
        };
        assert_eq!(evaluate(&drops).reasons, vec!["listener_drops"]);
        let unchecked = CycleFacts {
            targets_unchecked: true,
            ..facts()
        };
        assert_eq!(evaluate(&unchecked).annotations, vec!["target_unchecked"]);
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

    /// Seen on a cluster: the client's RST reached the target's request socket
    /// before its ACK; `sk_drops` and `EmbryonicRsts` rose, `ListenDrops` did not.
    #[test]
    fn socket_drops_without_netns_drops_are_not_syn_drops() {
        let embryonic = CycleFacts {
            after: counters(5, 4, 7),
            ..facts()
        };
        let v = evaluate(&embryonic);
        assert!(v.valid);
        assert_eq!(v.annotations, vec!["listener_drops_not_syn"]);
        let target = CycleFacts {
            targets: vec![TargetCycle {
                after: Some(counters(0, 3, 0)),
                ..target()
            }],
            ..facts()
        };
        assert!(evaluate(&target).valid);
        let mut unreadable = embryonic.clone();
        unreadable.before.netns_listen_drops = None;
        unreadable.before.netns_listen_overflows = None;
        assert_eq!(evaluate(&unreadable).reasons, vec!["listener_drops"]);
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
