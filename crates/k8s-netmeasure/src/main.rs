//! `k8s-netmeasure`: reproducible TCP connect-latency measurement (design README).
//!
//! Subcommands so far: `probe` and `selftest` (milestone 2, design §14); `listen`,
//! `run` and `report` follow in the next milestones.
#![deny(unsafe_code)]
#![forbid(clippy::too_many_arguments)]

mod build_info;
mod cmd;
mod diag;
mod exit;
mod logging;
mod measure;
mod output;
mod sys;

use std::process::ExitCode;
use std::sync::LazyLock;

use clap::FromArgMatches;
use netmeasure_cli::{Cli, Subcommands};

use crate::build_info::BUILD_INFO;

static VERSION_LINE: LazyLock<String> = LazyLock::new(|| BUILD_INFO.version_line());

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    // `--help`/`--version` exit 0; usage errors exit 2 (design §8).
    let command = netmeasure_cli::command(VERSION_LINE.as_str());
    let matches = command.clone().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let env = match netmeasure_cli::env::check(&command, &matches, std::env::vars_os()) {
        Ok(env) => env,
        Err(errors) => {
            for e in &errors {
                eprintln!("error: {e}");
            }
            return exit::code(exit::USAGE);
        }
    };
    logging::init(cli.command.log().log_level);
    for (var, owners) in &env.ignored {
        tracing::info!(variable = %var, belongs_to = %owners.join(","), "ignoring variable of another subcommand");
    }
    for (param, source) in &env.sources {
        tracing::debug!(parameter = %param, source = %source, "parameter source");
    }
    match &cli.command {
        Subcommands::Probe(args) => cmd::probe::run(args),
        Subcommands::Selftest(args) => cmd::selftest::run(args, &env.sources),
    }
}
