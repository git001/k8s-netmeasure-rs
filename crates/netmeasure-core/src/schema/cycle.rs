//! One measurement cycle in the result file (design §3.5, §3.6, §5): sample range,
//! timing, validity and the quality figures behind it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cycle {
    pub cycle_id: String,
    pub index: u64,
    /// Sample range `[first_sample, end_sample)`.
    pub first_sample: u64,
    pub end_sample: u64,
    /// Monotonic ns from the run origin to this cycle's first scheduled slot; slot `k`
    /// of the cycle is scheduled at `start_offset_ns + k × interval` unless re-anchored.
    pub start_offset_ns: u64,
    pub started_at: String,
    pub ended_at: String,
    pub validity: Validity,
    pub quality: Quality,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Validity {
    pub valid: bool,
    pub reasons: Vec<String>,
    pub annotations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub wake_lateness_ns: Quantiles,
    pub overruns: u64,
    pub cpu: CpuDelta,
    /// `SK_MEMINFO_DROPS` delta of the own loopback control listener (target listeners
    /// follow with their control ports).
    pub listener_drops: Option<u64>,
}

/// Throttling during the cycle; `None` if unreadable or unchecked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuDelta {
    pub throttled_usec_delta: Option<u64>,
    /// Sum over the visible ancestors with a quota (§3.7).
    pub ancestors_throttled_usec_delta: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantiles {
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
}
