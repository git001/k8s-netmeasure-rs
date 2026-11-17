//! End-to-end tests of `report --factor nic-settings` (design §10.7): ring sizes and
//! channels are the treatment; a kernel, driver or firmware change on the same node is
//! refused.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::path::Path;

use common::listen::start;
use common::reports::{dirs, edit, run};
use common::{bin, stdout};

/// The source of the run in `dir` gets one NIC with these rings and this kernel.
fn context(dir: &Path, tx: u32, kernel: &str) {
    nic_info(dir, tx, kernel, serde_json::json!({}));
}

/// As `context`, with `extra` fields (driver info, channels) on the interface.
fn nic_info(dir: &Path, tx: u32, kernel: &str, extra: serde_json::Value) {
    edit(dir, |v| {
        let c = &mut v["source"]["context_before"];
        c["kernel_release"]["value"] = kernel.into();
        c["nic"] = serde_json::json!({"source": "observed", "value": {
            "default_route_interface": "eth0",
            "interfaces": [{"name": "eth0", "driver": "drv-a", "rings_errno": null,
                "rings": {"rx": 1024, "rx_max": 8192, "tx": tx, "tx_max": 8192}}]}});
        let i = &mut c["nic"]["value"]["interfaces"][0];
        for (k, x) in extra.as_object().unwrap() {
            i[k] = x.clone();
        }
    });
}

fn info(fw: &str, combined: u32) -> serde_json::Value {
    let n = serde_json::Value::Null;
    serde_json::json!({
        "driver_info": {"version": "k1", "firmware": fw, "bus": "0000:01:00.0"},
        "driver_info_errno": n,
        "channels": {"rx": n, "tx": n, "other": n, "combined": combined,
            "rx_max": n, "tx_max": n, "other_max": n, "combined_max": 32},
        "channels_errno": n})
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
fn channels_are_the_treatment_and_the_firmware_must_not_change() {
    let l = start();
    let (before, after) = dirs("channels");
    run(&l, &before, "200");
    run(&l, &after, "200");
    nic_info(&before, 1024, "k1", info("fw1", 16));
    nic_info(&after, 1024, "k1", info("fw1", 32));
    let out = stdout(&report(&before, &after).code(0));
    for want in [
        "  nic_channels    before: drv-a:16/32",
        " after:  drv-a:32/32",
        "  nic_driver      both:   drv-a:k1 fw fw1",
    ] {
        assert!(
            out.contains(want),
            "missing `{want}` in:
{out}"
        );
    }
    assert!(!out.contains("did not change"), "{out}");

    // New firmware on the same node is another treatment: refused.
    nic_info(&after, 1024, "k1", info("fw2", 32));
    let err =
        String::from_utf8_lossy(&report(&before, &after).code(5).get_output().stderr).into_owned();
    assert!(
        err.contains("nic_driver drv-a:k1 fw fw1 → drv-a:k1 fw fw2"),
        "{err}"
    );
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

#[test]
fn source_nic_counters_are_summed_per_side() {
    let l = start();
    let (before, after) = dirs("nic-counters");
    run(&l, &before, "200");
    run(&l, &after, "200");
    context(&before, 256, "k1");
    context(&after, 1024, "k1");
    let set = |dir: &Path, nic: serde_json::Value| {
        edit(dir, |v| {
            for c in v["diagnostics"]["cycles"].as_array_mut().unwrap() {
                c["source"]["nic"] = nic.clone();
            }
        });
    };
    set(
        &before,
        serde_json::json!({"vf0": {"stop_queue": 2, "wake_queue": 2}}),
    );
    set(&after, serde_json::json!({}));
    let out = stdout(&report(&before, &after).code(0));
    assert!(
        out.contains("before: stop_queue ")
            && out.contains("(1 of 1 runs recorded); after: no increase (1 of 1 runs recorded)"),
        "{out}"
    );
}
