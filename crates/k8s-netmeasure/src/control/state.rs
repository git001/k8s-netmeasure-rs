//! What the control server reports (design §7): startup and shutdown flags, the
//! preassembled `/v1/info` body and the heartbeats of the watched threads. The server
//! only reads atomics and preassembled values, so it never blocks another thread.

use std::sync::atomic::{AtomicBool, Ordering};

/// A watched thread: `read` returns its last heartbeat (monotonic ns); it is stalled if
/// that is older than `bound_ns`.
pub struct Heartbeat {
    pub name: &'static str,
    pub read: Box<dyn Fn() -> u64 + Send + Sync>,
    pub bound_ns: u64,
}

pub struct State {
    started: AtomicBool,
    stopping: AtomicBool,
    info: serde_json::Value,
    heartbeats: Vec<Heartbeat>,
}

impl State {
    pub fn new(info: serde_json::Value, heartbeats: Vec<Heartbeat>) -> State {
        State {
            started: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            info,
            heartbeats,
        }
    }

    /// Startup finished (§4.1 start order); stays set for the life of the process.
    pub fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    /// The first shutdown signal arrived: `/ready` turns 503 at once (§8).
    pub fn mark_stopping(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    pub fn started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    pub fn info(&self) -> &serde_json::Value {
        &self.info
    }

    /// Names of the threads whose heartbeat is older than their bound at `now_ns`.
    pub fn stalled(&self, now_ns: u64) -> Vec<&'static str> {
        self.heartbeats
            .iter()
            .filter(|h| now_ns.saturating_sub((h.read)()) > h.bound_ns)
            .map(|h| h.name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn flags_and_stalled_threads() {
        let beat = Arc::new(AtomicU64::new(1_000));
        let b = Arc::clone(&beat);
        let s = State::new(
            serde_json::json!({ "k": 1 }),
            vec![Heartbeat {
                name: "accept",
                read: Box::new(move || b.load(Ordering::Relaxed)),
                bound_ns: 100,
            }],
        );
        assert!(!s.started() && !s.stopping());
        s.mark_started();
        s.mark_stopping();
        assert!(s.started() && s.stopping());
        assert_eq!(s.info()["k"], 1);
        assert!(s.stalled(1_100).is_empty());
        assert_eq!(s.stalled(1_101), vec!["accept"]);
    }
}
