//! End-to-end tests of `run` (design §3.4, §3.5, §5, §8) against a real `listen`.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::time::{Duration, Instant};

use netmeasure_core::measure::record::flag;
use netmeasure_core::schema::result::{ResultFile, RunStatus};
use nix::sys::signal::Signal;

use common::listen::{start, stop_with};
use common::result::{out_path, read};
use common::{bin, closed_port, spawn};

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
        "--no-cgroup-check",
        "--no-target-control",
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
    assert!(r.run.config.no_cgroup_check);
    for c in &r.measurement.cycles {
        assert!(c.validity.valid, "{:?}", c.validity);
        assert!(
            c.validity
                .annotations
                .contains(&"cpu_throttle_unchecked".to_owned())
        );
        assert_eq!(c.quality.listener_drops.source, Some(0));
        assert!(
            c.validity
                .annotations
                .contains(&"target_unchecked".to_owned())
        );
    }
    assert_eq!(r.diagnostics.cycles.len(), r.measurement.cycles.len());
    assert!(
        std::fs::read_dir(out.parent().unwrap()).unwrap().count() == 1,
        "no temp file left"
    );
    stop_with(l.child, Signal::SIGTERM);
}

/// `run` against a fresh `listen`, stopped by SIGTERM after 800 ms; returns the exit
/// code, the log, the time from signal to exit and the result file.
fn stopped_run(name: &str, deadline: &str) -> (i32, String, Duration, ResultFile) {
    let l = start();
    let out = out_path(name);
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
        "--no-cgroup-check",
        "--target-control-port",
        common::listen::port(&l.control),
        "--shutdown-deadline",
        deadline,
    ]);
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let (code, log) = stop_with(child, Signal::SIGTERM);
    let took = started.elapsed();
    stop_with(l.child, Signal::SIGTERM);
    (code, log, took, read(&out))
}

#[test]
fn sigterm_past_the_deadline_interrupts_the_cycle() {
    let (code, log, took, r) = stopped_run("interrupted", "1ms");
    assert_eq!(code, 143, "{log}");
    assert!(took < Duration::from_secs(5));
    assert_eq!(r.run.status, RunStatus::Interrupted);
    assert_eq!(r.run.config.shutdown_deadline_ns, 1_000_000);
    let last = r.measurement.cycles.last().expect("at least one cycle");
    assert!(
        last.validity.reasons.contains(&"interrupted".to_owned()),
        "{:?}",
        last.validity
    );
    assert_eq!(r.measurement.samples.count, last.end_sample);
}

#[test]
fn sigterm_within_the_deadline_finishes_the_cycle() {
    let (code, log, _, r) = stopped_run("finished", "10s");
    assert_eq!(code, 143, "{log}");
    assert_eq!(
        r.run.status,
        RunStatus::Interrupted,
        "the run as a whole ended early"
    );
    let cycles = &r.measurement.cycles;
    assert!(
        cycles.len() < r.run.config.cycle_slots.len(),
        "no new cycle after the signal"
    );
    let last = cycles.last().unwrap();
    assert!(last.validity.valid, "{:?}", last.validity);
    let planned = r.run.config.cycle_slots[cycles.len() - 1];
    assert_eq!(
        last.end_sample - last.first_sample,
        planned,
        "the cycle was measured in full"
    );
}
