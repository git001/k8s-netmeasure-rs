//! `listen` (design §4): the measurement target. Binds the measured port, accepts
//! until SIGTERM or SIGINT, then stops accepting, closes and exits with `128 + signal`.
//! The control port (§7) follows in the next step of milestone 3.

use std::net::SocketAddrV4;
use std::process::ExitCode;
use std::time::Duration;

use netmeasure_cli::ListenArgs;

use crate::exit;
use crate::listener::Listener;
use crate::shutdown::{self, Shutdown};

/// How often the main thread checks for a shutdown signal.
const TICK: Duration = Duration::from_millis(100);

pub fn run(args: &ListenArgs) -> ExitCode {
    let ListenArgs {
        port,
        bind,
        backlog,
        log: _,
    } = args.clone();
    let shutdown = match Shutdown::install() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "cannot install signal handlers");
            return exit::code(exit::INTERNAL);
        }
    };
    let addr = SocketAddrV4::new(bind, port);
    let listener = match Listener::start(addr, backlog) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(addr = %addr, error = %e, "cannot bind the measured port");
            return exit::code(exit::REFUSED);
        }
    };
    tracing::info!(
        addr = %listener.local,
        backlog,
        effective_backlog = listener.effective_backlog,
        "listening"
    );
    let signal = loop {
        if let Some(s) = shutdown.requested() {
            break s;
        }
        std::thread::sleep(TICK);
    };
    let drops = listener.drops();
    let stats = listener.stop();
    tracing::info!(
        signal,
        accepted = stats.accepted,
        aborted = stats.aborted,
        errors = stats.errors,
        listener_drops = drops.known(),
        "shut down"
    );
    exit::code(u8::try_from(shutdown::exit_status(signal)).unwrap_or(exit::INTERNAL))
}
