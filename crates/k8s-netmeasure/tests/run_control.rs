//! End-to-end tests of `run --control-port` (design §4.1, §7): probes while measuring,
//! `/v1/info` carries the `run_id`, a taken port refuses the start.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::time::{Duration, Instant};

use nix::sys::signal::Signal;

use common::listen::{get, port, start, stop_with};
use common::result::{out_path, read};
use common::{bin, closed_port};

#[test]
fn probes_answer_while_measuring() {
    let l = start();
    let out = out_path("control");
    let control = closed_port();
    let (addr, lc, o, c) = (
        l.addr.clone(),
        l.control.clone(),
        out.to_str().unwrap().to_owned(),
        control.clone(),
    );
    let runner = std::thread::spawn(move || {
        bin()
            .args([
                "run",
                "--target",
                &addr,
                "--target-control-port",
                port(&lc),
                "--samples",
                "1500",
            ])
            .args(["--interval", "2ms", "--no-cgroup-check", "--output", &o])
            .args(["--control-port", port(&c), "--control-bind", "127.0.0.1"])
            .assert()
            .code(0)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while get(&control, "/ready").map(|(c, _)| c) != Some(200) {
        assert!(
            Instant::now() < deadline,
            "run's control port did not become ready"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(get(&control, "/live").unwrap().0, 200);
    let (_, info) = get(&control, "/v1/info").unwrap();
    runner.join().unwrap();
    let r = read(&out);
    assert_eq!(
        info["listener_instance"].as_str(),
        Some(r.run.run_id.as_str())
    );
    assert_eq!(r.run.config.control.as_deref(), Some(control.as_str()));
    stop_with(l.child, Signal::SIGTERM);
}

#[test]
fn taken_control_port_refuses_the_start() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p = taken.local_addr().unwrap().port().to_string();
    let out = out_path("control-taken");
    bin()
        .args([
            "run",
            "--target",
            "127.0.0.1:9",
            "--no-target-control",
            "--no-cgroup-check",
        ])
        .args([
            "--control-port",
            &p,
            "--control-bind",
            "127.0.0.1",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(3);
}
