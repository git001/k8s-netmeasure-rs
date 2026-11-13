//! clap command definitions (design §2, §9.1).
//!
//! This crate exists only because clap's derive output allows the whole clippy
//! `complexity` group, which is a hard error (E0453) under
//! `#![forbid(clippy::too_many_arguments)]`. The CLI types therefore live here, under
//! `deny` instead of `forbid`; `cargo xtask line-limit` still rejects every
//! hand-written `allow` of the lint or of its groups. Parsing (reading argv and the
//! environment) happens in the binary.
#![forbid(unsafe_code)]
#![deny(clippy::too_many_arguments)]

use clap::{Command, CommandFactory, Parser};

/// Environment variable prefix for every parameter (design §9.1).
pub const ENV_PREFIX: &str = "K8S_NETMEASURE_";

#[derive(Debug, Parser)]
#[command(
    name = "k8s-netmeasure",
    about = "Reproducible TCP connect-latency measurement in Kubernetes and OpenShift",
    arg_required_else_help = true
)]
pub struct Cli {}

/// The full command tree with the given `--version` text.
pub fn command(version: &'static str) -> Command {
    Cli::command().version(version)
}

/// Expected env var name for a long flag: `--node-context-from` →
/// `K8S_NETMEASURE_NODE_CONTEXT_FROM`.
pub fn env_name(long: &str) -> String {
    format!(
        "{ENV_PREFIX}{}",
        long.replace('-', "_").to_ascii_uppercase()
    )
}

/// Config-surface rule (design §9.1, §12): every argument except `--help`/`--version`
/// has a long flag and exactly the derived env var, and no subcommand defines an
/// argument twice. Returns one message per violation.
pub fn surface_problems(cmd: &Command) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for arg in cmd.get_arguments() {
        let id = arg.get_id().as_str();
        if id == "help" || id == "version" {
            continue;
        }
        if !seen.insert(id.to_owned()) {
            problems.push(format!("{}: duplicate argument {id}", cmd.get_name()));
        }
        let Some(long) = arg.get_long() else {
            problems.push(format!("{}: {id} has no long flag", cmd.get_name()));
            continue;
        };
        let expected = env_name(long);
        match arg.get_env() {
            Some(env) if env.to_str() == Some(expected.as_str()) => {}
            Some(env) => problems.push(format!(
                "{}: --{long} has env {env:?}, expected {expected}",
                cmd.get_name()
            )),
            None => problems.push(format!("{}: --{long} has no env var", cmd.get_name())),
        }
    }
    for sub in cmd.get_subcommands() {
        problems.extend(surface_problems(sub));
    }
    problems
}

#[cfg(test)]
mod tests {
    use clap::Arg;

    use super::*;

    #[test]
    fn every_argument_has_its_env_var() {
        let problems = surface_problems(&command("test"));
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn clap_definition_is_consistent() {
        command("test").debug_assert();
    }

    #[test]
    fn env_name_follows_the_derivation_rule() {
        assert_eq!(
            env_name("node-context-from"),
            "K8S_NETMEASURE_NODE_CONTEXT_FROM"
        );
    }

    #[test]
    fn surface_check_reports_every_kind_of_violation() {
        let cmd = Command::new("x")
            .arg(Arg::new("a").long("alpha"))
            .arg(Arg::new("b").long("beta").env("WRONG"))
            .arg(Arg::new("c").short('c'))
            .arg(Arg::new("d").long("delta-x").env("K8S_NETMEASURE_DELTA_X"))
            .subcommand(Command::new("sub").arg(Arg::new("e").long("e")));
        let problems = surface_problems(&cmd);
        assert_eq!(problems.len(), 4, "{problems:#?}");
        assert!(problems[0].contains("--alpha has no env var"));
        assert!(problems[1].contains("expected K8S_NETMEASURE_BETA"));
        assert!(problems[2].contains("c has no long flag"));
        assert!(problems[3].starts_with("sub: --e"));
    }
}
