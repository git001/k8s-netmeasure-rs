//! What the binary collects around one cycle for the validity decision (design §3.6):
//! counters of both sides, NIC paths, target reachability and identity.

use crate::schema::nic::NicPath;
use crate::schema::sourced::Sourced;

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
    /// Targets with a control port.
    pub targets: Vec<TargetCycle>,
    /// `--no-target-control`: the target side is not checked.
    pub targets_unchecked: bool,
    /// The node listener at the before- and after-snapshot.
    pub node_listener: [NodeListener; 2],
}

/// The node listener (`--node-context-from`, §5) around one snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeListener {
    /// Not configured: the NIC path is observed directly or not at all.
    #[default]
    NotUsed,
    Ok,
    /// It did not answer; the NIC path of this snapshot is unknown.
    Unavailable,
    /// It now reports another boot id; it is no longer asked.
    Mismatch,
}

/// One target's control-port view of a cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetCycle {
    /// Counters before and after; `None` if the control port did not answer.
    pub before: Option<SideCounters>,
    pub after: Option<SideCounters>,
    /// `listener_instance` and `boot_id` still those seen at the start of the run.
    pub same_identity: bool,
    pub nic_before: Sourced<NicPath>,
    pub nic_after: Sourced<NicPath>,
    pub nic_first: Option<NicPath>,
}
