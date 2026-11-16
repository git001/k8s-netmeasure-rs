//! `report` arguments (design §10.4–§10.7). Phase 1 so far: the sequential design
//! with the factors `kernel` and `nic-settings`, and the concurrent design with the
//! factor `nic` (one data set, groups by observed NIC family); crossover, the factor
//! `load` and JSON output follow.

use std::path::PathBuf;

use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Design {
    /// Before and after on the same nodes, one after the other.
    Sequential,
    /// The compared groups measured at the same time (`--factor nic`).
    Concurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Factor {
    /// The kernel changed between the sides; other context differences are checked.
    Kernel,
    /// NIC settings (ring sizes) changed on the same nodes; the kernel and the NIC
    /// family must not change.
    NicSettings,
    /// NIC families compared in one data set measured at the same time
    /// (observational: the family comes with its host generation).
    Nic,
}

impl Factor {
    /// The name as on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Factor::Kernel => "kernel",
            Factor::NicSettings => "nic-settings",
            Factor::Nic => "nic",
        }
    }
}

/// `--equivalence-margin`: `10%` (relative, on the ratio) or `50us` (absolute).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarginArg {
    Relative(f64),
    AbsoluteNs(u64),
}

#[derive(Debug, Clone, Args)]
pub struct ReportArgs {
    /// Result files or directories (searched recursively) of the reference side
    /// (`kernel`, `nic-settings`).
    #[arg(long, env = "K8S_NETMEASURE_BEFORE", required_unless_present = "runs",
          conflicts_with = "runs", num_args = 1.., value_delimiter = ',')]
    pub before: Vec<PathBuf>,

    /// Result files or directories of the side under test (`kernel`, `nic-settings`).
    #[arg(long, env = "K8S_NETMEASURE_AFTER", required_unless_present = "runs",
          conflicts_with = "runs", num_args = 1.., value_delimiter = ',')]
    pub after: Vec<PathBuf>,

    /// One data set whose groups are the observed NIC families (`--factor nic`).
    #[arg(long, env = "K8S_NETMEASURE_RUNS", num_args = 1.., value_delimiter = ',')]
    pub runs: Vec<PathBuf>,

    /// The family the others are compared with (`--factor nic`), as the report names
    /// it (e.g. `drv-a+drv-b`). Default: the first in alphabetical order.
    #[arg(long, env = "K8S_NETMEASURE_REFERENCE_FAMILY")]
    pub reference_family: Option<String>,

    /// Study design, declared before the measurement (required).
    #[arg(long, env = "K8S_NETMEASURE_DESIGN", value_enum)]
    pub design: Design,

    /// The variable the comparison is about (required).
    #[arg(long, env = "K8S_NETMEASURE_FACTOR", value_enum)]
    pub factor: Factor,

    /// Equivalence margin of the primary quantile: `10%` or `50us`. Without it the
    /// verdict is never `equivalent`.
    #[arg(long, env = "K8S_NETMEASURE_EQUIVALENCE_MARGIN", value_parser = margin,
          requires = "equivalence_rationale")]
    pub equivalence_margin: Option<MarginArg>,

    /// Why this margin: printed in the report header (required with a margin).
    #[arg(long, env = "K8S_NETMEASURE_EQUIVALENCE_RATIONALE")]
    pub equivalence_rationale: Option<String>,

    /// The only latency quantile with a verdict, declared before the measurement.
    #[arg(long, env = "K8S_NETMEASURE_PRIMARY", default_value = "p99", value_parser = quantile)]
    pub primary: f64,

    /// Quantiles to report (comma-separated); the primary is added if missing.
    #[arg(long, env = "K8S_NETMEASURE_QUANTILES", default_value = "p50,p90,p99",
          value_delimiter = ',', value_parser = quantile)]
    pub quantiles: Vec<f64>,

    /// Bootstrap replicates (1000–100000).
    #[arg(long, env = "K8S_NETMEASURE_REPLICATES", default_value_t = 10_000,
          value_parser = clap::value_parser!(u32).range(1000..=100_000))]
    pub replicates: u32,

    /// RNG seed of the bootstrap; printed so that a report can be reproduced.
    /// Default: derived from the clock.
    #[arg(long, env = "K8S_NETMEASURE_SEED")]
    pub seed: Option<u64>,

    /// Context differences to accept instead of refusing (exit 5), comma-separated:
    /// `nic_family`, `cpu_model`, `kernel_release` (refused only with `nic-settings`).
    #[arg(long, env = "K8S_NETMEASURE_ALLOW_CONTEXT_MISMATCH", value_delimiter = ',',
          value_parser = ["nic_family", "cpu_model", "kernel_release"])]
    pub allow_context_mismatch: Vec<String>,

    #[command(flatten)]
    pub log: crate::LogArgs,
}

/// `p99` → 0.99, `p99.9` → 0.999, `p50` → 0.5.
pub fn quantile(text: &str) -> Result<f64, String> {
    let n: f64 = text
        .strip_prefix('p')
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("quantile must look like p99 or p99.9, got {text}"))?;
    if !(n > 0.0 && n < 100.0) {
        return Err(format!("quantile must be between p0 and p100, got {text}"));
    }
    Ok(n / 100.0)
}

fn margin(text: &str) -> Result<MarginArg, String> {
    if let Some(pct) = text.strip_suffix('%') {
        let v: f64 = pct.parse().map_err(|_| format!("invalid margin {text}"))?;
        if !(v > 0.0 && v < 100.0) {
            return Err(format!(
                "relative margin must be between 0% and 100%, got {text}"
            ));
        }
        return Ok(MarginArg::Relative(v / 100.0));
    }
    let d = netmeasure_util::time::parse_duration(text)
        .map_err(|e| format!("margin must be like 10% or 50us: {e}"))?;
    Ok(MarginArg::AbsoluteNs(netmeasure_util::time::as_nanos_u64(
        d,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_and_margins() {
        assert_eq!(quantile("p99").unwrap(), 0.99);
        assert!((quantile("p99.9").unwrap() - 0.999).abs() < 1e-12);
        assert!(quantile("99").is_err() && quantile("p100").is_err() && quantile("p0").is_err());
        assert_eq!(margin("10%").unwrap(), MarginArg::Relative(0.1));
        assert_eq!(margin("50us").unwrap(), MarginArg::AbsoluteNs(50_000));
        assert!(margin("0%").is_err() && margin("x").is_err());
    }
}
