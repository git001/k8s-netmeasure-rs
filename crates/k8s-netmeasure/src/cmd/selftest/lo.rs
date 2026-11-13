//! Loopback listeners for `selftest`: never a production listener (design §3.8).

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

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

/// Accepted connections a [`Draining`] listener keeps open. Closing at once lets the
/// client see the FIN before it reads `TCP_INFO` (`CLOSE_WAIT`, design §3.1); held
/// connections are closed only after this many newer ones, long after the client
/// has reset them.
const HOLD: usize = 64;

/// A listener whose accept thread accepts every connection until stopped and closes
/// it [`HOLD`] connections later.
pub struct Draining {
    pub addr: SocketAddrV4,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Draining {
    pub fn start() -> std::io::Result<Draining> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        let std::net::SocketAddr::V4(addr) = listener.local_addr()? else {
            return Err(std::io::Error::other("not IPv4"));
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("accept".into())
            .spawn(move || {
                let mut held = VecDeque::with_capacity(HOLD);
                for conn in listener.incoming() {
                    if flag.load(Ordering::Relaxed) {
                        break;
                    }
                    if held.len() == HOLD {
                        held.pop_front();
                    }
                    held.push_back(conn);
                }
            })?;
        Ok(Draining {
            addr,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Draining {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(self.addr); // wake the accept loop
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
