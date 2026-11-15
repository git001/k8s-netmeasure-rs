//! One target's counted samples in a stretch of consecutive valid cycles, and the
//! counting rules per outcome (design §10.1, §10.4).

use crate::measure::outcome::Outcome;

/// A timeout in the latency distribution: larger than every measured value, meaning
/// "at least the timeout" (§10.1).
pub const CENSORED: u64 = u64::MAX;

/// Per-attempt marks, in measurement order, for the rate families (§10.4).
pub mod mark {
    /// `timeout`, `refused` or `net_error`.
    pub const FAILED: u8 = 1;
    /// At least one SYN retransmission.
    pub const SYN_RETRANSMITTED: u8 = 1 << 1;
    /// The kernel did not report SYN retransmissions.
    pub const SYN_UNKNOWN: u8 = 1 << 2;
}

/// The counted samples of one target in a stretch of consecutive valid cycles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Segment {
    /// Indices of the cycles in this segment, ascending and consecutive.
    pub cycles: Vec<u64>,
    /// Latencies in measurement order: `ok` as ns, `timeout` as [`CENSORED`].
    pub values: Vec<u64>,
    /// Connects counted for the failure rate (every outcome except local errors).
    pub attempts: u64,
    /// `timeout` + `refused` + `net_error` (§10.4).
    pub failures: u64,
    /// Connects with at least one SYN retransmission, and connects where the kernel
    /// did not report it.
    pub syn_retransmitted: u64,
    pub syn_unknown: u64,
    /// One [`mark`] byte per attempt, in measurement order (block bootstrap of rates).
    pub marks: Vec<u8>,
}

impl Segment {
    /// Counts one sample: `ok` as its duration, `timeout` censored; failures and SYN
    /// retransmissions for their own families.
    pub fn add(&mut self, o: Outcome, duration_ns: u64, syn_retrans: i64) {
        if o.invalidates_cycle() {
            return; // never in a valid cycle; kept out of every family regardless
        }
        self.attempts += 1;
        self.failures += u64::from(o.is_failure());
        let mut m = if o.is_failure() { mark::FAILED } else { 0 };
        match o {
            Outcome::Ok => self.values.push(duration_ns),
            Outcome::Timeout => self.values.push(CENSORED),
            _ => {}
        }
        match syn_retrans {
            r if r < 0 => {
                self.syn_unknown += 1;
                m |= mark::SYN_UNKNOWN;
            }
            r if r > 0 => {
                self.syn_retransmitted += 1;
                m |= mark::SYN_RETRANSMITTED;
            }
            _ => {}
        }
        self.marks.push(m);
    }

    /// Appends the next consecutive cycle.
    pub fn merge(&mut self, next: Segment) {
        self.cycles.extend(next.cycles);
        self.values.extend(next.values);
        self.attempts += next.attempts;
        self.failures += next.failures;
        self.syn_retransmitted += next.syn_retransmitted;
        self.syn_unknown += next.syn_unknown;
        self.marks.extend(next.marks);
    }
}
