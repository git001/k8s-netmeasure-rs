//! The interface with the IPv4 default route and its address (design §3.6, §4.1), the
//! netns TCP counters and the selected NIC statistics of the physical interfaces.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netmeasure_core::diag::nicstats::{self, NicCounters};
use netmeasure_core::diag::{netstat, route};

use crate::sys::{ethtool_stats, ifaddr};

pub fn default_route_interface() -> Option<String> {
    super::read("/proc/net/route").and_then(|t| route::default_route_interface(&t))
}

/// The IPv4 address of the default-route interface: the pod IP in a pod, the node's
/// primary address with hostNetwork.
pub fn default_route_ipv4() -> Option<Ipv4Addr> {
    ifaddr::ipv4_of(&default_route_interface()?)
}

/// `TcpExt.*` and friends of the own network namespace (`/proc/net/netstat`).
pub fn netstat() -> BTreeMap<String, u64> {
    super::read("/proc/net/netstat")
        .map(|t| netstat::parse(&t))
        .unwrap_or_default()
}

/// Selected `ethtool -S` counters of every physical interface this process sees
/// (anything with `/sys/class/net/<if>/device`); an interface whose statistics cannot
/// be read is left out. Empty in a pod network namespace.
pub fn nic_stats() -> NicCounters {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return NicCounters::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().join("device").exists())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stats = ethtool_stats::stats(&name).ok()?;
            Some((name, nicstats::select(stats)))
        })
        .collect()
}
