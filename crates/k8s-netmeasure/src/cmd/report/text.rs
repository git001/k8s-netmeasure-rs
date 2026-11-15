//! Text output of `report` (design §10.6): header with the declarations, one section
//! per path, then the context of both sides ("context, not verdict").

use netmeasure_cli::ReportArgs;
use netmeasure_core::stats::blocklen::Determined;
use netmeasure_core::stats::compare::{Plan, QuantileComparison, SideSummary};
use netmeasure_core::stats::rate::RateComparison;
use netmeasure_core::stats::verdict::{Margin, Verdict};

use super::PathResult;
use super::checks::Field;
use super::fmt::{est, iv, ns, pname};

/// What has validated the statistical method so far (design §11).
const VALIDATION: &str =
    "unit tests and the simulation smoke grid (§11, 8 scenarios); the full grid follows";

pub fn header(args: &ReportArgs, plan: &Plan, overlap_ns: i128) {
    println!("k8s-netmeasure report");
    println!(
        "design: {} (validated: the sides overlap by {:.1} s)   factor: {}",
        format!("{:?}", args.design).to_lowercase(),
        overlap_ns as f64 / 1e9,
        format!("{:?}", args.factor).to_lowercase()
    );
    let margin = match plan.margin {
        Some(Margin::Relative(m)) => format!("±{:.1} % on the ratio after/before", m * 100.0),
        Some(Margin::AbsoluteNs(n)) => format!("±{} on the difference", ns(n as f64)),
        None => "none (the verdict is never `equivalent`)".into(),
    };
    println!(
        "primary: {} (declared before the measurement)   margin: {margin}",
        pname(plan.primary)
    );
    if let Some(r) = &args.equivalence_rationale {
        println!("margin rationale: {r}");
    }
    let l = netmeasure_core::stats::verdict::Levels::bonferroni(plan.k);
    println!(
        "family size k = {} → two-sided {:.1} %, TOST {:.1} %   replicates {}   seed {}",
        l.k,
        l.two_sided * 100.0,
        l.tost * 100.0,
        plan.replicates,
        plan.seed
    );
    println!("method validation: {VALIDATION}");
}

pub fn path(r: &PathResult, plan: &Plan) {
    let c = &r.targets;
    println!(
        "\n== path {} ==   evidence: {} ({})",
        r.path, r.evidence.0, r.evidence.1
    );
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
    let lq = &r.loopback.quantiles[0];
    let shift = match lq.verdict {
        Some(Verdict::Worse | Verdict::Better) => {
            "significant shift: the local machine changed; weigh its size against the effect"
        }
        _ => "no significant shift",
    };
    println!(
        "local control (loopback median): {} → {} ({})  interval {}  {shift}",
        est(lq.reference),
        est(lq.after),
        lq.relative
            .map_or("-".into(), |r| format!("{:+.1} %", r * 100.0)),
        iv(lq.interval, false)
    );
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

pub fn context(fields: &[Field], mismatches: &[(&'static str, String, String, String)]) {
    println!("\n== context, not verdict ==");
    for f in fields {
        let mark = match (f.differs(), f.factor) {
            (false, _) => "  ",
            (true, true) => "  ",
            (true, false) => "! ",
        };
        let join = |s: &std::collections::BTreeSet<String>| {
            s.iter().cloned().collect::<Vec<_>>().join(" | ")
        };
        if f.differs() {
            println!("{mark}{:<15} before: {}", f.name, join(&f.before));
            println!("{mark}{:<15} after:  {}", "", join(&f.after));
        } else {
            println!("{mark}{:<15} both:   {}", f.name, join(&f.before));
        }
    }
    if fields.iter().filter(|f| f.factor).all(|f| !f.differs()) {
        println!("! the declared factor did not change: before and after run the same kernel");
    }
    for (f, n, b, a) in mismatches {
        println!("! accepted mismatch on {n}: {f} {b} → {a}");
    }
}
