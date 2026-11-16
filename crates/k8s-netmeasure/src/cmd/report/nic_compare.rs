//! The comparisons of `report --factor nic` (design §10.7) and its context section:
//! per path the local control of the reference sources against the others, then the
//! send and receive comparisons (see `nic.rs`).

use std::collections::BTreeSet;

use netmeasure_cli::ReportArgs;
use netmeasure_core::stats::compare::compare;

use super::checks::{family, fields};
use super::load::{self, RunData};
use super::nic::{evidence, nodes};
use super::path::{Prepared, StratumResult, within_budget};
use super::strata;
use super::{block, text};

/// One planned comparison: label, reference side and side under test.
struct Planned<'a> {
    label: String,
    reference: (Prepared<'a>, String),
    test: (Prepared<'a>, String),
}

pub(super) fn compare_all(
    args: &ReportArgs,
    runs: &[RunData],
    reference: &str,
    others: &[&String],
    validated: &str,
) -> Result<(), (u8, String)> {
    let mut per_path = Vec::new();
    for (path, (idx, _)) in load::by_path(runs, &[]) {
        let rs: Vec<&RunData> = idx.iter().map(|&i| &runs[i]).collect();
        let by = |f: &str| -> Vec<&RunData> {
            rs.iter()
                .filter(|r| family(&r.context) == f)
                .copied()
                .collect()
        };
        let mut planned = Vec::new();
        let targets: BTreeSet<String> = rs
            .iter()
            .flat_map(|r| r.strata.keys())
            .filter_map(|k| k.split(" → ").nth(1).map(str::to_owned))
            .collect();
        for f in others {
            for x in &targets {
                let (kr, kf) = (strata::name(reference, x), strata::name(f, x));
                planned.push(Planned {
                    label: format!("send, targets {x}: {kf} against {kr}"),
                    reference: (Prepared::of(by(reference)), kr),
                    test: (Prepared::of(by(f)), kf),
                });
            }
            for s in [reference, f.as_str()] {
                let (kr, kf) = (strata::name(s, reference), strata::name(s, f));
                planned.push(Planned {
                    label: format!("receive, sources {s}: {kf} against {kr}"),
                    reference: (Prepared::of(by(s)), kr),
                    test: (Prepared::of(by(s)), kf),
                });
            }
        }
        planned.retain(|p| {
            !p.reference.0.runs_with(&p.reference.1).is_empty()
                && !p.test.0.runs_with(&p.test.1).is_empty()
        });
        let loopback = (
            Prepared::of(by(reference)),
            others.iter().flat_map(|f| by(f)).collect::<Vec<_>>(),
        );
        per_path.push((path, planned, loopback));
    }
    let k: usize = per_path.iter().map(|(_, p, _)| p.len()).sum();
    let plan = super::plan(args, k);
    text::header(args, &plan, &format!("time overlap {validated}"));
    println!(
        "! NIC family together with everything tied to it (instance and host generation): \
         an observational comparison, not a swap of the NIC; reference family {reference}"
    );
    for (path, planned, (lr, lf)) in &per_path {
        let lf = Prepared::of(lf.clone());
        let (sb, sa) = (lr.loopback(), lf.loopback());
        let loop_plan = netmeasure_core::stats::compare::Plan {
            quantiles: vec![0.5],
            primary: 0.5,
            margin: None,
            k: 1,
            replicates: plan.replicates.min(2000),
            ..plan.clone()
        };
        let loopback = compare(&sb, &sa, &within_budget(&loop_plan, &sb, &sa));
        text::path(path, &loopback, planned.len());
        println!("(local control: the sources of {reference} against the other families' sources)");
        for p in planned {
            let (sr, st) = (p.reference.0.side(&p.reference.1), p.test.0.side(&p.test.1));
            let (rr, rt) = (
                p.reference.0.runs_with(&p.reference.1),
                p.test.0.runs_with(&p.test.1),
            );
            let result = StratumResult {
                stratum: &p.label,
                targets: compare(&sr, &st, &within_budget(&plan, &sr, &st)),
                evidence: evidence(nodes(&rr), nodes(&rt)),
            };
            block::stratum(&result, &plan);
        }
    }
    context(runs, reference);
    Ok(())
}

/// Context per family ("context, not verdict").
fn context(runs: &[RunData], reference: &str) {
    let all: Vec<&RunData> = runs.iter().collect();
    let r: Vec<&RunData> = all
        .iter()
        .filter(|x| family(&x.context) == reference)
        .copied()
        .collect();
    let o: Vec<&RunData> = all
        .iter()
        .filter(|x| family(&x.context) != reference)
        .copied()
        .collect();
    println!("\n== context, not verdict (reference family | other families) ==");
    for f in fields(&r, &o, netmeasure_cli::report::Factor::Nic) {
        let join = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(" | ");
        let mark = if f.differs() && !f.factor { "! " } else { "  " };
        println!(
            "{mark}{:<15} {}   |   {}",
            f.name,
            join(&f.before),
            join(&f.after)
        );
    }
}
