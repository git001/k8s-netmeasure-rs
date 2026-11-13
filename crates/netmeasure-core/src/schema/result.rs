//! Result file (design §5, `schema_version` 1): header, contexts, targets, cycles and
//! the sample columns. `report` reads it with `deny_unknown_fields`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::control::{BuildInfo, NodeContext};

pub const SCHEMA: &str = "k8s-netmeasure/result";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultFile {
    pub schema: String,
    pub schema_version: u32,
    pub run: Run,
    pub source: SideContext,
    pub targets: Vec<Target>,
    pub measurement: Measurement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Complete,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub run_id: String,
    pub status: RunStatus,
    /// UTC, RFC 3339 with ns.
    pub started_at: String,
    pub ended_at: String,
    pub clock: Clock,
    pub labels: BTreeMap<String, String>,
    pub build_info: BuildInfo,
    pub config: Config,
    /// Parameter → `cli`, `env` or `default` (§9.1).
    pub config_source: BTreeMap<String, String>,
}

/// Maps the monotonic offsets of this file to wall-clock time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    /// Wall-clock time at monotonic offset 0 (the run origin).
    pub realtime_at_origin: String,
    pub boot_id: Option<String>,
    /// NTP synchronisation at start and end; `None` until it is read (milestone 3c2).
    pub synchronized: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub samples: u64,
    pub warmup: u64,
    pub interval_ns: u64,
    pub loopback_every: u32,
    /// Effective cycle count after clamping (§3.5).
    pub cycles: u64,
    pub cycle_slots: Vec<u64>,
    pub timeout_ns: u64,
    pub outlier_threshold_ns: u64,
    /// The own loopback control listener.
    pub listener: String,
    pub path: String,
    /// `declared` (`--path`) or `detected` (physical interfaces visible or not).
    pub path_source: String,
    /// `pair` (one target) or `sampled` (several).
    pub topology: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideContext {
    pub context_before: NodeContext,
    pub context_after: Option<NodeContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Value in the `target` column: 0 = loopback control.
    pub index: u16,
    pub name: String,
    pub addr: String,
    /// `loopback` or `listener`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub cycles: Vec<Cycle>,
    pub samples: SampleColumns,
}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantiles {
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleColumns {
    pub count: u64,
    pub result_codes: Vec<String>,
    pub target: Vec<u16>,
    /// Actual minus scheduled start.
    pub late_ns: Vec<u64>,
    pub duration_ns: Vec<u64>,
    pub result: Vec<u8>,
    /// -1 = unknown.
    pub syn_retrans: Vec<i64>,
    /// bit0 sync_connect, bit1 warmup, bit2 after_overrun, bit3 eintr, bit4 peer_closed.
    pub flags: Vec<u8>,
    pub reanchor: Reanchor,
    pub errno: SparseErrno,
}

/// Re-anchored slots only (§3.4 rule 2), with their scheduled offset from the start of
/// their cycle.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reanchor {
    pub index: Vec<u64>,
    pub sched_offset_ns: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparseErrno {
    pub index: Vec<u64>,
    pub value: Vec<i32>,
}
