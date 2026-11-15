//! `report` (design §10.4–§10.6), first step: the sequential design with the factor
//! `kernel`. Per path one comparison of all real targets, the source nodes as the
//! top resampling level, the loopback control on its median; the context of both
//! sides is listed and a changed NIC family or CPU model per node refuses (exit 5).
//! The verdict never sets the exit code.

mod checks;
mod fmt;
mod load;
mod text;

use std::collections::BTreeMap;
use std::process::ExitCode;

use netmeasure_cli::ReportArgs;
use netmeasure_cli::report::MarginArg;
use netmeasure_core::stats::compare::{Comparison, Plan, compare};
use netmeasure_core::stats::side::{RunInput, Side};
use netmeasure_core::stats::verdict::Margin;

use self::load::RunData;
use crate::exit;
use crate::sys::time;

pub fn run(args: &ReportArgs) -> ExitCode {
    match report(args) {
        Ok(()) => exit::code(exit::OK),
        Err((code, message)) => {
            tracing::error!(message = %message, "report refused");
            eprintln!("report: {message}");
            exit::code(code)
        }
    }
}

/// Everything the text output needs for one path.
pub struct PathResult<'a> {
    pub path: &'a str,
    pub targets: Comparison,
    pub loopback: Comparison,
    pub evidence: (&'static str, String),
}

fn report(args: &ReportArgs) -> Result<(), (u8, String)> {
    let usage = |e: String| (exit::USAGE, e);
    let before = load::load(&args.before).map_err(usage)?;
    let after = load::load(&args.after).map_err(usage)?;
    if before.is_empty() || after.is_empty() {
        return Err(usage("no result files on one side".into()));
    }
    let before_refs: Vec<&RunData> = before.iter().collect();
    let after_refs: Vec<&RunData> = after.iter().collect();

    let overlap =
        checks::sequential(&before_refs, &after_refs).map_err(|e| (exit::COMPARISON_REFUSED, e))?;
    let mismatches = checks::node_mismatches(&before_refs, &after_refs);
    let refused: Vec<_> = mismatches
        .iter()
        .filter(|m| !args.allow_context_mismatch.iter().any(|a| a == m.0))
        .collect();
    if !refused.is_empty() {
        let list: Vec<String> = refused
            .iter()
            .map(|(f, n, b, a)| format!("{n}: {f} {b} → {a}"))
            .collect();
        return Err((
            exit::COMPARISON_REFUSED,
            format!(
                "context changed on a node, which measures the host, not the {:?}: {} \
                 (accept with --allow-context-mismatch)",
                args.factor,
                list.join("; ")
            ),
        ));
    }
    let groups = load::by_path(&before, &after);
    let k = groups
        .values()
        .filter(|(b, a)| !b.is_empty() && !a.is_empty())
        .count() as u32;
    let mut quantiles = args.quantiles.clone();
    quantiles.push(args.primary);
    quantiles.sort_by(f64::total_cmp);
    quantiles.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let plan = Plan {
        quantiles,
        primary: args.primary,
        margin: args.equivalence_margin.map(|m| match m {
            MarginArg::Relative(r) => Margin::Relative(r),
            MarginArg::AbsoluteNs(n) => Margin::AbsoluteNs(n),
        }),
        k: k.max(1),
        replicates: args.replicates as usize,
        seed: args.seed.unwrap_or(time::realtime_ns() as u64),
        block_override: None,
    };
    text::header(args, &plan, overlap);
    for (path, (bi, ai)) in &groups {
        if bi.is_empty() || ai.is_empty() {
            println!("\n== path {path} ==  only on one side: not compared");
            continue;
        }
        let b: Vec<&RunData> = bi.iter().map(|&i| &before[i]).collect();
        let a: Vec<&RunData> = ai.iter().map(|&i| &after[i]).collect();
        let loop_plan = Plan {
            quantiles: vec![0.5],
            primary: 0.5,
            margin: None,
            k: 1,
            replicates: plan.replicates.min(2000),
            ..plan.clone()
        };
        let result = PathResult {
            path,
            targets: compare(&side(&b, false), &side(&a, false), &plan),
            loopback: compare(&side(&b, true), &side(&a, true), &loop_plan),
            evidence: checks::evidence(&b, &a),
        };
        text::path(&result, &plan);
    }
    text::context(&checks::fields(&before_refs, &after_refs), &mismatches);
    Ok(())
}

/// One side: source nodes as the top level, their runs below.
fn side<'a>(runs: &[&'a RunData], loopback: bool) -> Side<'a> {
    let mut nodes: BTreeMap<&str, Vec<RunInput<'a>>> = BTreeMap::new();
    for r in runs {
        let input = if loopback {
            RunInput {
                series: &r.loopback,
                floor_samples: r.loopback_floor,
            }
        } else {
            RunInput {
                series: &r.targets,
                floor_samples: r.floor,
            }
        };
        nodes.entry(r.node.as_str()).or_default().push(input);
    }
    Side {
        nodes: nodes.into_values().collect(),
    }
}
