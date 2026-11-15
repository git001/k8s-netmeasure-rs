//! The diagnostics part of the result file (design §5): context that can explain a
//! change but never decides one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diag::nicstats::NicCounters;

/// Context that can explain a change but never decides one (§5).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostics {
    pub cycles: Vec<CycleDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleDiagnostics {
    pub cycle_id: String,
    pub source: SourceDiagnostics,
    /// Target index → its control-port counters during the cycle.
    pub targets: BTreeMap<String, TargetDiagnostics>,
}

/// Deltas of a target's control-port counters; `None` if not available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDiagnostics {
    pub accepted: Option<u64>,
    pub aborted: Option<u64>,
    pub netns_listen_drops: Option<u64>,
    pub netns_listen_overflows: Option<u64>,
    pub netns_embryonic_rsts: Option<u64>,
    /// Selected NIC counters that increased (underlay targets; a pod network
    /// namespace sees no physical interface): `{}` read, nothing increased; absent
    /// not read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nic: Option<NicCounters>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDiagnostics {
    /// `/proc/net/netstat` counters that increased during the cycle.
    pub netstat: BTreeMap<String, u64>,
    /// Selected NIC counters (`ethtool -S`) that increased during the cycle, per
    /// physical interface; overlay runs read them from the node listener. `{}` read,
    /// nothing increased; absent not read (e.g. a file of an older version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nic: Option<NicCounters>,
}
