//! `probe` arguments (design §3.9).

use std::net::SocketAddrV4;
use std::time::Duration;

use clap::Args;

/// Limits of `--count` (design §3.9).
pub const MAX_COUNT: u32 = 100;
/// Limits of `--timeout`: long enough for several SYN retransmissions.
pub const MIN_TIMEOUT: Duration = Duration::from_millis(1);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Args)]
pub struct ProbeArgs {
    /// Target as IPv4 address:port, e.g. 10.0.0.5:17700.
    #[arg(long, env = "K8S_NETMEASURE_TARGET", value_parser = crate::parse::ipv4_target)]
    pub target: SocketAddrV4,

    /// Number of connects, one after another (1–100).
    #[arg(
        long,
        env = "K8S_NETMEASURE_COUNT",
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_COUNT))
    )]
    pub count: u32,

    /// Per-connect timeout (1ms–60s), e.g. 5s or 300ms.
    #[arg(
        long,
        env = "K8S_NETMEASURE_TIMEOUT",
        default_value = "5s",
        value_parser = timeout
    )]
    pub timeout: Duration,

    #[command(flatten)]
    pub log: crate::LogArgs,
}

fn timeout(text: &str) -> Result<Duration, String> {
    let d = netmeasure_util::time::parse_duration(text)?;
    if d < MIN_TIMEOUT || d > MAX_TIMEOUT {
        return Err(format!("--timeout must be between 1ms and 60s, got {text}"));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_bounds() {
        assert!(timeout("1ms").is_ok());
        assert!(timeout("60s").is_ok());
        assert!(timeout("61s").unwrap_err().contains("between 1ms and 60s"));
        assert!(timeout("500us").is_err());
    }
}
