//! End-to-end tests of `run` against targets with a control port (design §3.6, §7):
//! identity and context at start, per-cycle target counters, `target_set_changed`.
#![forbid(clippy::too_many_arguments)]

mod common;

use nix::sys::signal::Signal;

use common::bin;
use common::listen::{port, start, stop_with};
use common::result::{out_path, read};

fn run_args<'a>(addr: &'a str, control: &'a str, out: &'a str, samples: &'a str) -> Vec<&'a str> {
    vec![
        "run",
        "--target",
        addr,
        "--target-control-port",
        port(control),
        "--samples",
        samples,
        "--warmup",
        "0",
        "--interval",
        "1ms",
        "--output",
        out,
        "--no-cgroup-check",
    ]
}

#[test]
fn target_counters_and_context_are_recorded() {
    let l = start();
    let out = out_path("targets");
    let o = out.to_str().unwrap();
    bin()
        .args(run_args(&l.addr, &l.control, o, "200"))
        .assert()
        .code(0);
    let r = read(&out);
    let t = &r.targets[1];
    assert_eq!(t.control.as_deref(), Some(l.control.as_str()));
    assert_eq!(t.listener_instance.as_ref().map(String::len), Some(26));
    assert!(t.context_before.is_some() && t.context_after.is_some());
    assert_eq!(
        r.run.config.target_control_port,
        port(&l.control).parse().ok()
    );
    let accepted: u64 = r
        .diagnostics
        .cycles
        .iter()
        .map(|c| c.targets["1"].accepted.expect("delta known"))
        .sum();
    assert!(accepted >= 200, "the target saw every connect: {accepted}");
    for c in &r.measurement.cycles {
        assert!(c.validity.valid, "{:?}", c.validity);
        assert_eq!(c.quality.listener_drops.targets["1"], Some(0));
    }
    stop_with(l.child, Signal::SIGTERM);
}

#[test]
fn target_gone_mid_run_is_target_set_changed() {
    let l = start();
    let out = out_path("gone");
    let o = out.to_str().unwrap().to_owned();
    let (addr, control) = (l.addr.clone(), l.control.clone());
    let runner = std::thread::spawn(move || {
        bin()
            .args(run_args(&addr, &control, &o, "6000"))
            .args(["--cycles", "3"])
            .assert()
            .code(4)
    });
    std::thread::sleep(std::time::Duration::from_millis(1500));
    stop_with(l.child, Signal::SIGTERM);
    runner.join().unwrap();
    let r = read(&out);
    let reasons: Vec<&String> = r
        .measurement
        .cycles
        .iter()
        .flat_map(|c| &c.validity.reasons)
        .collect();
    assert!(
        reasons.iter().any(|x| *x == "target_set_changed"),
        "{reasons:?}"
    );
    assert!(
        r.targets[1].context_after.is_none(),
        "unreachable at the end"
    );
}

#[test]
fn missing_control_port_refuses_the_start() {
    let l = start();
    let out = out_path("nocontrol");
    let free = common::closed_port();
    bin()
        .args(run_args(&l.addr, &free, out.to_str().unwrap(), "10"))
        .assert()
        .code(3);
    stop_with(l.child, Signal::SIGTERM);
}
