//! Control-port bodies (design §7): `/v1/info`, `/v1/context` and `/v1/snapshot`.
//! `listen` writes them; `run` reads them with `deny_unknown_fields` (§9.1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::nic::NicPath;
use super::sourced::Sourced;
use crate::diag::cgroup::{CpuMax, CpuStat};
use crate::diag::nicstats::NicCounters;

pub const SCHEMA: &str = "k8s-netmeasure/control";
pub const SCHEMA_VERSION: u32 = 1;

/// Build of the answering process (same fields as in result files, §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildInfo {
    pub version: String,
    pub git_sha: String,
    pub rustc: String,
    pub target: String,
    #[serde(rename = "static")]
    pub is_static: bool,
}

/// `GET /v1/info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub schema: String,
    pub schema_version: u32,
    pub build_info: BuildInfo,
    /// ULID, new on every start: a different value means the target restarted.
    pub listener_instance: String,
    pub measured: String,
    pub backlog: u32,
    pub effective_backlog: u32,
}

/// `GET /v1/context`: the node context (§5), every field with its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeContext {
    pub node_name: Sourced<String>,
    pub zone: Sourced<String>,
    pub instance_type: Sourced<String>,
    pub kernel_release: Sourced<String>,
    pub kernel_version: Sourced<String>,
    pub boot_id: Sourced<String>,
    pub uptime_s: Sourced<u64>,
    /// Own cgroup's `cpu.max`.
    pub cpu_max: Sourced<CpuMax>,
    pub cpu_model: Sourced<String>,
    pub cpu_microcode: Sourced<String>,
    pub somaxconn: Sourced<u32>,
    pub tcp_syn_retries: Sourced<u32>,
    /// `unknown` on kernels without the sysctl.
    pub tcp_syn_linear_timeouts: Sourced<u32>,
    /// MTU of the default-route interface (the overlay MTU in a pod).
    pub mtu: Sourced<u32>,
    /// `unknown` where no physical interface is visible (pod network namespace).
    pub nic: Sourced<NicPath>,
}

/// `GET /v1/snapshot`: counters read before and after every cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// `CLOCK_MONOTONIC` of the answering node, ns.
    pub monotonic_ns: u64,
    pub boot_id: Option<String>,
    pub listener_instance: String,
    pub listener: ListenerCounters,
    /// `TcpExt.ListenOverflows` / `TcpExt.ListenDrops` of the network namespace.
    pub netns_listen_overflows: Option<u64>,
    pub netns_listen_drops: Option<u64>,
    /// `TcpExt.EmbryonicRsts`: resets that hit a request socket; they raise
    /// `sk_drops` without a lost SYN (explains `listener_drops_not_syn`).
    pub netns_embryonic_rsts: Option<u64>,
    /// Own cgroup's `cpu.stat`.
    pub cpu_stat: Option<CpuStat>,
    pub nic: Sourced<NicPath>,
    /// Selected `ethtool -S` counters of the physical interfaces this process sees
    /// (`diag::nicstats`); empty in a pod network namespace.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nic_stats: NicCounters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListenerCounters {
    pub accepted: u64,
    pub aborted: u64,
    pub errors: u64,
    /// `SK_MEMINFO_DROPS` of the measured listener; `None` if unreadable.
    pub sk_drops: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::sourced::Source;

    #[test]
    fn snapshot_round_trip_and_strictness() {
        let s = Snapshot {
            monotonic_ns: 1,
            boot_id: Some("b".into()),
            listener_instance: "01J".into(),
            listener: ListenerCounters {
                accepted: 2,
                aborted: 0,
                errors: 0,
                sk_drops: Some(0),
            },
            netns_listen_overflows: Some(0),
            netns_listen_drops: None,
            netns_embryonic_rsts: Some(0),
            cpu_stat: None,
            nic: Sourced::unknown(),
            nic_stats: NicCounters::new(),
        };
        let mut v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["nic"]["source"], "unknown");
        // Empty NIC statistics are left out, and a snapshot without them still reads.
        assert!(v.get("nic_stats").is_none());
        assert_eq!(serde_json::from_value::<Snapshot>(v.clone()).unwrap(), s);
        let mut with = s.clone();
        with.nic_stats
            .insert("vf0".into(), [("stop_queue".into(), 3)].into());
        let w = serde_json::to_value(&with).unwrap();
        assert_eq!(w["nic_stats"]["vf0"]["stop_queue"], 3);
        assert_eq!(serde_json::from_value::<Snapshot>(w).unwrap(), with);
        v["surprise"] = 1.into();
        assert!(serde_json::from_value::<Snapshot>(v).is_err());
    }

    #[test]
    fn build_info_uses_the_result_file_field_name() {
        let b = BuildInfo {
            version: "0.1.0".into(),
            git_sha: "abc".into(),
            rustc: "1.100.0".into(),
            target: "t".into(),
            is_static: true,
        };
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["static"], true);
        assert_eq!(Sourced::observed(Some(b)).source, Source::Observed);
    }
}
