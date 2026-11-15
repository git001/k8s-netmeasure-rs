//! Real result files for the semi-synthetic check: grouped by path and source node,
//! the real targets of a run pooled as in `report`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use netmeasure_core::schema::result::ResultFile;
use netmeasure_core::stats::blocklen;
use netmeasure_core::stats::series::{CENSORED, Series, series};

pub struct Run {
    pub targets: Series,
    pub floor: f64,
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        for e in std::fs::read_dir(p).unwrap() {
            collect(&e.unwrap().path(), out);
        }
    } else if p.extension().is_some_and(|e| e == "json") {
        out.push(p.to_owned());
    }
}

/// `path → node → runs`, real targets of a run pooled as in `report`.
pub fn load(dir: &Path) -> BTreeMap<String, BTreeMap<String, Vec<Run>>> {
    let mut files = Vec::new();
    collect(dir, &mut files);
    files.sort();
    let mut m: BTreeMap<String, BTreeMap<String, Vec<Run>>> = BTreeMap::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        if !text.contains("\"k8s-netmeasure/result\"") {
            continue;
        }
        let file: ResultFile = serde_json::from_str(&text).unwrap();
        let all = series(&file).unwrap();
        let real = &all[1..];
        let c = &file.run.config;
        let spacing = blocklen::spacing_ns(
            c.interval_ns,
            real.len() as u64,
            u64::from(c.loopback_every),
            false,
        );
        let run = Run {
            targets: Series {
                run_id: real[0].run_id.clone(),
                target: 1,
                name: "all targets".into(),
                timeout_ns: real[0].timeout_ns,
                segments: real.iter().flat_map(|s| s.segments.clone()).collect(),
            },
            floor: blocklen::floor_samples(blocklen::FLOOR_NS, spacing),
        };
        let node = file.source.context_before.node_name.value.clone();
        m.entry(c.path.clone())
            .or_default()
            .entry(node.unwrap_or_else(|| "unknown".into()))
            .or_default()
            .push(run);
    }
    m
}

pub fn scaled(s: &Series, factor: f64) -> Series {
    let mut s = s.clone();
    for seg in &mut s.segments {
        for v in &mut seg.values {
            if *v != CENSORED {
                *v = (*v as f64 * factor).round() as u64;
            }
        }
    }
    s
}
