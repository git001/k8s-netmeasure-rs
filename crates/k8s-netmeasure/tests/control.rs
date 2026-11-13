//! End-to-end tests of the control port of `listen` (design §7): probes, `/v1/info`,
//! strictness, and `/ready` during shutdown.
#![forbid(clippy::too_many_arguments)]

mod common;

use nix::sys::signal::Signal;

use common::listen::{get, request, start, stop_with};

#[test]
fn probes_and_info() {
    let l = start();
    for path in ["/startup", "/live", "/ready"] {
        assert_eq!(get(&l.control, path).unwrap().0, 200, "{path}");
    }
    let (code, info) = get(&l.control, "/v1/info").unwrap();
    assert_eq!(code, 200);
    assert_eq!(info["schema"], "k8s-netmeasure/control");
    assert_eq!(info["measured"], l.addr);
    assert_eq!(
        info["listener_instance"].as_str().unwrap().len(),
        26,
        "ULID"
    );
    assert!(info["build_info"]["version"].is_string());
    let (code, log) = stop_with(l.child, Signal::SIGTERM);
    assert_eq!(code, 143, "{log}");
}

#[test]
fn unknown_path_method_and_query_are_rejected() {
    let l = start();
    assert_eq!(get(&l.control, "/metrics").unwrap().0, 404);
    assert_eq!(request(&l.control, "POST", "/ready").unwrap().0, 405);
    assert_eq!(get(&l.control, "/v1/info?verbose=1").unwrap().0, 400);
    stop_with(l.child, Signal::SIGTERM);
}
