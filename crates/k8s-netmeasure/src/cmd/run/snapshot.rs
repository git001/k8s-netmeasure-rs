//! The measuring side's state around every cycle (design §3.5, §3.6, §3.7), read on
//! the main thread outside any timed section: own and ancestor cgroup throttling, the
//! loopback control listener's SYN drops, netns counters and the NIC path.

use std::collections::BTreeMap;

use netmeasure_core::diag::cgroup::{ancestor_throttling, floor_millicores};
use netmeasure_core::measure::validity::SideCounters;
use netmeasure_core::schema::control::Snapshot;
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::result::CgroupCheck;
use netmeasure_core::schema::sourced::Sourced;

use super::remote::{self, Remote};
use crate::control::client::Client;
use crate::diag::{cgroup, context, net};
use crate::listener::Handle;

pub struct Snap {
    pub counters: SideCounters,
    pub netstat: BTreeMap<String, u64>,
    pub nic: Sourced<NicPath>,
    /// One per target with a control port; `None` where it did not answer.
    pub targets: Vec<Option<Snapshot>>,
}

pub fn take(listener: &Handle, client: &mut Client, remotes: &[Remote]) -> Snap {
    let cg = cgroup::read();
    let throttled = |l: &cgroup::Level| l.cpu_stat.map(|s| s.throttled_usec);
    let limited: Vec<u64> = cg
        .visible_ancestors
        .iter()
        .filter(|l| l.cpu_max.is_some_and(|m| m.quota_us.is_some()))
        .filter_map(throttled)
        .collect();
    let netstat = net::netstat();
    Snap {
        counters: SideCounters {
            throttled_usec: cg.own.as_ref().and_then(throttled),
            ancestors_throttled_usec: (!limited.is_empty()).then(|| limited.iter().sum()),
            sk_drops: listener.drops().known(),
            netns_listen_drops: netstat.get("TcpExt.ListenDrops").copied(),
            netns_listen_overflows: netstat.get("TcpExt.ListenOverflows").copied(),
        },
        netstat,
        nic: context::nic_path(),
        targets: remote::snapshots(client, remotes),
    }
}

/// The CPU checks at start (§3.7, `load_threads` 0 in Phase 1 until milestone 6).
/// `Err` means refuse to start (exit 3).
pub fn cgroup_check(no_cgroup_check: bool) -> Result<CgroupCheck, String> {
    let cg = cgroup::read();
    if !cg.readable && !no_cgroup_check {
        return Err("cgroup v2 cpu.max/cpu.stat unreadable; use --no-cgroup-check to measure without the throttling check".into());
    }
    let cpu_max = cg.own.as_ref().and_then(|l| l.cpu_max);
    let quota = cpu_max.and_then(|m| m.millicores());
    let floor = floor_millicores(0);
    if let Some(q) = quota.filter(|&q| q < floor) {
        return Err(format!(
            "CPU quota {q}m is below the floor of {floor}m (design §3.7)"
        ));
    }
    let (state, note) = ancestor_throttling(cg.visible_ancestors.len(), quota.is_some());
    Ok(CgroupCheck {
        path: cg.path,
        quota_millicores: quota,
        floor_millicores: floor,
        visible_ancestors: cg.visible_ancestors.into_iter().map(|l| l.path).collect(),
        ancestor_cpu_throttling: state.into(),
        note: note.map(str::to_owned),
    })
}
