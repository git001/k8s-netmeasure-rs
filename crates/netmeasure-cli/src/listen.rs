//! `listen` arguments (design §4). The control port (`--control-port`,
//! `--control-bind`) follows in the next step of milestone 3.

use std::net::Ipv4Addr;

use clap::Args;

/// Default measured port (design §4.1).
pub const DEFAULT_PORT: u16 = 17700;
/// Default and maximum `listen()` backlog; the kernel caps it at `somaxconn`.
pub const DEFAULT_BACKLOG: u32 = 4096;
pub const MAX_BACKLOG: u32 = 65535;

#[derive(Debug, Clone, Args)]
pub struct ListenArgs {
    /// Measured port: raw TCP, connections are accepted and nothing is read or written.
    #[arg(
        long,
        env = "K8S_NETMEASURE_PORT",
        default_value_t = DEFAULT_PORT,
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub port: u16,

    /// IPv4 address the measured port binds to.
    #[arg(long, env = "K8S_NETMEASURE_BIND", default_value_t = Ipv4Addr::UNSPECIFIED)]
    pub bind: Ipv4Addr,

    /// listen() backlog (1–65535); the effective value is min(backlog, somaxconn).
    #[arg(
        long,
        env = "K8S_NETMEASURE_BACKLOG",
        default_value_t = DEFAULT_BACKLOG,
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_BACKLOG))
    )]
    pub backlog: u32,

    #[command(flatten)]
    pub log: crate::LogArgs,
}
