//! `k8s-netmeasure`: reproducible TCP connect-latency measurement (design README).
//!
//! Milestone 1 (scaffold, design §14): the CLI frame and build information only. The
//! subcommands `listen`, `run`, `report`, `selftest` and `probe` follow in the next
//! milestones.
#![deny(unsafe_code)]
#![forbid(clippy::too_many_arguments)]

mod build_info;

use std::process::ExitCode;
use std::sync::LazyLock;

use clap::FromArgMatches;
use netmeasure_cli::Cli;

use crate::build_info::BUILD_INFO;

static VERSION_LINE: LazyLock<String> = LazyLock::new(|| BUILD_INFO.version_line());

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    // `--help`/`--version` exit 0; usage errors exit 2 (design §8).
    let matches = netmeasure_cli::command(VERSION_LINE.as_str()).get_matches();
    let _cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    ExitCode::SUCCESS
}
