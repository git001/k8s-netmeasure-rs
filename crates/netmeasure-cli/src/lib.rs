//! clap command definitions (design §2, §9.1).
//!
//! This crate exists only because clap's derive output allows the whole clippy
//! `complexity` group, which is a hard error (E0453) under
//! `#![forbid(clippy::too_many_arguments)]`. The CLI types therefore live here, under
//! `deny` instead of `forbid`; `cargo xtask line-limit` still rejects every
//! hand-written `allow` of the lint or of its groups. Reading argv and the
//! environment happens in the binary; everything here takes them as input.
#![forbid(unsafe_code)]
#![deny(clippy::too_many_arguments)]

pub mod context;
pub mod env;
pub mod listen;
pub mod log;
pub mod parse;
pub mod probe;
pub mod selftest;
pub mod surface;

use clap::{Command, CommandFactory, Parser, Subcommand};

pub use crate::context::ContextArgs;
pub use crate::listen::{ControlBind, ListenArgs};
pub use crate::log::{LogArgs, LogLevel};
pub use crate::probe::ProbeArgs;
pub use crate::selftest::SelftestArgs;

/// Environment variable prefix for every parameter (design §9.1).
pub const ENV_PREFIX: &str = "K8S_NETMEASURE_";

#[derive(Debug, Parser)]
#[command(
    name = "k8s-netmeasure",
    about = "Reproducible TCP connect-latency measurement in Kubernetes and OpenShift",
    long_about = "Reproducible TCP connect-latency measurement in Kubernetes and OpenShift.\n\n\
        Every parameter can also be set as an environment variable K8S_NETMEASURE_<FLAG> \
        (e.g. --timeout → K8S_NETMEASURE_TIMEOUT). Setting the same parameter on the \
        command line and in the environment is an error, as is an unknown \
        K8S_NETMEASURE_* variable.",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Subcommands,
}

#[derive(Debug, Subcommand)]
pub enum Subcommands {
    /// Measurement target: accepts TCP connections on the measured port and never
    /// reads or writes on them. Runs until SIGTERM or SIGINT.
    Listen(ListenArgs),
    /// Manual cross-check: a few connects with the measurement code path, one JSON
    /// line each. Not a measurement: no pacing, cycles or self-validation.
    Probe(ProbeArgs),
    /// Verifies on this kernel what the measurement relies on: SYN retransmission
    /// field, listener drop counter, SYN backoff, loopback behaviour, pacing, cgroup
    /// and signal isolation.
    Selftest(SelftestArgs),
}

impl Subcommands {
    /// The logging arguments every subcommand carries.
    pub fn log(&self) -> &LogArgs {
        match self {
            Subcommands::Listen(a) => &a.log,
            Subcommands::Probe(a) => &a.log,
            Subcommands::Selftest(a) => &a.log,
        }
    }
}

/// The full command tree with the given `--version` text.
pub fn command(version: &'static str) -> Command {
    Cli::command().version(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_argument_has_its_env_var() {
        let problems = surface::problems(&command("test"));
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn clap_definition_is_consistent() {
        command("test").debug_assert();
    }
}
