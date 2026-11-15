//! End-to-end tests of `report --factor nic-settings` (design §10.7): the ring sizes
//! are the treatment; a kernel change on the same node is refused.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::path::Path;

use common::listen::start;
use common::reports::{dirs, edit, run};
use common::{bin, stdout};

/// The source of the run in `dir` gets one NIC with these rings and this kernel.
fn context(dir: &Path, tx: u32, kernel: &str) {
    edit(dir, |v| {
        let c = &mut v["source"]["context_before"];
        c["kernel_release"]["value"] = kernel.into();
        c["nic"] = serde_json::json!({"source": "observed", "value": {
            "default_route_interface": "eth0",
            "interfaces": [{"name": "eth0", "driver": "drv-a", "rings_errno": null,
                "rings": {"rx": 1024, "rx_max": 8192, "tx": tx, "tx_max": 8192}}]}});
    });
}

fn report(before: &Path, after: &Path) -> assert_cmd::assert::Assert {
    bin()
        .args([
            "report",
            "--design",
            "sequential",
            "--factor",
            "nic-settings",
        ])
        .args(["--before", before.to_str().unwrap()])
        .args(["--after", after.to_str().unwrap()])
        .args(["--replicates", "1000", "--seed", "1"])
        .assert()
}

#[test]
fn ring_sizes_are_the_treatment_and_the_kernel_must_not_change() {
    let l = start();
    let (before, after) = dirs("nic-settings");
    run(&l, &before, "200");
    run(&l, &after, "200");
    context(&before, 256, "k1");
    context(&after, 1024, "k1");
    let out = stdout(&report(&before, &after).code(0));
    for want in [
        "factor: nic-settings",
        "  nic_rings       before: drv-a:1024/256",
        "  kernel_release  both:   k1",
    ] {
        assert!(out.contains(want), "missing `{want}` in:\n{out}");
    }
    assert!(!out.contains("did not change"), "{out}");

    // The same node with another kernel measures the kernel too: refused.
    context(&after, 1024, "k2");
    let err =
        String::from_utf8_lossy(&report(&before, &after).code(5).get_output().stderr).into_owned();
    assert!(err.contains("kernel_release k1 → k2"), "{err}");
}

#[test]
fn different_source_nodes_are_observational() {
    let l = start();
    let (before, after) = dirs("other-nodes");
    run(&l, &before, "200");
    run(&l, &after, "200");
    context(&before, 256, "k1");
    context(&after, 1024, "k1");
    edit(&after, |v| {
        v["source"]["context_before"]["node_name"]["value"] = "node-b".into();
    });
    let out = stdout(&report(&before, &after).code(0));
    for want in [
        "! source nodes: 0 on both sides, 1 only before, 1 only after",
        "evidence: exploratory (no source node on both sides",
    ] {
        assert!(out.contains(want), "missing `{want}` in:\n{out}");
    }
}
