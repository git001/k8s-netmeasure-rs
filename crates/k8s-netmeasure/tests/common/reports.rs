//! Result files for the `report` end-to-end tests: short real runs per directory.

use std::path::{Path, PathBuf};

use super::bin;
use super::listen::Running;

/// A short real run against the listener, written into `dir`.
pub fn run(l: &Running, dir: &Path, samples: &str) {
    run_at(l, dir, samples, "1ms");
}

pub fn run_at(l: &Running, dir: &Path, samples: &str, interval: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let control_port = l.control.rsplit(':').next().unwrap();
    bin()
        .args([
            "run",
            "--target",
            &l.addr,
            "--target-control-port",
            control_port,
        ])
        .args(["--samples", samples, "--warmup", "0"])
        .args([
            "--interval",
            interval,
            "--path",
            "underlay",
            "--node-name",
            "node-a",
        ])
        .args([
            "--no-cgroup-check",
            "--output",
            &format!("{}/", dir.display()),
        ])
        .assert()
        .code(0);
}

pub fn dirs(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("nm-report-{}-{name}", std::process::id()));
    (base.join("before"), base.join("after"))
}
