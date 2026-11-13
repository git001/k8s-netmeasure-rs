//! Config-surface rule (design §9.1, §12): every argument except `--help`/`--version`
//! has a long flag and exactly the derived env var, and no subcommand defines an
//! argument twice.

use std::collections::BTreeSet;

use clap::Command;

use crate::ENV_PREFIX;

/// Expected env var name for a long flag: `--node-context-from` →
/// `K8S_NETMEASURE_NODE_CONTEXT_FROM`.
pub fn env_name(long: &str) -> String {
    format!(
        "{ENV_PREFIX}{}",
        long.replace('-', "_").to_ascii_uppercase()
    )
}

/// `true` for clap's built-in `--help` / `--version`.
pub fn is_builtin(id: &str) -> bool {
    id == "help" || id == "version"
}

/// One message per violation, for the whole command tree.
pub fn problems(cmd: &Command) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for arg in cmd.get_arguments() {
        let id = arg.get_id().as_str();
        if is_builtin(id) {
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
        problems.extend(self::problems(sub));
    }
    problems
}

#[cfg(test)]
mod tests {
    use clap::Arg;

    use super::*;

    #[test]
    fn env_name_follows_the_derivation_rule() {
        assert_eq!(
            env_name("node-context-from"),
            "K8S_NETMEASURE_NODE_CONTEXT_FROM"
        );
    }

    #[test]
    fn reports_every_kind_of_violation() {
        let cmd = Command::new("x")
            .arg(Arg::new("a").long("alpha"))
            .arg(Arg::new("b").long("beta").env("WRONG"))
            .arg(Arg::new("c").short('c'))
            .arg(Arg::new("d").long("delta-x").env("K8S_NETMEASURE_DELTA_X"))
            .subcommand(Command::new("sub").arg(Arg::new("e").long("e")));
        let problems = problems(&cmd);
        assert_eq!(problems.len(), 4, "{problems:#?}");
        assert!(problems[0].contains("--alpha has no env var"));
        assert!(problems[1].contains("expected K8S_NETMEASURE_BETA"));
        assert!(problems[2].contains("c has no long flag"));
        assert!(problems[3].starts_with("sub: --e"));
    }
}
