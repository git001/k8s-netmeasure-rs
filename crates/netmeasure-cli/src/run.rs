//! `run` arguments (design §3.4, §3.5, §3.7, §5, §7, §8, §9);
//! background load in milestone 6.

use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, ValueEnum};

use crate::parse::ipv4_target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PathKind {
    /// Pod network (CNI overlay).
    Overlay,
    /// Node network (hostNetwork).
    Underlay,
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// Target as IPv4 address:port, repeatable (comma-separated in the env form).
    /// One target is `pair` topology, several are `sampled`.
    #[arg(long, env = "K8S_NETMEASURE_TARGET", required = true, value_delimiter = ',', value_parser = ipv4_target)]
    pub target: Vec<SocketAddrV4>,

    /// Control port of every target (same IP as the target). `run` reads each
    /// target's identity, context and counters there before and after every cycle.
    #[arg(long, env = "K8S_NETMEASURE_TARGET_CONTROL_PORT", default_value_t = 17701,
          value_parser = clap::value_parser!(u16).range(1..))]
    pub target_control_port: u16,

    /// Targets have no control port (any TCP listener, e.g. a node's kubelet): the
    /// target-side rules (listener drops, restarts, NIC path) are not checked and
    /// every cycle is annotated `target_unchecked`.
    #[arg(long, env = "K8S_NETMEASURE_NO_TARGET_CONTROL", action = clap::ArgAction::SetTrue,
          value_parser = clap::builder::BoolValueParser::new(), conflicts_with = "target_control_port")]
    pub no_target_control: bool,

    /// Counted samples per real target, after warm-up (1–1000000).
    #[arg(long, env = "K8S_NETMEASURE_SAMPLES", default_value_t = 5000,
          value_parser = clap::value_parser!(u64).range(1..=1_000_000))]
    pub samples: u64,

    /// Warm-up samples per target, measured but excluded from statistics (0–10000).
    #[arg(long, env = "K8S_NETMEASURE_WARMUP", default_value_t = 50,
          value_parser = clap::value_parser!(u64).range(0..=10_000))]
    pub warmup: u64,

    /// Start-to-start spacing of slots (100us–10s).
    #[arg(long, env = "K8S_NETMEASURE_INTERVAL", default_value = "10ms",
          value_parser = |t: &str| bounded(t, "--interval", Duration::from_micros(100), Duration::from_secs(10)))]
    pub interval: Duration,

    /// Every n-th slot goes to the own loopback control (2–1000; 2 = strict alternation).
    #[arg(long, env = "K8S_NETMEASURE_LOOPBACK_EVERY", default_value_t = 10,
          value_parser = clap::value_parser!(u32).range(2..=1000))]
    pub loopback_every: u32,

    /// Number of cycles (1–1000); the effective count keeps a cycle between 2 s and 60 s.
    #[arg(long, env = "K8S_NETMEASURE_CYCLES", conflicts_with = "cycle",
          value_parser = clap::value_parser!(u64).range(1..=1000))]
    pub cycles: Option<u64>,

    /// Fixed cycle length instead of --cycles (2s–60s).
    #[arg(long, env = "K8S_NETMEASURE_CYCLE",
          value_parser = |t: &str| bounded(t, "--cycle", Duration::from_secs(2), Duration::from_secs(60)))]
    pub cycle: Option<Duration>,

    /// Per-connect timeout (1ms–60s).
    #[arg(long, env = "K8S_NETMEASURE_TIMEOUT", default_value = "5s",
          value_parser = |t: &str| bounded(t, "--timeout", Duration::from_millis(1), Duration::from_secs(60)))]
    pub timeout: Duration,

    /// Samples slower than this are logged at WARN after their cycle (1ms–60s).
    #[arg(long, env = "K8S_NETMEASURE_OUTLIER_THRESHOLD", default_value = "20ms",
          value_parser = |t: &str| bounded(t, "--outlier-threshold", Duration::from_millis(1), Duration::from_secs(60)))]
    pub outlier_threshold: Duration,

    /// listen() backlog of the own loopback control listener (1–65535).
    #[arg(long, env = "K8S_NETMEASURE_BACKLOG", default_value_t = 4096,
          value_parser = clap::value_parser!(u32).range(1..=65535))]
    pub backlog: u32,

    /// Which network the targets are reached through; if not set, detected: underlay
    /// when physical interfaces are visible, else overlay.
    #[arg(long, env = "K8S_NETMEASURE_PATH", value_enum)]
    pub path: Option<PathKind>,

    /// Measure even if the own cgroup v2 is unreadable; throttling is then not
    /// checked and every cycle is annotated `cpu_throttle_unchecked`.
    #[arg(long, env = "K8S_NETMEASURE_NO_CGROUP_CHECK", action = clap::ArgAction::SetTrue,
          value_parser = clap::builder::BoolValueParser::new())]
    pub no_cgroup_check: bool,

    /// After the first SIGTERM/SIGINT: the running cycle is finished if it fits into
    /// this time, else it ends at once as `interrupted` (1ms–300s). Keep
    /// terminationGracePeriodSeconds ≥ this + --timeout + 10s.
    #[arg(long, env = "K8S_NETMEASURE_SHUTDOWN_DEADLINE", default_value = "10s",
          value_parser = |t: &str| bounded(t, "--shutdown-deadline", Duration::from_millis(1), Duration::from_secs(300)))]
    pub shutdown_deadline: Duration,

    /// Serve probes and /v1/* on this port (off by default, so a measurement never
    /// fails because a port is taken; suggested 17702).
    #[arg(long, env = "K8S_NETMEASURE_CONTROL_PORT", value_parser = clap::value_parser!(u16).range(1..))]
    pub control_port: Option<u16>,

    /// IPv4 address of the control port; `auto` = the address of the default-route
    /// interface. Only used with --control-port.
    #[arg(long, env = "K8S_NETMEASURE_CONTROL_BIND", default_value_t = crate::ControlBind::Auto,
          value_parser = crate::listen::control_bind)]
    pub control_bind: crate::ControlBind,

    /// Result file (JSON, written atomically at the end), or an existing directory:
    /// then the name is `<path>-<node>-<UTC start>-<run_id>.json`. An existing file is
    /// never overwritten (exit 3).
    #[arg(long, env = "K8S_NETMEASURE_OUTPUT")]
    pub output: PathBuf,

    /// Label key=value for the result file and logs, repeatable.
    #[arg(long, env = "K8S_NETMEASURE_LABEL", value_delimiter = ',', value_parser = crate::labels::item)]
    pub label: Vec<(String, String)>,

    #[command(flatten)]
    pub context: crate::ContextArgs,

    #[command(flatten)]
    pub log: crate::LogArgs,
}

fn bounded(text: &str, flag: &str, min: Duration, max: Duration) -> Result<Duration, String> {
    let d = netmeasure_util::time::parse_duration(text)?;
    if d < min || d > max {
        let f = netmeasure_util::time::format_duration;
        return Err(format!(
            "{flag} must be between {} and {}, got {text}",
            f(min),
            f(max)
        ));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_bounds() {
        let (lo, hi) = (Duration::from_millis(1), Duration::from_secs(1));
        assert!(bounded("1ms", "--x", lo, hi).is_ok());
        assert!(
            bounded("2s", "--x", lo, hi)
                .unwrap_err()
                .contains("--x must be between")
        );
    }
}
