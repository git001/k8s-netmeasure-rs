//! The dedicated measuring thread (design §2, §3.1): all asynchronous signals
//! blocked, timer slack 1 ns. It receives work as a closure and returns its result.

use std::thread;

use crate::sys::{signals, time};

/// Setup facts of the measuring thread, reported by `selftest` and in result files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadSetup {
    pub signals_blocked: bool,
    /// `None` if `PR_SET_TIMERSLACK` succeeded, else the errno.
    pub timer_slack_errno: Option<i32>,
}

/// Runs `work` on a new measuring thread and waits for it.
pub fn run<T: Send + 'static>(work: impl FnOnce(ThreadSetup) -> T + Send + 'static) -> T {
    match spawn(work).join() {
        Ok(v) => v,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Starts `work` on a new measuring thread (for `run`, which talks to it per cycle).
pub fn spawn<T: Send + 'static>(
    work: impl FnOnce(ThreadSetup) -> T + Send + 'static,
) -> thread::JoinHandle<T> {
    thread::Builder::new()
        .name("measure".to_owned())
        .spawn(move || {
            let setup = ThreadSetup {
                signals_blocked: signals::block_async_signals().is_ok(),
                timer_slack_errno: time::set_timer_slack(1).err(),
            };
            work(setup)
        })
        .expect("spawning the measuring thread")
}
