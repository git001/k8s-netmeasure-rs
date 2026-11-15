//! End-to-end tests of `run`'s refusals (design §3.7, §8, §9): configuration errors
//! (exit 2) and the start checks (exit 3).
#![forbid(clippy::too_many_arguments)]

mod common;

use common::result::{out_path, read};
use common::{bin, closed_port};

#[test]
fn configuration_errors() {
    let out = out_path("errors");
    let o = out.to_str().unwrap();
    for args in [
        &[
            "run",
            "--target",
            "127.0.0.1:9",
            "--output",
            o,
            "--label",
            "path=x",
        ][..],
        &[
            "run",
            "--target",
            "127.0.0.1:9",
            "--output",
            o,
            "--cycles",
            "3",
            "--cycle",
            "5s",
        ],
        &[
            "run",
            "--target",
            "127.0.0.1:9",
            "--output",
            o,
            "--loopback-every",
            "1",
        ],
        &["run", "--output", o],
    ] {
        bin().args(args).assert().code(2);
    }
    bin()
        .args([
            "run",
            "--target",
            "127.0.0.1:9",
            "--output",
            "/nonexistent/dir/r.json",
        ])
        .assert()
        .code(3);
}

#[test]
fn unreadable_cgroup_refuses_without_the_flag() {
    let out = out_path("cgroup");
    let o = out.to_str().unwrap();
    let assert = bin()
        .args([
            "run",
            "--target",
            &closed_port(),
            "--samples",
            "20",
            "--warmup",
            "0",
            // Only the cgroup check is under test; where the cgroup is readable (CI),
            // the target control port check would refuse the closed port next.
            "--no-target-control",
        ])
        .args(["--interval", "1ms", "--output", o])
        .assert();
    let code = assert.get_output().status.code();
    let log = common::stderr(&assert);
    match code {
        Some(3) => assert!(log.contains("CPU check failed"), "{log}"),
        Some(0) => assert!(!read(&out).run.config.no_cgroup_check),
        other => panic!("exit {other:?}: {log}"),
    }
}
