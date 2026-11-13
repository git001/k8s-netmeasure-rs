//! The measured listener (design §4.2), used by `listen`, by `run` as its loopback
//! control target and by `selftest`. A dedicated accept thread waits in `poll` (1 s,
//! so its heartbeat advances while idle), accepts until `EAGAIN` and keeps the last
//! [`HOLD`] connections open: an immediate close sends a FIN that can reach the client
//! before it reads `TCP_INFO` (`CLOSE_WAIT`, §3.1). Nothing is read or written.

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use netmeasure_core::measure::sockinfo::{Field, decode_meminfo_drops};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::sys::poll::{self, PollResult};
use crate::sys::{sockopt, time};

/// Accepted connections kept open; the oldest is closed when a new one arrives.
pub const HOLD: usize = 64;
const POLL_NS: u64 = 1_000_000_000;
/// Pause after an accept error such as `EMFILE`, so a socket that stays readable
/// does not turn the accept thread into a busy loop.
const ERROR_PAUSE: Duration = Duration::from_millis(10);

/// Counters of the accept thread, readable at any time.
#[derive(Debug, Default)]
pub struct Counters {
    pub accepted: AtomicU64,
    /// `ECONNABORTED`: the client reset the connection before it was accepted.
    pub aborted: AtomicU64,
    /// Any other accept or poll error (e.g. `EMFILE`).
    pub errors: AtomicU64,
    /// Monotonic ns of the last loop iteration (liveness, design §7).
    pub heartbeat_ns: AtomicU64,
}

/// A snapshot of [`Counters`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Stats {
    pub accepted: u64,
    pub aborted: u64,
    pub errors: u64,
    pub heartbeat_ns: u64,
}

pub struct Listener {
    pub local: SocketAddrV4,
    /// `min(backlog, somaxconn)`; `backlog` if `somaxconn` is unreadable.
    pub effective_backlog: u32,
    socket: Arc<Socket>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    /// Binds `addr` (`SO_REUSEADDR`, non-blocking) and starts the accept thread.
    pub fn start(addr: SocketAddrV4, backlog: u32) -> io::Result<Listener> {
        let socket = Socket::new(
            Domain::IPV4,
            Type::STREAM.nonblocking().cloexec(),
            Some(Protocol::TCP),
        )?;
        socket.set_reuse_address(true)?;
        socket.bind(&SockAddr::from(addr))?;
        socket.listen(i32::try_from(backlog).unwrap_or(i32::MAX))?;
        let local = socket
            .local_addr()?
            .as_socket_ipv4()
            .ok_or_else(|| io::Error::other("not IPv4"))?;
        let somaxconn = crate::diag::read("/proc/sys/net/core/somaxconn")
            .and_then(|s| s.trim().parse::<u32>().ok());
        let socket = Arc::new(socket);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (s, c, f) = (
            Arc::clone(&socket),
            Arc::clone(&counters),
            Arc::clone(&stop),
        );
        let thread = std::thread::Builder::new()
            .name("accept".into())
            .spawn(move || accept_loop(&s, &f, &c))?;
        Ok(Listener {
            local,
            effective_backlog: somaxconn.map_or(backlog, |m| m.min(backlog)),
            socket,
            counters,
            stop,
            thread: Some(thread),
        })
    }

    pub fn stats(&self) -> Stats {
        let c = &self.counters;
        Stats {
            accepted: c.accepted.load(Ordering::Relaxed),
            aborted: c.aborted.load(Ordering::Relaxed),
            errors: c.errors.load(Ordering::Relaxed),
            heartbeat_ns: c.heartbeat_ns.load(Ordering::Relaxed),
        }
    }

    /// `SK_MEMINFO_DROPS` of the listening socket (design §4.2): SYNs dropped because
    /// the accept queue was full.
    pub fn drops(&self) -> Field<u32> {
        match sockopt::meminfo(self.socket.as_fd()) {
            Ok((buf, len)) => decode_meminfo_drops(sockopt::SK_MEMINFO_DROPS, &buf, len),
            Err(_) => Field::Unknown,
        }
    }

    /// Stops accepting, joins the accept thread and closes everything; returns the
    /// final counters.
    pub fn stop(mut self) -> Stats {
        self.shut_down();
        self.stats()
    }

    fn shut_down(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept thread now instead of after its poll timeout.
        let ip = match *self.local.ip() {
            Ipv4Addr::UNSPECIFIED => Ipv4Addr::LOCALHOST,
            ip => ip,
        };
        let wake = SocketAddrV4::new(ip, self.local.port()).into();
        let _ = std::net::TcpStream::connect_timeout(&wake, Duration::from_millis(100));
        let _ = thread.join();
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn accept_loop(socket: &Socket, stop: &AtomicBool, c: &Counters) {
    let mut held: VecDeque<Socket> = VecDeque::with_capacity(HOLD);
    while !stop.load(Ordering::Relaxed) {
        c.heartbeat_ns
            .store(time::monotonic_ns(), Ordering::Relaxed);
        match poll::wait_readable(socket.as_fd(), POLL_NS) {
            PollResult::Event => {}
            PollResult::TimedOut | PollResult::Interrupted => continue,
            PollResult::Failed(_) => {
                c.errors.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(ERROR_PAUSE);
                continue;
            }
        }
        while !stop.load(Ordering::Relaxed) {
            match socket.accept() {
                Ok((conn, _)) => {
                    c.accepted.fetch_add(1, Ordering::Relaxed);
                    if held.len() == HOLD {
                        held.pop_front();
                    }
                    held.push_back(conn);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.raw_os_error() == Some(libc::ECONNABORTED) => {
                    c.aborted.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    c.errors.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(error = %e, "accept failed");
                    std::thread::sleep(ERROR_PAUSE);
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    fn loopback() -> Listener {
        Listener::start(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0), 128).unwrap()
    }

    fn wait_for(l: &Listener, accepted: u64) {
        for _ in 0..500 {
            if l.stats().accepted >= accepted {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("accepted {} of {accepted}", l.stats().accepted);
    }

    #[test]
    fn accepts_and_keeps_connections_open() {
        let l = loopback();
        let client = TcpStream::connect(l.local).unwrap();
        wait_for(&l, 1);
        // The server side is still open: no FIN, so a read would block, not return 0.
        client.set_nonblocking(true).unwrap();
        let mut buf = [0u8; 1];
        let err = std::io::Read::read(&mut &client, &mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
        let stats = l.stop();
        assert_eq!((stats.accepted, stats.aborted, stats.errors), (1, 0, 0));
    }

    #[test]
    fn closes_the_oldest_beyond_hold() {
        let l = loopback();
        let clients: Vec<_> = (0..=HOLD)
            .map(|_| TcpStream::connect(l.local).unwrap())
            .collect();
        wait_for(&l, HOLD as u64 + 1);
        let mut buf = [0u8; 1];
        // The first connection was closed by the server: EOF.
        clients[0]
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(std::io::Read::read(&mut &clients[0], &mut buf).unwrap(), 0);
    }

    #[test]
    fn stop_is_fast_and_heartbeat_advances() {
        let l = loopback();
        std::thread::sleep(Duration::from_millis(20));
        assert!(l.stats().heartbeat_ns > 0);
        assert!(l.effective_backlog <= 128);
        assert_eq!(l.drops(), Field::Known(0));
        let t = std::time::Instant::now();
        drop(l);
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
    }

    #[test]
    fn port_in_use_is_an_error() {
        let l = loopback();
        let err = Listener::start(l.local, 128)
            .err()
            .expect("second bind fails");
        assert_eq!(err.raw_os_error(), Some(libc::EADDRINUSE));
    }
}
