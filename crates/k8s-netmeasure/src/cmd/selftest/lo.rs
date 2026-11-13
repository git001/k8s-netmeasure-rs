//! The bounded loopback listener for the SYN checks of `selftest`; the paced checks use
//! [`crate::listener::Listener`]. Never a production listener (design §3.8).

use std::net::{Ipv4Addr, SocketAddrV4};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

/// A listener with an explicit `backlog` that nobody accepts from.
pub fn bounded(backlog: i32) -> std::io::Result<(Socket, SocketAddrV4)> {
    let s = Socket::new(Domain::IPV4, Type::STREAM.cloexec(), Some(Protocol::TCP))?;
    s.bind(&SockAddr::from(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))?;
    s.listen(backlog)?;
    let addr = s
        .local_addr()?
        .as_socket_ipv4()
        .ok_or_else(|| std::io::Error::other("not IPv4"))?;
    Ok((s, addr))
}
