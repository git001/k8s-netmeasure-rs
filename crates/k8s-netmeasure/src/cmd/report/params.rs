//! Runs with different load parameters are never pooled (design §10.6): within a path,
//! every run of both sides must have the same pacing interval, loopback share,
//! connect timeout and topology. The sample count may differ; it changes the
//! precision, not what is measured.

use std::collections::BTreeMap;

use super::load::RunData;

/// The parameters that change the load or the measured quantity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Load {
    interval_ns: u64,
    loopback_every: u32,
    timeout_ns: u64,
    topology: String,
}

impl Load {
    fn of(r: &RunData) -> Self {
        let c = &r.config;
        Self {
            interval_ns: c.interval_ns,
            loopback_every: c.loopback_every,
            timeout_ns: c.timeout_ns,
            topology: c.topology.clone(),
        }
    }

    fn describe(&self) -> String {
        format!(
            "interval {} ms, loopback every {}, timeout {} ms, topology {}",
            self.interval_ns as f64 / 1e6,
            self.loopback_every,
            self.timeout_ns as f64 / 1e6,
            self.topology
        )
    }
}

/// `Err` (exit 5) naming every parameter set of a path and where it occurs.
pub fn check(path: &str, before: &[&RunData], after: &[&RunData]) -> Result<(), String> {
    let mut sets: BTreeMap<Load, Vec<String>> = BTreeMap::new();
    for (side, runs) in [("before", before), ("after", after)] {
        for r in runs {
            sets.entry(Load::of(r))
                .or_default()
                .push(format!("{side} {} {}", r.node, r.run_id));
        }
    }
    if sets.len() <= 1 {
        return Ok(());
    }
    let list: Vec<String> = sets
        .iter()
        .map(|(l, runs)| {
            let first = runs.first().map_or("", String::as_str);
            format!("{} in {} run(s), e.g. {first}", l.describe(), runs.len())
        })
        .collect();
    Err(format!(
        "path {path}: runs with different load parameters are never pooled: {}",
        list.join("; ")
    ))
}
