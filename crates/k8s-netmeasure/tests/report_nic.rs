//! End-to-end tests of `report --design concurrent --factor nic` (design §10.7): two
//! runs measured at the same time from two "nodes", rewritten to two NIC families.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::path::Path;

use common::listen::{Running, start};
use common::reports::edit;
use common::{bin, spawn, stderr, stdout};

/// Two runs at the same time, from `node-a` and `node-b`, into `dir/<node>/`.
fn concurrent(l: &Running, dir: &Path) {
    let port = l.control.rsplit(':').next().unwrap().to_owned();
    let children: Vec<_> = ["node-a", "node-b"]
        .iter()
        .map(|node| {
            let out = dir.join(node);
            std::fs::create_dir_all(&out).unwrap();
            let out = format!("{}/", out.display());
            spawn(&[
                "run",
                "--target",
                &l.addr,
                "--target-control-port",
                &port,
                "--samples",
                "400",
                "--warmup",
                "0",
                "--interval",
                "1ms",
                "--path",
                "underlay",
                "--node-name",
                node,
                "--no-cgroup-check",
                "--output",
                &out,
            ])
        })
        .collect();
    for mut c in children {
        assert!(c.wait().unwrap().success());
    }
}

/// Source family `source`, every real target family `target`.
fn families(dir: &Path, source: &str, target: &str) {
    let nic = |d: &str| {
        serde_json::json!({"source": "observed", "value": {"default_route_interface": "eth0",
            "interfaces": [{"name": "eth0", "driver": d, "rings": null, "rings_errno": null}]}})
    };
    edit(dir, |v| {
        v["source"]["context_before"]["nic"] = nic(source);
        for t in v["targets"].as_array_mut().unwrap() {
            if t["kind"] == "listener" {
                t["context_before"]["nic"] = nic(target);
            }
        }
    });
}

fn report(dir: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    bin()
        .args(["report", "--design", "concurrent", "--factor", "nic"])
        .args([
            "--runs",
            dir.to_str().unwrap(),
            "--replicates",
            "1000",
            "--seed",
            "1",
        ])
        .args(extra)
        .assert()
}

#[test]
fn families_measured_together_are_compared_on_the_same_targets() {
    let l = start();
    let dir = std::env::temp_dir().join(format!("nm-report-nic-{}", std::process::id()));
    concurrent(&l, &dir);
    families(&dir.join("node-a"), "drv-a", "drv-b");
    families(&dir.join("node-b"), "drv-b", "drv-b");
    let out = stdout(&report(&dir, &[]).code(0));
    for want in [
        "design: concurrent (validated: time overlap drv-b with drv-a",
        "factor: nic",
        "an observational comparison, not a swap of the NIC; reference family drv-a",
        "-- stratum send, targets drv-b: drv-b → drv-b against drv-a → drv-b --",
        "evidence: exploratory (1 vs 1 nodes per family)",
        "family size k = 1",
        "== context, not verdict (reference family | other families) ==",
    ] {
        assert!(out.contains(want), "missing `{want}` in:\n{out}");
    }
    // The other family as reference turns the comparison around.
    let out = stdout(&report(&dir, &["--reference-family", "drv-b"]).code(0));
    assert!(out.contains("drv-a → drv-b against drv-b → drv-b"), "{out}");
    // An unknown reference family is a usage error.
    let err = stderr(&report(&dir, &["--reference-family", "drv-x"]).code(2));
    assert!(err.contains("not in the data"), "{err}");
    // One family only: nothing to compare.
    families(&dir.join("node-b"), "drv-a", "drv-b");
    let err = stderr(&report(&dir, &[]).code(5));
    assert!(err.contains("only one NIC family"), "{err}");
}

#[test]
fn nic_needs_the_concurrent_design_and_one_data_set() {
    bin()
        .args(["report", "--design", "sequential", "--factor", "nic"])
        .args(["--runs", "/nonexistent-nm"])
        .assert()
        .code(2);
    bin()
        .args(["report", "--design", "concurrent", "--factor", "kernel"])
        .args(["--runs", "/nonexistent-nm"])
        .assert()
        .code(2);
    // --runs and --before exclude each other.
    bin()
        .args(["report", "--design", "concurrent", "--factor", "nic"])
        .args(["--runs", "/a", "--before", "/b", "--after", "/c"])
        .assert()
        .code(2);
}
