//! Value parsers shared by several subcommands. Durations are not parsed here:
//! every duration flag uses `netmeasure_util::time::parse_duration` directly.

use std::net::{SocketAddr, SocketAddrV4};

/// An IPv4 `address:port`. The measurement uses `AF_INET` sockets (design §3.1), so
/// IPv6 is rejected with a clear message rather than silently failing later.
pub fn ipv4_target(text: &str) -> Result<SocketAddrV4, String> {
    match text.trim().parse::<SocketAddr>() {
        Ok(SocketAddr::V4(v4)) if v4.port() != 0 => Ok(v4),
        Ok(SocketAddr::V4(_)) => Err(format!("`{text}`: port 0 is not a valid target")),
        Ok(SocketAddr::V6(_)) => Err(format!("`{text}`: IPv6 is not supported in Phase 1")),
        Err(_) => Err(format!(
            "`{text}` is not an IPv4 address:port, e.g. 10.0.0.5:17700"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(ipv4_target("10.0.0.5:17700").unwrap().port(), 17700);
        assert!(ipv4_target("[::1]:80").unwrap_err().contains("IPv6"));
        assert!(ipv4_target("10.0.0.5:0").unwrap_err().contains("port 0"));
        assert!(
            ipv4_target("node-a:80")
                .unwrap_err()
                .contains("IPv4 address:port")
        );
    }
}
