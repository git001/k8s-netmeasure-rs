//! End-to-end tests of `listen` (design §4, §8): start, `probe` against it, shutdown
//! by signal with `128 + signal`, refusal when the port is taken.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::net::TcpListener;

use nix::sys::signal::Signal;

use common::listen::{start, stop_with};
use common::{bin, stdout};

#[test]
fn probe_against_listen_is_ok_and_sigterm_exits_143() {
    let l = start();
    let out = bin()
        .args(["probe", "--target", &l.addr, "--count", "20"])
        .assert()
        .code(0);
    for line in stdout(&out).lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["outcome"], "ok", "{line}");
        assert_eq!(v["peer_closed"], false, "listen holds connections: {line}");
    }
    let (code, log) = stop_with(l.child, Signal::SIGTERM);
    assert_eq!(code, 143, "{log}");
    assert!(log.contains("\"message\":\"listening\""), "{log}");
    assert!(log.contains("\"message\":\"shut down\""), "{log}");
}

#[test]
fn sigint_exits_130() {
    let (code, log) = stop_with(start().child, Signal::SIGINT);
    assert_eq!(code, 130, "{log}");
}

#[test]
fn port_in_use_is_refused_with_exit_3() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    bin()
        .args([
            "listen",
            "--bind",
            "127.0.0.1",
            "--port",
            &port,
            "--control-bind",
            "127.0.0.1",
        ])
        .assert()
        .code(3);
}

#[test]
fn invalid_port_and_backlog_are_usage_errors() {
    let cases: [&[&str]; 6] = [
        &["listen", "--port", "0"],
        &["listen", "--backlog", "0"],
        &["listen", "--backlog", "65536"],
        &["listen", "--control-port", "0"],
        &["listen", "--control-bind", "::1"],
        &["listen", "--port", "17700", "--control-port", "17700"],
    ];
    for args in cases {
        bin().args(args).assert().code(2);
    }
}
