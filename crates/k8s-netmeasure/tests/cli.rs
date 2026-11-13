//! End-to-end checks of the CLI frame through the real binary.
#![forbid(clippy::too_many_arguments)]

use assert_cmd::Command;

fn bin() -> Command {
    Command::cargo_bin("k8s-netmeasure").expect("binary is built")
}

#[test]
fn version_prints_build_info() {
    let out = bin().arg("--version").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8");
    assert!(stdout.starts_with("k8s-netmeasure "), "{stdout}");
    assert!(stdout.contains("(git "), "{stdout}");
    assert!(stdout.contains("rustc "), "{stdout}");
}

#[test]
fn help_succeeds() {
    bin().arg("--help").assert().success();
}

#[test]
fn no_arguments_is_a_usage_error() {
    bin().assert().code(2);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    bin().arg("--no-such-flag").assert().code(2);
}
