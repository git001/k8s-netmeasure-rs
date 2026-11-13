//! Result file (design §5, `schema_version` 1): header, contexts, targets, cycles and
//! the sample columns. `report` reads it with `deny_unknown_fields`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::control::{BuildInfo, NodeContext};
pub use super::cycle::{CpuDelta, Cycle, ListenerDrops, Quality, Quantiles, Validity};
pub use super::diagnostics::{CycleDiagnostics, Diagnostics, SourceDiagnostics, TargetDiagnostics};

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
    pub diagnostics: Diagnostics,
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
    pub cgroup: CgroupCheck,
}

/// The CPU checks at start (§3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CgroupCheck {
    /// Own cgroup v2 path; `None` without cgroup v2.
    pub path: Option<String>,
    /// Own `cpu.max` in millicores; `None` = unlimited or unreadable.
    pub quota_millicores: Option<u64>,
    pub floor_millicores: u64,
    pub visible_ancestors: Vec<String>,
    /// `observed`, `inferred_impossible` or `unknown`.
    pub ancestor_cpu_throttling: String,
    pub note: Option<String>,
}

/// Maps the monotonic offsets of this file to wall-clock time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    /// Wall-clock time at monotonic offset 0 (the run origin).
    pub realtime_at_origin: String,
    pub boot_id: Option<String>,
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
    /// `--no-cgroup-check`: throttling is not checked, every cycle is annotated.
    pub no_cgroup_check: bool,
    /// `--target-control-port`; `None` with `--no-target-control`.
    pub target_control_port: Option<u16>,
    /// `--shutdown-deadline` (§8).
    pub shutdown_deadline_ns: u64,
    /// The own control port (`--control-port`), `None` if off.
    pub control: Option<String>,
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
    /// Control port (`<ip>:<port>`); `None` for the loopback control and with
    /// `--no-target-control`.
    pub control: Option<String>,
    /// From `/v1/info` at start; a different value later means the target restarted.
    pub listener_instance: Option<String>,
    pub context_before: Option<NodeContext>,
    pub context_after: Option<NodeContext>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub cycles: Vec<Cycle>,
    pub samples: SampleColumns,
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
