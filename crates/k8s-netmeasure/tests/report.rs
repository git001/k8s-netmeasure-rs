//! End-to-end tests of `report` (design §10.4–§10.6) on real result files.
#![forbid(clippy::too_many_arguments)]

mod common;

use common::listen::start;
use common::reports::{dirs, run, run_at};
use common::{bin, stdout};

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
        "NIC counters of the sources (diagnostics",
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
    // The same run twice on one side (e.g. from two archives) is a usage error.
    let file = std::fs::read_dir(&before)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let copy = before.with_file_name("copy");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::copy(&file, copy.join("again.json")).unwrap();
    let assert = bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args([
            "--before",
            b,
            "--before",
            copy.to_str().unwrap(),
            "--after",
            b,
        ])
        .assert()
        .code(2);
    let err = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(err.contains("given twice"), "{err}");
}

#[test]
fn different_load_parameters_are_never_pooled() {
    let l = start();
    let (before, after) = dirs("params");
    run_at(&l, &before, "200", "1ms");
    run_at(&l, &after, "200", "2ms");
    let assert = bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args([
            "--before",
            before.to_str().unwrap(),
            "--after",
            after.to_str().unwrap(),
        ])
        .args(["--replicates", "1000"])
        .assert()
        .code(5);
    let err = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        err.contains("never pooled") && err.contains("interval 2 ms"),
        "{err}"
    );
}
