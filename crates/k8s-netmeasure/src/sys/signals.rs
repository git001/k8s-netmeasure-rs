//! Signal mask of the measuring thread (design §3.1): every asynchronous signal is
//! blocked, so the kernel delivers SIGTERM/SIGINT/… to another thread and the timed
//! section sees no `EINTR`. Synchronous fault signals stay unblocked.

use nix::sys::signal::{SigSet, Signal};

/// Signals the kernel raises for a fault of the thread itself; blocking them would
/// hide a crash.
const SYNCHRONOUS: [Signal; 5] = [
    Signal::SIGSEGV,
    Signal::SIGBUS,
    Signal::SIGFPE,
    Signal::SIGILL,
    Signal::SIGTRAP,
];

/// Blocks all asynchronous signals for the calling thread only.
pub fn block_async_signals() -> Result<(), i32> {
    let mut set = SigSet::all();
    for sig in SYNCHRONOUS {
        set.remove(sig);
    }
    set.thread_block().map_err(|e| e as i32)
}

/// Unblocks `signal` for the calling thread (tests: the EINTR case of §11).
#[cfg(test)]
pub fn unblock(signal: Signal) -> Result<(), i32> {
    SigSet::from(signal).thread_unblock().map_err(|e| e as i32)
}

/// Whether `signal` is blocked in the calling thread (`false` if the mask cannot be
/// read).
pub fn is_blocked(signal: Signal) -> bool {
    SigSet::thread_get_mask().is_ok_and(|mask| mask.contains(signal))
}
