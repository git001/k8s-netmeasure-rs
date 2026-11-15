//! One path of the report: the loopback control once, then one comparison per NIC
//! stratum present on both sides (design §10.7); a stratum on one side only is named
//! and not compared.

use std::collections::{BTreeMap, BTreeSet};

use netmeasure_core::stats::compare::{Comparison, Plan, compare};
use netmeasure_core::stats::side::{RunInput, Side};

use super::load::RunData;
use super::{block, evidence, text};

/// The runs of one side of one path (their targets already split per stratum).
pub struct Prepared<'a> {
    pub runs: Vec<&'a RunData>,
}

impl<'a> Prepared<'a> {
    pub fn new(runs: &'a [RunData], idx: &[usize]) -> Self {
        Prepared {
            runs: idx.iter().map(|&i| &runs[i]).collect(),
        }
    }

    fn keys(&self) -> BTreeSet<&str> {
        self.runs
            .iter()
            .flat_map(|r| r.strata.keys().map(String::as_str))
            .collect()
    }

    /// Source nodes as the top level, their runs below, for one stratum.
    fn side(&self, stratum: &str) -> Side<'_> {
        let mut nodes: BTreeMap<&str, Vec<RunInput<'_>>> = BTreeMap::new();
        for r in &self.runs {
            if let Some(series) = r.strata.get(stratum) {
                nodes.entry(r.node.as_str()).or_default().push(RunInput {
                    series,
                    floor_samples: r.floor,
                });
            }
        }
        Side {
            nodes: nodes.into_values().collect(),
        }
    }

    fn runs_with(&self, stratum: &str) -> Vec<&RunData> {
        self.runs
            .iter()
            .filter(|r| r.strata.contains_key(stratum))
            .copied()
            .collect()
    }

    fn loopback(&self) -> Side<'_> {
        let mut nodes: BTreeMap<&str, Vec<RunInput<'_>>> = BTreeMap::new();
        for r in &self.runs {
            nodes.entry(r.node.as_str()).or_default().push(RunInput {
                series: &r.loopback,
                floor_samples: r.loopback_floor,
            });
        }
        Side {
            nodes: nodes.into_values().collect(),
        }
    }
}

/// Strata on both sides (compared) and on one side only.
pub fn strata(b: &Prepared, a: &Prepared) -> (Vec<String>, Vec<String>) {
    let (kb, ka) = (b.keys(), a.keys());
    let both = kb.intersection(&ka).map(|s| s.to_string()).collect();
    let one = kb
        .symmetric_difference(&ka)
        .map(|s| s.to_string())
        .collect();
    (both, one)
}

/// Memory for resampling buffers: every bootstrap thread holds one resampled side.
const BUFFER_BUDGET: usize = 1 << 30;

/// `plan` with as many threads as the buffer budget allows, at least one.
fn within_budget(plan: &Plan, b: &Side, a: &Side) -> Plan {
    let n = |s: &Side| -> usize {
        s.runs()
            .flat_map(|r| &r.series.segments)
            .map(|g| g.values.len())
            .sum()
    };
    // 8 bytes per value, 1 per mark, doubled for vector growth, rounded up.
    let per_thread = 20 * n(b).max(n(a)).max(1);
    Plan {
        threads: (BUFFER_BUDGET / per_thread).clamp(1, plan.threads.max(1)),
        ..plan.clone()
    }
}

/// Everything the text output needs for one stratum.
pub struct StratumResult<'a> {
    pub stratum: &'a str,
    pub targets: Comparison,
    pub evidence: (&'static str, String),
}

pub fn report(path: &str, b: &Prepared, a: &Prepared, plan: &Plan) {
    let loop_plan = Plan {
        quantiles: vec![0.5],
        primary: 0.5,
        margin: None,
        k: 1,
        replicates: plan.replicates.min(2000),
        ..plan.clone()
    };
    let (lb, la) = (b.loopback(), a.loopback());
    let loopback = compare(&lb, &la, &within_budget(&loop_plan, &lb, &la));
    let (both, one) = strata(b, a);
    text::path(path, &loopback, both.len());
    let (common, only_b, only_a) = evidence::node_sets(&b.runs, &a.runs);
    if only_b + only_a > 0 {
        println!(
            "! source nodes: {common} on both sides, {only_b} only before, {only_a} only after \
             (the sequential design compares the same nodes)"
        );
    }
    for s in &both {
        let result = StratumResult {
            stratum: s,
            targets: {
                let (sb, sa) = (b.side(s), a.side(s));
                compare(&sb, &sa, &within_budget(plan, &sb, &sa))
            },
            evidence: evidence::evidence(&b.runs_with(s), &a.runs_with(s)),
        };
        block::stratum(&result, plan);
    }
    for s in &one {
        println!("\n-- stratum {s} --   only on one side: not compared");
    }
}
