//! `selftest` arguments (design §3.8). The node-listener check
//! (`--node-context-from`) arrives together with `listen` in milestone 3.

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct SelftestArgs {
    /// Also write the full result as JSON to this file (atomic write).
    #[arg(long, env = "K8S_NETMEASURE_OUTPUT")]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub log: crate::LogArgs,
}
