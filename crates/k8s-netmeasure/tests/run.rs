//! End-to-end tests of `run` (design §3.4, §3.5, §5, §8) against a real `listen`.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::time::{Duration, Instant};

use netmeasure_core::measure::record::flag;
use netmeasure_core::schema::result::{ResultFile, RunStatus};
use nix::sys::signal::Signal;

use common::listen::{start, stop_with};
use common::{bin, closed_port, spawn};

fn out_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nm-run-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("result.json")
}

fn read(path: &std::path::Path) -> ResultFile {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).expect("result file matches the schema strictly")
}

#[test]
fn complete_run_counts_samples_per_target() {
    let l = start();
    let out = out_path("complete");
    let unreachable = closed_port();
    let args = [
        "run",
        "--target",
        &l.addr,
        "--target",
        &unreachable,
        "--samples",
        "60",
        "--warmup",
        "5",
        "--interval",
        "1ms",
        "--loopback-every",
        "4",
        "--output",
        out.to_str().unwrap(),
        "--label",
        "phase=test",
        "--path",
        "overlay",
    ];
    bin().args(args).assert().code(0);
    let r = read(&out);
    assert_eq!(r.run.status, RunStatus::Complete);
    assert_eq!(r.run.labels["phase"], "test");
    assert_eq!(
        (
            r.run.config.topology.as_str(),
            r.run.config.path_source.as_str()
        ),
        ("sampled", "declared")
    );
    let s = &r.measurement.samples;
    assert_eq!(s.count, r.run.config.cycle_slots.iter().sum::<u64>());
    for target in [1u16, 2] {
        let mine: Vec<usize> = (0..s.target.len())
            .filter(|&i| s.target[i] == target)
            .collect();
        let counted = mine
            .iter()
            .filter(|&&i| s.flags[i] & flag::WARMUP == 0)
            .count();
        assert_eq!((mine.len(), counted), (65, 60), "target {target}");
    }
    let refused = (0..s.target.len())
        .filter(|&i| s.target[i] == 2)
        .all(|i| s.result[i] == 2);
    assert!(
        refused,
        "the closed port is `refused`, which does not invalidate a cycle"
    );
    assert!(r.measurement.cycles.iter().all(|c| c.validity.valid));
    assert!(
        std::fs::read_dir(out.parent().unwrap()).unwrap().count() == 1,
        "no temp file left"
    );
    stop_with(l.child, Signal::SIGTERM);
}

#[test]
fn sigterm_writes_an_interrupted_file_and_exits_143() {
    let l = start();
    let out = out_path("interrupted");
    let child = spawn(&[
        "run",
        "--target",
        &l.addr,
        "--samples",
        "5000",
        "--interval",
        "2ms",
        "--output",
        out.to_str().unwrap(),
    ]);
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let (code, log) = stop_with(child, Signal::SIGTERM);
    assert_eq!(code, 143, "{log}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let r = read(&out);
    assert_eq!(r.run.status, RunStatus::Interrupted);
    let last = r.measurement.cycles.last().expect("at least one cycle");
    assert!(
        last.validity.reasons.contains(&"interrupted".to_owned()),
        "{:?}",
        last.validity
    );
    assert_eq!(r.measurement.samples.count, last.end_sample);
    stop_with(l.child, Signal::SIGTERM);
}

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
