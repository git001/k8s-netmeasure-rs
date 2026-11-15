//! One NIC stratum of a path in the text output (design §10.6, §10.7): sides,
//! quantiles with verdict, rates and the between-run spread.

use netmeasure_core::stats::blocklen::Determined;
use netmeasure_core::stats::compare::{Plan, QuantileComparison, SideSummary};
use netmeasure_core::stats::rate::RateComparison;
use netmeasure_core::stats::verdict::{Margin, Verdict};

use super::fmt::{est, iv, ns, pname};
use super::path::StratumResult;
use super::strata::UNKNOWN;

pub fn stratum(r: &StratumResult, plan: &Plan) {
    let c = &r.targets;
    println!(
        "\n-- stratum {} --   evidence: {} ({})",
        r.stratum, r.evidence.0, r.evidence.1
    );
    if r.stratum.contains(UNKNOWN) {
        println!(
            "! NIC family unknown on one end (overlay without a node listener, or no \
             target context): this stratum may mix families"
        );
    }
    println!(
        "{:<8} {:>5} {:>5} {:>9} {:>8}  block length",
        "side", "nodes", "runs", "samples", "timeouts"
    );
    summary("before", &c.reference);
    summary("after", &c.after);
    println!(
        "{:<6} {:>11} {:>11} {:>8}  {:<34} verdict",
        "", "before", "after", "change", "interval"
    );
    for q in &c.quantiles {
        quantile(q, c.levels.two_sided, plan);
    }
    rates("failures", &c.failures);
    rates("SYN retransmissions (diagnostics)", &c.syn_retransmissions);
    let spread = |s: &SideSummary| {
        let v: Vec<u64> = s.per_run_primary.iter().filter_map(|x| x.1).collect();
        match (v.iter().min(), v.iter().max()) {
            (Some(lo), Some(hi)) => format!("{} – {}", ns(*lo as f64), ns(*hi as f64)),
            _ => "-".into(),
        }
    };
    println!(
        "between-run spread of {} (per run): before {}, after {}",
        pname(plan.primary),
        spread(&c.reference),
        spread(&c.after)
    );
}

fn summary(name: &str, s: &SideSummary) {
    let by = |d: Determined| match d {
        Determined::Data => "data",
        Determined::Floor => "floor 5 s",
        Determined::Cap => "cap n/10",
    };
    let deciding = s
        .blocks
        .iter()
        .max_by(|a, b| a.applied.total_cmp(&b.applied));
    let raw: f64 = s.blocks.iter().map(|b| b.raw).fold(0.0, f64::max);
    println!(
        "{name:<8} {:>5} {:>5} {:>9} {:>8}  {:.0} samples ({}; largest raw estimate {:.1})",
        s.nodes,
        s.runs,
        s.n,
        s.timeouts,
        s.block_length,
        deciding.map_or("-", |d| by(d.determined_by)),
        raw
    );
}

fn quantile(q: &QuantileComparison, level: f64, plan: &Plan) {
    let ratio = matches!(plan.margin, Some(Margin::Relative(_)));
    let change = q
        .relative
        .map_or("-".into(), |r| format!("{:+.1} %", r * 100.0));
    let verdict = match q.verdict {
        None => "descriptive, no verdict".into(),
        Some(Verdict::Equivalent { significant: true }) => {
            "EQUIVALENT (statistically significant, practically irrelevant)".into()
        }
        Some(Verdict::Inconclusive { censored: true }) => "INCONCLUSIVE (censored)".into(),
        Some(v) => format!("{v:?}")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_uppercase(),
    };
    let lvl = if q.verdict.is_some() { level } else { 0.95 };
    let mut interval = format!("{} {:.1} %", iv(q.interval, ratio), lvl * 100.0);
    if let Some(t) = q.tost_interval {
        interval.push_str(&format!(", TOST {}", iv(Some(t), ratio)));
    }
    println!(
        "{:<6} {:>11} {:>11} {:>8}  {:<34} {verdict}",
        pname(q.p),
        est(q.reference),
        est(q.after),
        change,
        interval
    );
}

fn rates(name: &str, r: &RateComparison) {
    let side = |s: &netmeasure_core::stats::rate::RateSide| format!("{}/{}", s.events, s.attempts);
    let tail = match (&r.verdict, r.difference) {
        (Some(v), _) => format!("verdict {v:?}"),
        (None, Some(d)) => format!("difference {}", iv(Some(d), false)),
        (None, None) => "fewer than 10 events: descriptive only".into(),
    };
    println!(
        "{name}: before {}, after {}  {tail}",
        side(&r.reference),
        side(&r.after)
    );
}
