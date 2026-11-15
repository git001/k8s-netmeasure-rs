//! One target's counted samples in a stretch of consecutive valid cycles, and the
//! counting rules per outcome (design §10.1, §10.4).

use crate::measure::outcome::Outcome;

/// A timeout in the latency distribution: larger than every measured value, meaning
/// "at least the timeout" (§10.1).
pub const CENSORED: u64 = u64::MAX;

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
        match o {
            Outcome::Ok => self.values.push(duration_ns),
            Outcome::Timeout => self.values.push(CENSORED),
            _ => {}
        }
        match syn_retrans {
            r if r < 0 => self.syn_unknown += 1,
            r if r > 0 => self.syn_retransmitted += 1,
            _ => {}
        }
    }

    /// Appends the next consecutive cycle.
    pub fn merge(&mut self, next: Segment) {
        self.cycles.extend(next.cycles);
        self.values.extend(next.values);
        self.attempts += next.attempts;
        self.failures += next.failures;
        self.syn_retransmitted += next.syn_retransmitted;
        self.syn_unknown += next.syn_unknown;
    }
}
