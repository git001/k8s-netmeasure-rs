//! End-to-end tests of `listen` (design §4, §8): start, `probe` against it, shutdown
//! by signal with `128 + signal`, refusal when the port is taken.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::net::{TcpListener, TcpStream};
use std::process::Child;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use common::{bin, closed_port, spawn, stdout};

/// Starts `listen` on a free loopback port and waits until it accepts.
fn start() -> (Child, String) {
    let addr = closed_port();
    let port = addr.rsplit(':').next().unwrap().to_owned();
    let child = spawn(&["listen", "--bind", "127.0.0.1", "--port", &port]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(&addr).is_err() {
        assert!(
            Instant::now() < deadline,
            "listen did not come up on {addr}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    (child, addr)
}

fn stop_with(mut child: Child, signal: Signal) -> (i32, String) {
    kill(Pid::from_raw(child.id() as i32), signal).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let out = child.wait_with_output().unwrap();
            let log =
                String::from_utf8(out.stdout).unwrap() + &String::from_utf8(out.stderr).unwrap();
            return (status.code().expect("exit code, not a signal"), log);
        }
        assert!(Instant::now() < deadline, "listen did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn probe_against_listen_is_ok_and_sigterm_exits_143() {
    let (child, addr) = start();
    let out = bin()
        .args(["probe", "--target", &addr, "--count", "20"])
        .assert()
        .code(0);
    for line in stdout(&out).lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["outcome"], "ok", "{line}");
        assert_eq!(v["peer_closed"], false, "listen holds connections: {line}");
    }
    let (code, log) = stop_with(child, Signal::SIGTERM);
    assert_eq!(code, 143, "{log}");
    assert!(log.contains("\"message\":\"listening\""), "{log}");
    assert!(log.contains("\"message\":\"shut down\""), "{log}");
}

#[test]
fn sigint_exits_130() {
    let (child, _) = start();
    let (code, log) = stop_with(child, Signal::SIGINT);
    assert_eq!(code, 130, "{log}");
}

#[test]
fn port_in_use_is_refused_with_exit_3() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    bin()
        .args(["listen", "--bind", "127.0.0.1", "--port", &port])
        .assert()
        .code(3);
}

#[test]
fn invalid_port_and_backlog_are_usage_errors() {
    for args in [
        ["listen", "--port", "0"],
        ["listen", "--backlog", "0"],
        ["listen", "--backlog", "65536"],
    ] {
        bin().args(args).assert().code(2);
    }
}
