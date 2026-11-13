//! One connect attempt, exactly the sequence of design §3.1. Socket creation and
//! `SO_LINGER` happen before `t0`; `SO_ERROR`, `TCP_INFO` and `close` after `t1`.

use std::net::SocketAddrV4;
use std::os::fd::AsFd;

use netmeasure_core::measure::outcome::{self, Completion, Facts, Outcome};
use netmeasure_core::measure::sockinfo::{self, TcpInfo};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::sys::poll::{self, PollResult};
use crate::sys::{sockopt, time};

/// Everything recorded for one attempt.
///
/// The timed section runs from `t0` (right before `connect()`) to `t1` (right after
/// the wait ends). There is deliberately no end field: `t1 = start_ns + duration_ns`,
/// and a second field could only disagree. The pacer's own clock reads around
/// [`attempt`] are different instants on purpose: wake-up before the socket is
/// created, and slot end after `SO_ERROR`, `TCP_INFO` and `close` (overrun check).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    /// `t0` (monotonic ns), taken right before `connect()`.
    pub start_ns: u64,
    /// `t1 − t0`, with `t1` (monotonic ns) taken right after the wait ends.
    pub duration_ns: u64,
    pub outcome: Outcome,
    pub errno: Option<i32>,
    /// `connect()` returned 0: handshake completed inside the call.
    pub sync_connect: bool,
    /// `EINTR` seen while waiting (expected 0 with signals blocked).
    pub eintr: u32,
    pub tcp_info: TcpInfo,
}

/// Runs one attempt against `addr` with a per-attempt `timeout_ns`.
pub fn attempt(addr: SocketAddrV4, timeout_ns: u64) -> Attempt {
    let socket = match prepare() {
        Ok(s) => s,
        Err(e) => return failed_before_start(e),
    };
    let target = SockAddr::from(addr);
    let fd = socket.as_fd();
    let mut eintr = 0;

    // ---- timed section ----
    let t0 = time::monotonic_ns();
    let deadline = t0.saturating_add(timeout_ns);
    let completion = match socket.connect(&target) {
        Ok(()) => Completion::Immediate,
        Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => loop {
            let remaining = deadline.saturating_sub(time::monotonic_ns());
            match poll::wait_writable(fd, remaining) {
                PollResult::Event => break Completion::Event,
                PollResult::TimedOut => break Completion::TimedOut,
                PollResult::Interrupted => eintr += 1,
                PollResult::Failed(errno) => break Completion::LocalFailure(errno),
            }
        },
        Err(e) => Completion::ConnectFailed(e.raw_os_error().unwrap_or(libc::EIO)),
    };
    let t1 = time::monotonic_ns();
    // ---- end of timed section ----

    let so_error = match socket.take_error() {
        Ok(None) => Some(0),
        Ok(Some(e)) => e.raw_os_error(),
        Err(_) => None,
    };
    let tcp_info = match sockopt::tcp_info(fd) {
        Ok((buf, len)) => sockinfo::decode_tcp_info(sockopt::TCP_INFO_LAYOUT, &buf, len),
        Err(_) => TcpInfo::UNAVAILABLE,
    };
    drop(socket); // SO_LINGER 0: RST, no TIME_WAIT

    let facts = Facts {
        completion,
        so_error,
        tcp_state: tcp_info.state,
    };
    Attempt {
        start_ns: t0,
        duration_ns: t1 - t0,
        outcome: outcome::classify(&facts),
        errno: outcome::errno(&facts),
        sync_connect: completion == Completion::Immediate,
        eintr,
        tcp_info,
    }
}

/// Non-blocking, close-on-exec TCP socket with `SO_LINGER {1, 0}`: one `socket()`
/// and one `setsockopt()`, both outside the timed section.
fn prepare() -> std::io::Result<Socket> {
    let socket = Socket::new(
        Domain::IPV4,
        Type::STREAM.nonblocking().cloexec(),
        Some(Protocol::TCP),
    )?;
    socket.set_linger(Some(std::time::Duration::ZERO))?;
    Ok(socket)
}

fn failed_before_start(e: std::io::Error) -> Attempt {
    let errno = e.raw_os_error().unwrap_or(libc::EIO);
    Attempt {
        start_ns: time::monotonic_ns(),
        duration_ns: 0,
        outcome: Outcome::LocalError,
        errno: Some(errno),
        sync_connect: false,
        eintr: 0,
        tcp_info: TcpInfo::UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener};
    use std::os::unix::thread::JoinHandleExt;
    use std::time::Duration;

    use nix::sys::pthread::{Pthread, pthread_kill};
    use nix::sys::signal::Signal;

    use socket2::{Domain, Socket, Type};

    use super::*;
    use crate::sys::{sigaction, signals};

    const MS: u64 = 1_000_000;

    fn listener() -> (TcpListener, SocketAddrV4) {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let std::net::SocketAddr::V4(a) = l.local_addr().unwrap() else {
            unreachable!()
        };
        (l, a)
    }

    /// A backlog-1 listener whose accept queue is already full: further SYNs drop.
    fn full_queue() -> (Socket, SocketAddrV4, Vec<std::net::TcpStream>) {
        let s = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
        s.bind(&SockAddr::from(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
            .unwrap();
        s.listen(1).unwrap();
        let addr = s.local_addr().unwrap().as_socket_ipv4().unwrap();
        let mut held = Vec::new();
        while let Ok(c) =
            std::net::TcpStream::connect_timeout(&addr.into(), Duration::from_millis(200))
        {
            held.push(c);
            assert!(held.len() < 64, "queue never filled");
        }
        (s, addr, held)
    }

    #[test]
    fn immediate_success_on_loopback() {
        let (_l, addr) = listener();
        let a = attempt(addr, 1000 * MS);
        assert_eq!(a.outcome, Outcome::Ok);
        assert_eq!(a.errno, None);
        assert!(a.duration_ns < 50 * MS, "{} ns", a.duration_ns);
        assert_eq!(a.tcp_info.total_retrans, sockinfo::Field::Known(0));
        assert_eq!(a.eintr, 0);
    }

    #[test]
    fn refused_via_so_error_and_short() {
        let (l, addr) = listener();
        drop(l);
        let a = attempt(addr, 1000 * MS);
        assert_eq!(a.outcome, Outcome::Refused);
        assert_eq!(a.errno, Some(libc::ECONNREFUSED));
        assert!(a.duration_ns < 50 * MS);
    }

    #[test]
    fn timeout_lasts_the_timeout_and_never_much_longer() {
        let (_l, addr, _held) = full_queue();
        let a = attempt(addr, 300 * MS);
        assert_eq!(a.outcome, Outcome::Timeout);
        assert!(a.duration_ns >= 300 * MS, "{} ns", a.duration_ns);
        assert!(a.duration_ns < 350 * MS, "{} ns", a.duration_ns);
    }

    #[test]
    fn eintr_keeps_the_same_absolute_deadline() {
        let (_l, addr, _held) = full_queue();
        sigaction::install_usr2_counter().unwrap();
        let (ready, is_ready) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            signals::block_async_signals().unwrap();
            signals::unblock(Signal::SIGUSR2).unwrap(); // this thread must see EINTR
            ready.send(()).unwrap();
            attempt(addr, 400 * MS)
        });
        is_ready.recv().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        // std's id is an integer on every target, `Pthread` a pointer on musl.
        let thread = worker.as_pthread_t() as Pthread;
        pthread_kill(thread, Signal::SIGUSR2).unwrap();
        let a = worker.join().unwrap();
        assert_eq!(a.outcome, Outcome::Timeout);
        assert!(a.eintr >= 1, "the signal must have interrupted ppoll");
        assert!(
            a.duration_ns >= 400 * MS && a.duration_ns < 450 * MS,
            "{} ns",
            a.duration_ns
        );
    }

    #[test]
    fn socket_creation_failure_is_a_local_error() {
        let a = failed_before_start(std::io::Error::from_raw_os_error(libc::EMFILE));
        assert_eq!(a.outcome, Outcome::LocalError);
        assert_eq!(a.errno, Some(libc::EMFILE));
    }
}
