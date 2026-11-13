//! IPv4 address of an interface (`getifaddrs` through the safe `nix` API), for
//! `--control-bind auto` (design §4.1).

use std::net::Ipv4Addr;

use nix::ifaddrs::getifaddrs;

/// The first IPv4 address of `ifname`; `None` if it has none or the list is unreadable.
pub fn ipv4_of(ifname: &str) -> Option<Ipv4Addr> {
    getifaddrs()
        .ok()?
        .filter(|a| a.interface_name == ifname)
        .find_map(|a| a.address?.as_sockaddr_in().map(|s| s.ip()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_has_127_0_0_1_and_unknown_has_none() {
        assert_eq!(ipv4_of("lo"), Some(Ipv4Addr::LOCALHOST));
        assert_eq!(ipv4_of("nm-no-such-if0"), None);
    }
}
