//! End-to-end test of the NIC-family strata of `report` (design §10.7): result files
//! of real runs, rewritten to carry two NIC families and a target without context.
#![forbid(clippy::too_many_arguments)]

mod common;

use std::path::Path;

use common::listen::start;
use common::reports::{dirs, run};
use common::{bin, stdout};

/// Rewrites the only result file in `dir`: the source becomes family `source`, the
/// target `target` (`None`: no target context at all).
fn set_families(dir: &Path, source: &str, target: Option<&str>) {
    let file = std::fs::read_dir(dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let nic = |d: &str| {
        serde_json::json!({"source": "observed", "value": {"default_route_interface": "eth0",
            "interfaces": [{"name": "eth0", "driver": d, "rings": null, "rings_errno": null}]}})
    };
    v["source"]["context_before"]["nic"] = nic(source);
    for t in v["targets"].as_array_mut().unwrap() {
        if t["kind"] == "listener" {
            match target {
                Some(d) => t["context_before"]["nic"] = nic(d),
                None => t["context_before"] = serde_json::Value::Null,
            }
        }
    }
    std::fs::write(&file, serde_json::to_string(&v).unwrap()).unwrap();
}

#[test]
fn strata_by_nic_family_are_never_pooled() {
    let l = start();
    let (before, after) = dirs("strata");
    let base = before.parent().unwrap().to_owned();
    let (b2, a2) = (base.join("before2"), base.join("after2"));
    // Both before runs first: the sides must not overlap in time.
    for (dir, target) in [
        (&before, Some("drv-b")),
        (&b2, None),
        (&after, Some("drv-b")),
        (&a2, None),
    ] {
        run(&l, dir, "200");
        set_families(dir, "drv-a", target);
    }
    let assert = bin()
        .args(["report", "--design", "sequential", "--factor", "kernel"])
        .args([
            "--before",
            before.to_str().unwrap(),
            "--before",
            b2.to_str().unwrap(),
        ])
        .args([
            "--after",
            after.to_str().unwrap(),
            "--after",
            a2.to_str().unwrap(),
        ])
        .args(["--replicates", "1000", "--seed", "1"])
        .assert()
        .code(0);
    let out = stdout(&assert);
    for want in [
        "family size k = 2 (paths × NIC strata)",
        "2 NIC stratum/strata compared",
        "-- stratum drv-a → drv-b --",
        "-- stratum drv-a → unknown --",
        "NIC family unknown on one end",
    ] {
        assert!(out.contains(want), "missing `{want}` in:\n{out}");
    }
}
