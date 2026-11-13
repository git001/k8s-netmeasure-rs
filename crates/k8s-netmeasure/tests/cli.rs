//! End-to-end checks through the real binary: exit codes (§8), `probe` (§3.9) and
//! `selftest` (§3.8). Configuration rules are in `config.rs`.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::net::TcpListener;

use common::{bin, closed_port, stderr, stdout};

#[test]
fn version_prints_build_info() {
    let out = bin().arg("--version").assert().success();
    let s = stdout(&out);
    assert!(
        s.starts_with("k8s-netmeasure ") && s.contains("(git ") && s.contains("rustc "),
        "{s}"
    );
}

#[test]
fn help_lists_every_subcommand() {
    let s = stdout(&bin().arg("--help").assert().success());
    assert!(
        s.contains("probe") && s.contains("selftest") && s.contains("K8S_NETMEASURE_"),
        "{s}"
    );
}

#[test]
fn usage_errors_exit_2() {
    bin().assert().code(2);
    bin().arg("--no-such-flag").assert().code(2);
    bin()
        .args(["probe", "--target", "[::1]:80"])
        .assert()
        .code(2);
    bin()
        .args(["probe", "--target", "127.0.0.1:1", "--count", "101"])
        .assert()
        .code(2);
}

#[test]
fn probe_ok_prints_one_json_line_per_connect() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = l.local_addr().unwrap().to_string();
    let out = bin()
        .args(["probe", "--target", &target, "--count", "3"])
        .assert()
        .code(0);
    let lines: Vec<serde_json::Value> = stdout(&out)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    for (i, v) in lines.iter().enumerate() {
        assert_eq!(v["seq"], i);
        assert_eq!(v["outcome"], "ok");
        assert_eq!(v["syn_retrans"], 0);
        assert!(v["duration_ns"].as_u64().unwrap() > 0);
    }
}

#[test]
fn probe_not_ok_exits_7() {
    let out = bin()
        .args(["probe", "--target", &closed_port()])
        .assert()
        .code(7);
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(v["outcome"], "refused");
    assert_eq!(v["errno"], 111);
}

#[test]
fn selftest_runs_every_check_and_writes_its_report() {
    let dir = std::env::temp_dir().join(format!("nm-selftest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("selftest.json");
    let out = bin().args(["selftest", "--output"]).arg(&file).assert();
    let code = out.get_output().status.code();
    assert!(
        matches!(code, Some(0) | Some(6)),
        "exit {code:?}: {}",
        stderr(&out)
    );
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let names: Vec<&str> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for n in [
        "syn-retrans-field",
        "listener-drop-indicator",
        "syn-backoff",
        "tcpi-total-rto",
        "loopback-connect",
        "pacing",
        "signal-isolation",
        "cgroup-v2",
    ] {
        assert!(names.contains(&n), "missing {n} in {names:?}");
    }
    assert_eq!(doc["config_source"]["output"], "cli");
    let table = stdout(&out);
    assert!(
        table.contains("signal-isolation") && table.contains("pacing"),
        "{table}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
