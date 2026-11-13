//! `ppoll` for one socket with a nanosecond timeout (design §3.1, §4.2).

use std::os::fd::BorrowedFd;

use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, ppoll};

use super::time::timespec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollResult {
    /// Any `revents` (`POLLOUT`, `POLLERR`, `POLLHUP`): the attempt finished.
    Event,
    TimedOut,
    /// `EINTR`: the caller re-waits for the remaining time only.
    Interrupted,
    Failed(i32),
}

/// Waits at most `timeout_ns` for `POLLOUT` (connect finished); 0 checks readiness once
/// (never a negative timespec). No signal mask change during the wait (`sigmask` `None`).
pub fn wait_writable(fd: BorrowedFd<'_>, timeout_ns: u64) -> PollResult {
    wait(fd, PollFlags::POLLOUT, timeout_ns)
}

/// Like [`wait_writable`], for `POLLIN` (a listening socket has a connection to accept).
pub fn wait_readable(fd: BorrowedFd<'_>, timeout_ns: u64) -> PollResult {
    wait(fd, PollFlags::POLLIN, timeout_ns)
}

fn wait(fd: BorrowedFd<'_>, events: PollFlags, timeout_ns: u64) -> PollResult {
    let mut fds = [PollFd::new(fd, events)];
    match ppoll(&mut fds, Some(timespec(timeout_ns)), None) {
        Ok(0) => PollResult::TimedOut,
        Ok(_) => PollResult::Event,
        Err(Errno::EINTR) => PollResult::Interrupted,
        Err(e) => PollResult::Failed(e as i32),
    }
}
