//! `--log-level`, shared by every subcommand (design §9.3). Flattened into each
//! subcommand's arguments, so the env rules of §9.1 apply unchanged.

use clap::{Args, ValueEnum};

/// Minimum level of the JSON log lines on stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Args)]
pub struct LogArgs {
    /// Minimum level of the JSON log lines on stderr. `debug` adds details for
    /// development and support cases; stdout (results) is unaffected.
    #[arg(
        long,
        env = "K8S_NETMEASURE_LOG_LEVEL",
        value_enum,
        default_value_t = LogLevel::Info
    )]
    pub log_level: LogLevel,
}
