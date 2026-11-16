//! `report --design concurrent --factor nic` (design §10.7): one data set, measured
//! at the same time; the groups are the observed NIC families of the source nodes.
//! Observational: a family comes with its host generation, so the report names it
//! "NIC family together with everything tied to it".
//!
//! Per path two kinds of comparison, each with its own verdict (k counts them all):
//! - send: sources of family F against sources of the reference R on the same target
//!   family X (`F → X` against `R → X`);
//! - receive: the same sources S on targets of both families (`S → F` against
//!   `S → R`). Both sides come from the same runs; resampling them independently is
//!   conservative for the positive correlation this gives (§10.3).

use std::collections::BTreeSet;

use netmeasure_cli::ReportArgs;

use super::checks::family;
use super::load::{self, RunData};
use super::params;
use super::strata::{self, UNKNOWN};
use crate::exit;

/// Two groups must share at least this part of their joint time span (§10.6).
const MIN_OVERLAP: f64 = 0.8;

/// Intersection over union of the two groups' wall-clock spans.
fn overlap(a: &[&RunData], b: &[&RunData]) -> f64 {
    let span = |rs: &[&RunData]| {
        let s = rs.iter().map(|r| r.start_ns).min().unwrap_or(0);
        let e = rs.iter().map(|r| r.end_ns).max().unwrap_or(0);
        (s, e)
    };
    let ((as_, ae), (bs, be)) = (span(a), span(b));
    let inter = (ae.min(be) - as_.max(bs)).max(0) as f64;
    let union = (ae.max(be) - as_.min(bs)).max(1) as f64;
    inter / union
}

/// Evidence for `--factor nic` (§10.7): by nodes per family.
pub(super) fn evidence(nodes_r: usize, nodes_f: usize) -> (&'static str, String) {
    let n = nodes_r.min(nodes_f);
    let detail = format!("{nodes_r} vs {nodes_f} nodes per family");
    match n {
        0..=2 => ("exploratory", detail),
        _ => (
            "preliminary",
            format!(
                "{detail}; `strong (observational)` needs ≥ 5 per family and \
                 leave-one-node-out stability, not checked yet"
            ),
        ),
    }
}

pub(super) fn nodes(runs: &[&RunData]) -> usize {
    runs.iter()
        .map(|r| r.node.as_str())
        .collect::<BTreeSet<_>>()
        .len()
}

pub fn report(args: &ReportArgs) -> Result<(), (u8, String)> {
    let refuse = |e: String| (exit::COMPARISON_REFUSED, e);
    let mut runs = load::load(&args.runs).map_err(|e| (exit::USAGE, e))?;
    let families = strata::node_families(&runs.iter().collect::<Vec<_>>());
    runs.iter_mut().for_each(|r| strata::assign(r, &families));
    let all: Vec<&RunData> = runs.iter().collect();
    let known: BTreeSet<String> = all
        .iter()
        .map(|r| family(&r.context))
        .filter(|f| f != UNKNOWN)
        .collect();
    let reference = match &args.reference_family {
        Some(f) if known.contains(f) => f.clone(),
        Some(f) => {
            return Err((
                exit::USAGE,
                format!("reference family {f} not in the data: {known:?}"),
            ));
        }
        None => known.first().cloned().unwrap_or_default(),
    };
    let others: Vec<&String> = known.iter().filter(|f| **f != reference).collect();
    if others.is_empty() {
        return Err(refuse(format!(
            "only one NIC family in the data ({reference}): nothing to compare"
        )));
    }
    let of = |f: &str| -> Vec<&RunData> {
        all.iter()
            .filter(|r| family(&r.context) == f)
            .copied()
            .collect()
    };
    let group_r = of(&reference);
    let mut validated = Vec::new();
    for f in &others {
        let o = overlap(&group_r, &of(f));
        if o < MIN_OVERLAP {
            return Err(refuse(format!(
                "families {reference} and {f} overlap by {:.0} % of their time span, \
                 below {:.0} %: not a concurrent measurement",
                o * 100.0,
                MIN_OVERLAP * 100.0
            )));
        }
        validated.push(format!("{f} with {reference} {:.0} %", o * 100.0));
    }
    let kernels: BTreeSet<String> = all
        .iter()
        .map(|r| {
            r.context
                .kernel_release
                .value
                .clone()
                .unwrap_or_else(|| UNKNOWN.into())
        })
        .collect();
    if kernels.len() > 1
        && !args
            .allow_context_mismatch
            .iter()
            .any(|a| a == "kernel_release")
    {
        return Err(refuse(format!(
            "different kernels in the data ({kernels:?}): the families must run the same \
             kernel (accept with --allow-context-mismatch kernel_release)"
        )));
    }
    for (path, (idx, _)) in load::by_path(&runs, &[]) {
        let rs: Vec<&RunData> = idx.iter().map(|&i| &runs[i]).collect();
        let r: Vec<&RunData> = rs
            .iter()
            .filter(|x| family(&x.context) == reference)
            .copied()
            .collect();
        let f: Vec<&RunData> = rs
            .iter()
            .filter(|x| family(&x.context) != reference)
            .copied()
            .collect();
        params::check(&path, &r, &f).map_err(refuse)?;
    }
    super::nic_compare::compare_all(args, &runs, &reference, &others, &validated.join(", "))
}
