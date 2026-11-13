//! The interface with the IPv4 default route and its address (design §3.6, §4.1).

use std::net::Ipv4Addr;

use netmeasure_core::diag::route;

use crate::sys::ifaddr;

pub fn default_route_interface() -> Option<String> {
    super::read("/proc/net/route").and_then(|t| route::default_route_interface(&t))
}

/// The IPv4 address of the default-route interface: the pod IP in a pod, the node's
/// primary address with hostNetwork.
pub fn default_route_ipv4() -> Option<Ipv4Addr> {
    ifaddr::ipv4_of(&default_route_interface()?)
}
