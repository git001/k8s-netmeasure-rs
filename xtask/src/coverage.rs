//! Gate `coverage` (design §12): line coverage per crate, measured with
//! `cargo llvm-cov`. lcov and HTML reports are written for the CI artifact.

use std::collections::BTreeMap;
use std::path::Path;

use crate::proc::cargo;

/// Minimum line coverage in percent per crate directory under `crates/`.
pub const THRESHOLDS: &[(&str, f64)] = &[
    ("netmeasure-core", 90.0),
    ("netmeasure-util", 90.0),
    ("netmeasure-cli", 90.0),
    ("k8s-netmeasure", 70.0),
];

pub fn run(root: &Path) -> Result<(), String> {
    let dir = root.join("target/llvm-cov");
    let summary = dir.join("summary.json");
    let lcov = dir.join("lcov.info");
    cargo(root, &["llvm-cov", "clean", "--workspace"])?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    cargo(
        root,
        &[
            "llvm-cov",
            "--no-report",
            "--workspace",
            "--exclude",
            "xtask",
            "--locked",
        ],
    )?;
    let summary_s = summary.to_string_lossy().into_owned();
    let lcov_s = lcov.to_string_lossy().into_owned();
    cargo(
        root,
        &[
            "llvm-cov",
            "report",
            "--json",
            "--summary-only",
            "--output-path",
            &summary_s,
        ],
    )?;
    cargo(
        root,
        &["llvm-cov", "report", "--lcov", "--output-path", &lcov_s],
    )?;
    cargo(root, &["llvm-cov", "report", "--html"])?;
    let json = std::fs::read_to_string(&summary).map_err(|e| format!("{summary_s}: {e}"))?;
    let per_crate = lines_per_crate(&json)?;
    check(&per_crate)
}

/// `(covered, count)` line totals per crate directory, from llvm-cov's JSON export.
pub fn lines_per_crate(json: &str) -> Result<BTreeMap<String, (u64, u64)>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let files = v["data"][0]["files"]
        .as_array()
        .ok_or("llvm-cov JSON without data[0].files")?;
    let mut totals = BTreeMap::new();
    for f in files {
        let name = f["filename"].as_str().unwrap_or("");
        let Some(krate) = crate_of(name) else {
            continue;
        };
        let lines = &f["summary"]["lines"];
        let entry = totals.entry(krate.to_owned()).or_insert((0, 0));
        entry.0 += lines["covered"].as_u64().unwrap_or(0);
        entry.1 += lines["count"].as_u64().unwrap_or(0);
    }
    Ok(totals)
}

fn crate_of(path: &str) -> Option<&str> {
    let rest = &path[path.find("/crates/")? + "/crates/".len()..];
    rest.split('/').next()
}

pub fn check(per_crate: &BTreeMap<String, (u64, u64)>) -> Result<(), String> {
    let mut failures = Vec::new();
    for (krate, min) in THRESHOLDS {
        match per_crate.get(*krate) {
            Some(&(covered, count)) if count > 0 => {
                let pct = 100.0 * covered as f64 / count as f64;
                eprintln!(
                    "coverage: {krate}: {pct:.1} % ({covered}/{count} lines, minimum {min} %)"
                );
                if pct < *min {
                    failures.push(format!("{krate}: {pct:.1} % < {min} %"));
                }
            }
            _ => eprintln!("coverage: {krate}: no executable lines yet, nothing to measure"),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"data":[{"files":[
        {"filename":"/w/crates/netmeasure-core/src/lib.rs","summary":{"lines":{"count":10,"covered":9}}},
        {"filename":"/w/crates/netmeasure-core/src/a.rs","summary":{"lines":{"count":10,"covered":10}}},
        {"filename":"/w/crates/k8s-netmeasure/src/main.rs","summary":{"lines":{"count":10,"covered":6}}},
        {"filename":"/w/xtask/src/main.rs","summary":{"lines":{"count":5,"covered":0}}}
    ]}]}"#;

    #[test]
    fn sums_per_crate_and_ignores_other_paths() {
        let t = lines_per_crate(SAMPLE).unwrap();
        assert_eq!(t["netmeasure-core"], (19, 20));
        assert_eq!(t["k8s-netmeasure"], (6, 10));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn thresholds_apply_per_crate() {
        let mut t = lines_per_crate(SAMPLE).unwrap();
        assert!(check(&t).unwrap_err().contains("k8s-netmeasure: 60.0 %"));
        t.insert("k8s-netmeasure".into(), (7, 10));
        assert!(check(&t).is_ok());
    }
}
