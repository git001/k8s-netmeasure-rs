//! `selftest` arguments (design §3.8).

use std::net::SocketAddrV4;
use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct SelftestArgs {
    /// Also write the full result as JSON to this file (atomic write).
    #[arg(long, env = "K8S_NETMEASURE_OUTPUT")]
    pub output: Option<PathBuf>,

    /// Also check the node listener at this control port (IPv4:port): reachable,
    /// same node (boot id), NIC path visible.
    #[arg(long, env = "K8S_NETMEASURE_NODE_CONTEXT_FROM", value_parser = crate::parse::ipv4_target)]
    pub node_context_from: Option<SocketAddrV4>,

    #[command(flatten)]
    pub log: crate::LogArgs,
}
