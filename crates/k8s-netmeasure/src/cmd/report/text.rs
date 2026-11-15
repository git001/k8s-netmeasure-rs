//! Text output of `report` (design §10.6): header with the declarations, the path
//! headings with the loopback control (strata: `block.rs`), then the context of both sides ("context, not verdict").

use netmeasure_cli::ReportArgs;
use netmeasure_core::stats::compare::{Comparison, Plan};
use netmeasure_core::stats::normal::sample_factor;
use netmeasure_core::stats::verdict::{Margin, Verdict};

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
        args.factor.name()
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
        "family size k = {} (paths × NIC strata) → two-sided {:.1} %, TOST {:.1} % \
         (the power of k = 1 needs ≈ {:.2} × the samples)   replicates {}   seed {}",
        l.k,
        l.two_sided * 100.0,
        l.tost * 100.0,
        sample_factor(l.k),
        plan.replicates,
        plan.seed
    );
    println!("method validation: {VALIDATION}");
}

/// Path heading and the loopback control, which belongs to the source machines and
/// is the same for every stratum.
pub fn path(path: &str, loopback: &Comparison, strata: usize) {
    println!("\n== path {path} ==   {strata} NIC stratum/strata compared");
    let lq = &loopback.quantiles[0];
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
    let factor: Vec<&str> = fields.iter().filter(|f| f.factor).map(|f| f.name).collect();
    if fields.iter().filter(|f| f.factor).all(|f| !f.differs()) {
        println!(
            "! the declared factor did not change: {} the same on both sides",
            factor.join(", ")
        );
    }
    for (f, n, b, a) in mismatches {
        println!("! accepted mismatch on {n}: {f} {b} → {a}");
    }
}
