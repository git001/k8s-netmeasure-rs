//! The watched threads of `listen` (design §7).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::control::state::Heartbeat;
use crate::listener::Listener;

/// Liveness bounds (§7): accept thread 5 s (it beats at least every second); the main
/// thread of `listen` only waits for signals, so the same bound applies.
const ACCEPT_BOUND_NS: u64 = 5_000_000_000;
const MAIN_BOUND_NS: u64 = 5_000_000_000;

pub fn heartbeats(listener: &Listener, main_beat: &Arc<AtomicU64>) -> Vec<Heartbeat> {
    let handle = listener.handle();
    let main = Arc::clone(main_beat);
    vec![
        Heartbeat {
            name: "accept",
            read: Box::new(move || handle.stats().heartbeat_ns),
            bound_ns: ACCEPT_BOUND_NS,
        },
        Heartbeat {
            name: "main",
            read: Box::new(move || main.load(Ordering::Relaxed)),
            bound_ns: MAIN_BOUND_NS,
        },
    ]
}
