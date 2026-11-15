//! One path of the report: the loopback control once, then one comparison per NIC
//! stratum present on both sides (design §10.7); a stratum on one side only is named
//! and not compared.

use std::collections::{BTreeMap, BTreeSet};

use netmeasure_core::stats::compare::{Comparison, Plan, compare};
use netmeasure_core::stats::series::Series;
use netmeasure_core::stats::side::{RunInput, Side};

use super::load::RunData;
use super::{block, checks, text};

/// The runs of one side of one path with their targets split per stratum.
pub struct Prepared<'a> {
    pub runs: Vec<&'a RunData>,
    pub strata: Vec<BTreeMap<String, Series>>,
}

impl<'a> Prepared<'a> {
    /// The runs `idx` of `runs`, their targets split per stratum; `families` maps
    /// nodes to the family they reported as a source.
    pub fn new(runs: &'a [RunData], idx: &[usize], families: &BTreeMap<String, String>) -> Self {
        Prepared {
            runs: idx.iter().map(|&i| &runs[i]).collect(),
            strata: idx
                .iter()
                .map(|&i| super::strata::split(&runs[i], families))
                .collect(),
        }
    }

    fn keys(&self) -> BTreeSet<&str> {
        self.strata
            .iter()
            .flat_map(|m| m.keys().map(String::as_str))
            .collect()
    }

    /// Source nodes as the top level, their runs below, for one stratum.
    fn side(&self, stratum: &str) -> Side<'_> {
        let mut nodes: BTreeMap<&str, Vec<RunInput<'_>>> = BTreeMap::new();
        for (r, m) in self.runs.iter().zip(&self.strata) {
            if let Some(series) = m.get(stratum) {
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
            .zip(&self.strata)
            .filter(|(_, m)| m.contains_key(stratum))
            .map(|(r, _)| *r)
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
    let loopback = compare(&b.loopback(), &a.loopback(), &loop_plan);
    let (both, one) = strata(b, a);
    text::path(path, &loopback, both.len());
    for s in &both {
        let result = StratumResult {
            stratum: s,
            targets: compare(&b.side(s), &a.side(s), plan),
            evidence: checks::evidence(&b.runs_with(s), &a.runs_with(s)),
        };
        block::stratum(&result, plan);
    }
    for s in &one {
        println!("\n-- stratum {s} --   only on one side: not compared");
    }
}
