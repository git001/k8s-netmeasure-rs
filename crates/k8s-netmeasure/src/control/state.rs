//! What the control server reports (design §7): startup and shutdown flags, the
//! endpoint bodies and the heartbeats of the watched threads. Bodies are built on the
//! control thread from atomics, `/proc`, `/sys` and one `ioctl` per interface, so a
//! request never blocks another thread.

use std::sync::atomic::{AtomicBool, Ordering};

/// A watched thread: `read` returns its last heartbeat (monotonic ns); it is stalled if
/// that is older than `bound_ns`.
pub struct Heartbeat {
    pub name: &'static str,
    pub read: Box<dyn Fn() -> u64 + Send + Sync>,
    pub bound_ns: u64,
}

/// A body built on request.
pub type Body = Box<dyn Fn() -> serde_json::Value + Send + Sync>;

/// The bodies of `/v1/info` (fixed), `/v1/context` and `/v1/snapshot`.
pub struct Endpoints {
    pub info: serde_json::Value,
    pub context: Body,
    pub snapshot: Body,
}

pub struct State {
    started: AtomicBool,
    stopping: AtomicBool,
    endpoints: Endpoints,
    heartbeats: Vec<Heartbeat>,
}

impl State {
    pub fn new(endpoints: Endpoints, heartbeats: Vec<Heartbeat>) -> State {
        State {
            started: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            endpoints,
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

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
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
            Endpoints {
                info: serde_json::json!({ "k": 1 }),
                context: Box::new(|| serde_json::json!({ "c": 2 })),
                snapshot: Box::new(|| serde_json::json!({ "s": 3 })),
            },
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
        assert_eq!(s.endpoints().info["k"], 1);
        assert_eq!((s.endpoints().context)()["c"], 2);
        assert_eq!((s.endpoints().snapshot)()["s"], 3);
        assert!(s.stalled(1_100).is_empty());
        assert_eq!(s.stalled(1_101), vec!["accept"]);
    }
}
