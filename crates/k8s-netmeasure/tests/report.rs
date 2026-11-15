//! End-to-end tests of `report` (design §10.4–§10.6) on real result files.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::path::{Path, PathBuf};

use common::listen::{Running, start};
use common::{bin, stdout};

/// A short real run against the listener, written into `dir`.
fn run(l: &Running, dir: &Path, samples: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let control_port = l.control.rsplit(':').next().unwrap();
    bin()
        .args([
            "run",
            "--target",
            &l.addr,
            "--target-control-port",
            control_port,
        ])
        .args(["--samples", samples, "--warmup", "0"])
        .args([
            "--interval",
            "1ms",
            "--path",
            "underlay",
            "--node-name",
            "node-a",
        ])
        .args([
            "--no-cgroup-check",
            "--output",
            &format!("{}/", dir.display()),
        ])
        .assert()
        .code(0);
}

fn dirs(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("nm-report-{}-{name}", std::process::id()));
    (base.join("before"), base.join("after"))
}

#[test]
fn sequential_kernel_report_on_real_runs() {
    let l = start();
    let (before, after) = dirs("ok");
    run(&l, &before, "200");
    run(&l, &after, "200");
    let assert = bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args([
            "--before",
            before.to_str().unwrap(),
            "--after",
            after.to_str().unwrap(),
        ])
        .args([
            "--equivalence-margin",
            "20%",
            "--equivalence-rationale",
            "test",
        ])
        .args(["--replicates", "1000", "--seed", "1", "--primary", "p90"])
        .assert()
        .code(0);
    let out = stdout(&assert);
    for want in [
        "design: sequential",
        "== path underlay ==",
        "evidence: exploratory",
        "p90",
        "local control (loopback median)",
        "the declared factor did not change",
        "seed 1",
    ] {
        assert!(out.contains(want), "missing `{want}` in:\n{out}");
    }
}

#[test]
fn overlapping_sides_and_usage_errors_are_refused() {
    let l = start();
    let (before, _) = dirs("refused");
    // Longer than the 1 s overlap a clock step may cause.
    run(&l, &before, "1500");
    let b = before.to_str().unwrap();
    // The same runs on both sides overlap completely: not sequential (exit 5).
    bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args(["--before", b, "--after", b, "--replicates", "1000"])
        .assert()
        .code(5);
    // --design is required; a margin needs its rationale (exit 2).
    bin()
        .args(["report", "--factor", "kernel", "--before", b, "--after", b])
        .assert()
        .code(2);
    bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args(["--before", b, "--after", b, "--equivalence-margin", "10%"])
        .assert()
        .code(2);
    bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args(["--before", "/nonexistent-nm", "--after", b])
        .assert()
        .code(2);
}
