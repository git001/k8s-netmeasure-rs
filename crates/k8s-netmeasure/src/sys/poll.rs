//! `ppoll` for one socket's `POLLOUT` with a nanosecond timeout (design §3.1).

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

/// Waits at most `timeout_ns`; 0 checks readiness once (never a negative timespec).
/// No signal mask change during the wait (`sigmask` `None`).
pub fn wait_writable(fd: BorrowedFd<'_>, timeout_ns: u64) -> PollResult {
    let mut fds = [PollFd::new(fd, PollFlags::POLLOUT)];
    match ppoll(&mut fds, Some(timespec(timeout_ns)), None) {
        Ok(0) => PollResult::TimedOut,
        Ok(_) => PollResult::Event,
        Err(Errno::EINTR) => PollResult::Interrupted,
        Err(e) => PollResult::Failed(e as i32),
    }
}
