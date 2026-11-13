//! SIGUSR2 for the signal-isolation check (design §3.8): a no-op handler that only
//! counts, and sending the signal to the own process.

use std::sync::atomic::{AtomicU32, Ordering};

use nix::sys::signal::{self, SaFlags, SigAction, SigHandler, SigSet, Signal};
use nix::unistd::Pid;

static USR2_DELIVERED: AtomicU32 = AtomicU32::new(0);

extern "C" fn on_usr2(_sig: libc::c_int) {
    USR2_DELIVERED.fetch_add(1, Ordering::Relaxed);
}

/// Installs the counting handler for SIGUSR2 (without `SA_RESTART`, so a thread that
/// did not block it would really see `EINTR`).
pub fn install_usr2_counter() -> Result<(), i32> {
    let act = SigAction::new(
        SigHandler::Handler(on_usr2),
        SaFlags::empty(),
        SigSet::empty(),
    );
    // SAFETY: the handler is async-signal-safe (one atomic add) and replaces no
    // handler the program relies on (SIGUSR2 is unused otherwise).
    unsafe { signal::sigaction(Signal::SIGUSR2, &act) }
        .map(drop)
        .map_err(|e| e as i32)
}

/// Sends SIGUSR2 to the own process; the kernel picks a thread that does not block it.
pub fn raise_usr2_process() -> Result<(), i32> {
    signal::kill(Pid::this(), Signal::SIGUSR2).map_err(|e| e as i32)
}

pub fn usr2_delivered() -> u32 {
    USR2_DELIVERED.load(Ordering::Relaxed)
}
