//! The interface with the IPv4 default route and its address (design §3.6, §4.1), and
//! the netns TCP counters.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netmeasure_core::diag::{netstat, route};

use crate::sys::ifaddr;

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
