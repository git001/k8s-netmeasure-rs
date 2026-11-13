//! End-to-end checks of the configuration surface: the env rules (§9.1) and
//! `--log-level` (§9.3).
#![forbid(clippy::too_many_arguments)]

mod common;

use common::{bin, closed_port, stderr};

#[test]
fn log_level_defaults_to_info_and_is_settable() {
    let debug_line = "\"level\":\"DEBUG\"";
    let default = bin()
        .args(["probe", "--target", &closed_port()])
        .assert()
        .code(7);
    assert!(
        !stderr(&default).contains(debug_line),
        "{}",
        stderr(&default)
    );
    let flag = bin()
        .args(["probe", "--log-level", "debug", "--target", &closed_port()])
        .assert()
        .code(7);
    assert!(
        stderr(&flag).contains("parameter source"),
        "{}",
        stderr(&flag)
    );
    let env = bin()
        .env("K8S_NETMEASURE_LOG_LEVEL", "debug")
        .args(["probe", "--target", &closed_port()])
        .assert()
        .code(7);
    assert!(stderr(&env).contains(debug_line), "{}", stderr(&env));
    bin()
        .args([
            "probe",
            "--log-level",
            "verbose",
            "--target",
            &closed_port(),
        ])
        .assert()
        .code(2);
}

#[test]
fn env_is_equivalent_to_the_flag() {
    bin()
        .arg("probe")
        .env("K8S_NETMEASURE_TARGET", closed_port())
        .assert()
        .code(7);
}

#[test]
fn same_parameter_on_cli_and_env_is_an_error() {
    let out = bin()
        .args(["probe", "--target", &closed_port()])
        .env("K8S_NETMEASURE_TARGET", closed_port())
        .assert()
        .code(2);
    assert!(
        stderr(&out).contains("--target is given on the command line and as K8S_NETMEASURE_TARGET")
    );
}

#[test]
fn unknown_and_empty_variables_are_errors() {
    let out = bin()
        .args(["probe", "--target", &closed_port()])
        .env("K8S_NETMEASURE_TRAGET", "x")
        // Not read by clap for `probe`, so this exercises our own empty-value rule;
        // an empty `--count` is already rejected by clap itself (also exit 2).
        .env("K8S_NETMEASURE_OUTPUT", "")
        .assert()
        .code(2);
    let e = stderr(&out);
    assert!(
        e.contains("K8S_NETMEASURE_TRAGET is not a parameter"),
        "{e}"
    );
    assert!(e.contains("K8S_NETMEASURE_OUTPUT is set but empty"), "{e}");
    bin()
        .args(["probe", "--target", "127.0.0.1:1"])
        .env("K8S_NETMEASURE_COUNT", "")
        .assert()
        .code(2);
}

#[test]
fn variable_of_another_subcommand_is_ignored_and_logged() {
    let out = bin()
        .args(["probe", "--target", &closed_port()])
        .env("K8S_NETMEASURE_OUTPUT", "/nonexistent/x.json")
        .assert()
        .code(7);
    let e = stderr(&out);
    assert!(
        e.contains("\"variable\":\"K8S_NETMEASURE_OUTPUT\"")
            && e.contains("\"belongs_to\":\"selftest\""),
        "{e}"
    );
}
