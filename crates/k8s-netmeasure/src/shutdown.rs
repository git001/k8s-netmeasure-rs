//! SIGTERM / SIGINT handling (design §8) without a signal handler: both signals are
//! blocked before any other thread starts, so every thread inherits the mask, and one
//! `signals` thread takes them with `sigwait`. The first signal sets a flag that every
//! thread can read; a second one exits at once with `128 + signal`.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::sys::{signals, time};

#[derive(Clone)]
pub struct Shutdown {
    signal: Arc<AtomicI32>,
    /// Monotonic ns of the first signal; 0 before.
    at_ns: Arc<AtomicU64>,
}

impl Shutdown {
    /// Blocks SIGTERM and SIGINT and starts the `signals` thread. Must run before any
    /// other thread is started; a thread started earlier keeps its own mask and could
    /// receive the signal with its default action (termination).
    pub fn install() -> io::Result<Shutdown> {
        let set = signals::termination();
        signals::block(&set).map_err(io::Error::from_raw_os_error)?;
        let signal = Arc::new(AtomicI32::new(0));
        let at_ns = Arc::new(AtomicU64::new(0));
        let (seen, at) = (Arc::clone(&signal), Arc::clone(&at_ns));
        std::thread::Builder::new()
            .name("signals".into())
            .spawn(move || {
                loop {
                    let Ok(sig) = signals::wait(&set) else {
                        tracing::error!("sigwait failed; signals are no longer handled");
                        return;
                    };
                    let sig = sig as i32;
                    let now = time::monotonic_ns();
                    // Time first: whoever sees the signal also sees its time.
                    let _ = at.compare_exchange(0, now, Ordering::SeqCst, Ordering::SeqCst);
                    if seen
                        .compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        tracing::info!(signal = sig, "shutdown requested");
                    } else {
                        tracing::warn!(signal = sig, "second signal, exiting at once");
                        std::process::exit(exit_status(sig));
                    }
                }
            })?;
        Ok(Shutdown { signal, at_ns })
    }

    /// Monotonic ns of the first signal, if any.
    pub fn requested_at_ns(&self) -> Option<u64> {
        self.requested()?;
        Some(self.at_ns.load(Ordering::SeqCst))
    }

    /// The first signal received, if any.
    pub fn requested(&self) -> Option<i32> {
        match self.signal.load(Ordering::SeqCst) {
            0 => None,
            s => Some(s),
        }
    }
}

/// `128 + signal`: 143 for SIGTERM, 130 for SIGINT (design §8).
pub fn exit_status(signal: i32) -> i32 {
    128 + signal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_follows_the_shell_convention() {
        assert_eq!(exit_status(libc::SIGTERM), 143);
        assert_eq!(exit_status(libc::SIGINT), 130);
    }
}
