//! Environment rules of design §9.1, checked after clap has parsed:
//! - a `K8S_NETMEASURE_*` variable that belongs to no subcommand is an error;
//! - one that belongs only to other subcommands is ignored and reported;
//! - an empty value is an error, not "unset";
//! - a parameter given on the command line *and* in the environment is an error.
//!   clap's `value_source()` reports only the winner, so the environment is passed in
//!   and compared here.
//!
//! Also yields the provenance (`cli` / `env` / `default`) of every parameter.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;

use clap::parser::ValueSource;
use clap::{ArgMatches, Command};

use crate::ENV_PREFIX;
use crate::surface::is_builtin;

/// Outcome of a successful check.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnvReport {
    /// Variables of other subcommands: `(variable, owning subcommands)`.
    pub ignored: Vec<(String, Vec<String>)>,
    /// Source per parameter of the active subcommand: `cli`, `env` or `default`.
    pub sources: BTreeMap<String, &'static str>,
}

/// Checks the environment against the command tree and the parse result.
pub fn check(
    cmd: &Command,
    matches: &ArgMatches,
    env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<EnvReport, Vec<String>> {
    let owners = env_owners(cmd);
    let (active, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| vec!["no subcommand given".to_owned()])?;
    let active_cmd = cmd
        .find_subcommand(active)
        .ok_or_else(|| vec![format!("unknown subcommand {active}")])?;

    let mut errors = Vec::new();
    let mut report = EnvReport::default();
    let mut present = BTreeSet::new();
    for (key, value) in env {
        let Some(key) = key.to_str().map(str::to_owned) else {
            continue; // not UTF-8, cannot carry our prefix
        };
        if !key.starts_with(ENV_PREFIX) {
            continue;
        }
        if value.is_empty() {
            errors.push(format!("{key} is set but empty; unset it instead"));
        }
        match owners.get(&key) {
            None => errors.push(format!("{key} is not a parameter of any subcommand")),
            Some(subs) if !subs.contains(active) => {
                report
                    .ignored
                    .push((key.clone(), subs.iter().cloned().collect()));
            }
            Some(_) => {}
        }
        present.insert(key);
    }

    for arg in active_cmd.get_arguments() {
        let id = arg.get_id().as_str();
        if is_builtin(id) {
            continue;
        }
        let env_var = arg.get_env().and_then(|e| e.to_str()).unwrap_or_default();
        let source = match sub_matches.value_source(id) {
            Some(ValueSource::CommandLine) if present.contains(env_var) => {
                let long = arg.get_long().unwrap_or(id);
                errors.push(format!(
                    "--{long} is given on the command line and as {env_var}"
                ));
                "cli"
            }
            Some(ValueSource::CommandLine) => "cli",
            Some(ValueSource::EnvVariable) => "env",
            _ => "default",
        };
        report.sources.insert(id.to_owned(), source);
    }

    if errors.is_empty() {
        Ok(report)
    } else {
        Err(errors)
    }
}

/// Env var name → names of the subcommands that define it.
fn env_owners(cmd: &Command) -> BTreeMap<String, BTreeSet<String>> {
    let mut owners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for sub in cmd.get_subcommands() {
        for arg in sub.get_arguments() {
            if let Some(env) = arg.get_env().and_then(|e| e.to_str()) {
                owners
                    .entry(env.to_owned())
                    .or_default()
                    .insert(sub.get_name().to_owned());
            }
        }
    }
    owners
}

#[cfg(test)]
mod tests {
    use clap::Arg;

    use super::*;

    fn cmd() -> Command {
        Command::new("t")
            .subcommand(
                Command::new("probe")
                    .arg(
                        Arg::new("target")
                            .long("target")
                            .env("K8S_NETMEASURE_TARGET"),
                    )
                    .arg(
                        Arg::new("count")
                            .long("count")
                            .env("K8S_NETMEASURE_COUNT")
                            .default_value("1"),
                    ),
            )
            .subcommand(
                Command::new("run")
                    .arg(
                        Arg::new("target")
                            .long("target")
                            .env("K8S_NETMEASURE_TARGET"),
                    )
                    .arg(
                        Arg::new("samples")
                            .long("samples")
                            .env("K8S_NETMEASURE_SAMPLES"),
                    ),
            )
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    fn run(args: &[&str], vars: &[(&str, &str)]) -> Result<EnvReport, Vec<String>> {
        let cmd = cmd();
        let matches = cmd.clone().try_get_matches_from(args).expect("parses");
        check(&cmd, &matches, env(vars))
    }

    #[test]
    fn sources_and_unrelated_variables() {
        let report = run(&["t", "probe", "--target", "a"], &[("HOME", "/x")]).unwrap();
        assert_eq!(report.sources["target"], "cli");
        assert_eq!(report.sources["count"], "default");
        assert!(report.ignored.is_empty());
    }

    #[test]
    fn variable_of_another_subcommand_is_ignored_and_reported() {
        let report = run(
            &["t", "probe", "--target", "a"],
            &[("K8S_NETMEASURE_SAMPLES", "5")],
        );
        let report = report.unwrap();
        assert_eq!(
            report.ignored,
            vec![("K8S_NETMEASURE_SAMPLES".to_owned(), vec!["run".to_owned()])]
        );
    }

    #[test]
    fn unknown_empty_and_duplicate_are_errors() {
        let errors = run(
            &["t", "probe", "--target", "a"],
            &[
                ("K8S_NETMEASURE_SAMPELS", "5"),
                ("K8S_NETMEASURE_COUNT", ""),
                ("K8S_NETMEASURE_TARGET", "b"),
            ],
        )
        .unwrap_err();
        assert_eq!(errors.len(), 3, "{errors:#?}");
        assert!(errors[0].contains("K8S_NETMEASURE_SAMPELS is not a parameter"));
        assert!(errors[1].contains("K8S_NETMEASURE_COUNT is set but empty"));
        assert!(errors[2].contains("--target is given on the command line and as"));
    }

    #[test]
    fn missing_subcommand_is_an_error() {
        let cmd = cmd();
        let matches = cmd.clone().try_get_matches_from(["t"]).unwrap();
        assert!(check(&cmd, &matches, env(&[])).is_err());
    }
}
