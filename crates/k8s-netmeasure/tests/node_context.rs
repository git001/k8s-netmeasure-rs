//! End-to-end tests of `--node-context-from` (design §5): a local `listen` that sees
//! the physical interfaces stands in for the underlay node listener. Skipped where no
//! physical interface is visible (some CI sandboxes).
#![forbid(clippy::too_many_arguments)]

mod common;

use netmeasure_core::schema::control::NodeContext;
use netmeasure_core::schema::sourced::Source;
use nix::sys::signal::Signal;

use common::bin;
use common::listen::{get, port, start, start_with, stop_with};
use common::result::{out_path, read};

fn physical_nic_visible() -> bool {
    let visible = std::fs::read_dir("/sys/class/net")
        .map(|d| d.flatten().any(|e| e.path().join("device").exists()))
        .unwrap_or(false);
    if !visible {
        eprintln!("skipped: no physical interface visible");
    }
    visible
}

#[test]
fn listen_passes_the_node_nic_path_on() {
    if !physical_nic_visible() {
        return;
    }
    let node = start();
    let overlay = start_with(&["--node-context-from", &node.control]);
    let (_, body) = get(&overlay.control, "/v1/context").unwrap();
    let ctx: NodeContext = serde_json::from_value(body).unwrap();
    assert_eq!(ctx.nic.source, Source::ObservedViaNodeListener);
    let (_, snap) = get(&overlay.control, "/v1/snapshot").unwrap();
    assert_eq!(snap["nic"]["source"], "observed_via_node_listener");
    stop_with(overlay.child, Signal::SIGTERM);
    stop_with(node.child, Signal::SIGTERM);
}

#[test]
fn run_uses_the_node_listener_and_annotates_its_loss() {
    if !physical_nic_visible() {
        return;
    }
    let node = start();
    let target = start();
    let out = out_path("node");
    let o = out.to_str().unwrap().to_owned();
    let (addr, control, node_control) = (
        target.addr.clone(),
        target.control.clone(),
        node.control.clone(),
    );
    let runner = std::thread::spawn(move || {
        bin()
            .args([
                "run",
                "--target",
                &addr,
                "--target-control-port",
                port(&control),
            ])
            .args([
                "--node-context-from",
                &node_control,
                "--samples",
                "6000",
                "--warmup",
                "0",
            ])
            .args([
                "--interval",
                "1ms",
                "--cycles",
                "3",
                "--no-cgroup-check",
                "--output",
                &o,
            ])
            .assert()
            .code(0)
    });
    std::thread::sleep(std::time::Duration::from_millis(1500));
    stop_with(node.child, Signal::SIGTERM);
    runner.join().unwrap();
    let r = read(&out);
    assert_eq!(
        r.source.context_before.nic.source,
        Source::ObservedViaNodeListener
    );
    let notes: Vec<&String> = r
        .measurement
        .cycles
        .iter()
        .flat_map(|c| &c.validity.annotations)
        .collect();
    assert!(
        notes.iter().any(|n| *n == "node_context_unavailable"),
        "{notes:?}"
    );
    assert_eq!(r.source.context_after.unwrap().nic.source, Source::Unknown);
    stop_with(target.child, Signal::SIGTERM);
}

#[test]
fn unreachable_node_listener_refuses_the_start() {
    let out = out_path("node-refused");
    bin()
        .args([
            "run",
            "--target",
            "127.0.0.1:9",
            "--no-target-control",
            "--no-cgroup-check",
        ])
        .args([
            "--node-context-from",
            &common::closed_port(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(3);
    bin()
        .args([
            "listen",
            "--bind",
            "127.0.0.1",
            "--port",
            port(&common::closed_port()),
        ])
        .args([
            "--control-bind",
            "127.0.0.1",
            "--node-context-from",
            &common::closed_port(),
        ])
        .assert()
        .code(3);
}

#[test]
fn selftest_checks_the_node_listener() {
    if !physical_nic_visible() {
        return;
    }
    let node = start();
    let out = bin()
        .args(["selftest", "--node-context-from", &node.control])
        .assert();
    let text = common::stdout(&out);
    let line = text
        .lines()
        .find(|l| l.starts_with("node-listener"))
        .expect("check listed");
    assert!(line.contains("confirmed"), "{line}");
    stop_with(node.child, Signal::SIGTERM);
}
