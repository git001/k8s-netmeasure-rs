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

/// A result file is never overwritten; a directory gets a unique generated name.
#[test]
fn output_directory_names_and_existing_file_refusal() {
    let out = out_path("dir");
    let dir = out.parent().unwrap();
    let quick = |output: &str| {
        bin()
            .args(["run", "--target", &closed_port(), "--samples", "20"])
            .args(["--warmup", "0", "--interval", "1ms", "--path", "underlay"])
            .args(["--node-name", "node-a", "--no-cgroup-check"])
            .args(["--no-target-control", "--output", output])
            .assert()
    };
    let d = format!("{}/", dir.display());
    quick(&d).code(0);
    quick(&d).code(0);
    let names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "two runs, two files: {names:?}");
    for n in &names {
        assert!(
            n.starts_with("underlay-node-a-") && n.ends_with(".json"),
            "{n}"
        );
        let r = read(&dir.join(n));
        assert!(n.ends_with(&format!("-{}.json", r.run.run_id)), "{n}");
    }
    let existing = dir.join(&names[0]);
    let before = std::fs::read(&existing).unwrap();
    let assert = quick(existing.to_str().unwrap()).code(3);
    assert!(common::stderr(&assert).contains("never overwritten"));
    assert_eq!(std::fs::read(&existing).unwrap(), before);
}
