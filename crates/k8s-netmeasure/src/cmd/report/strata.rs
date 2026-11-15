//! NIC-family strata (design §10.7): every measured pair belongs to the stratum
//! `source family → target family`, and strata are never pooled. The family is the
//! set of drivers of a node's physical interfaces (decided 2026-11-15: a synthetic NIC
//! present on every host does not blur the distinction, and the code needs no list of
//! vendor drivers). A target without its own context takes the family its node
//! reported as a source; otherwise it is `unknown`.

use std::collections::{BTreeMap, BTreeSet};

use netmeasure_core::stats::series::Series;

use super::checks::family;
use super::load::RunData;

pub const UNKNOWN: &str = "unknown";

/// Node → NIC family as reported by the runs on that node (the source contexts).
pub fn node_families(runs: &[&RunData]) -> BTreeMap<String, String> {
    runs.iter()
        .map(|r| (r.node.clone(), family(&r.file.source.context_before)))
        .filter(|(_, f)| f != UNKNOWN)
        .collect()
}

pub fn name(source: &str, target: &str) -> String {
    format!("{source} → {target}")
}

/// One run's real targets pooled per stratum (blocks never cross targets, because
/// every target's stretches stay separate segments).
pub fn split(run: &RunData, nodes: &BTreeMap<String, String>) -> BTreeMap<String, Series> {
    let source = family(&run.file.source.context_before);
    let mut out: BTreeMap<String, Series> = BTreeMap::new();
    for t in &run.targets {
        let target = t
            .family
            .clone()
            .or_else(|| nodes.get(&t.name).cloned())
            .unwrap_or_else(|| UNKNOWN.into());
        let key = name(&source, &target);
        let s = out.entry(key.clone()).or_insert_with(|| Series {
            run_id: t.series.run_id.clone(),
            target: 1,
            name: key,
            timeout_ns: t.series.timeout_ns,
            segments: Vec::new(),
        });
        s.segments.extend(t.series.segments.iter().cloned());
    }
    out
}

/// Instance types that occur with more than one NIC family, with the families
/// (§10.7: results are stratified by observed family, never by instance type).
pub fn mixed_instance_types(runs: &[&RunData]) -> Vec<(String, Vec<String>)> {
    let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in runs {
        let c = &r.file.source.context_before;
        if let Some(t) = &c.instance_type.value {
            m.entry(t.clone()).or_default().insert(family(c));
        }
    }
    m.into_iter()
        .filter(|(_, f)| f.len() > 1)
        .map(|(t, f)| (t, f.into_iter().collect()))
        .collect()
}
