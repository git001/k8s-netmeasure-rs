//! `report` (design §10.4–§10.7), first step: the sequential design with the factor
//! `kernel`. Per path and NIC stratum (source family → target family) one comparison,
//! never pooled across strata, the source nodes as the top resampling level; the
//! loopback control on its median per path; the context of both sides is listed and
//! a changed NIC family or CPU model per node refuses (exit 5). The verdict never
//! sets the exit code.

mod block;
mod checks;
mod fmt;
mod load;
mod params;
mod path;
mod strata;
mod text;

use std::process::ExitCode;

use netmeasure_cli::ReportArgs;
use netmeasure_cli::report::MarginArg;
use netmeasure_core::stats::compare::Plan;
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
    for (path, (bi, ai)) in &groups {
        let b: Vec<&RunData> = bi.iter().map(|&i| &before[i]).collect();
        let a: Vec<&RunData> = ai.iter().map(|&i| &after[i]).collect();
        params::check(path, &b, &a).map_err(|e| (exit::COMPARISON_REFUSED, e))?;
    }
    let (fam_b, fam_a) = (
        strata::node_families(&before_refs),
        strata::node_families(&after_refs),
    );
    let paths: Vec<(&String, path::Prepared, path::Prepared)> = groups
        .iter()
        .map(|(p, (bi, ai))| {
            (
                p,
                path::Prepared::new(&before, bi, &fam_b),
                path::Prepared::new(&after, ai, &fam_a),
            )
        })
        .collect();
    // Every stratum verdict can stand alone as a finding: k = paths × strata (§10.7).
    let k: usize = paths
        .iter()
        .map(|(_, b, a)| path::strata(b, a).0.len())
        .sum();
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
        k: (k as u32).max(1),
        replicates: args.replicates as usize,
        seed: args.seed.unwrap_or(time::realtime_ns() as u64),
        block_override: None,
    };
    text::header(args, &plan, overlap);
    let all: Vec<&RunData> = before_refs.iter().chain(&after_refs).copied().collect();
    for (t, families) in strata::mixed_instance_types(&all) {
        println!(
            "! instance type {t} occurs with several NIC families ({}): results are \
             stratified by observed family, never by instance type",
            families.join(", ")
        );
    }
    for (p, b, a) in &paths {
        if b.runs.is_empty() || a.runs.is_empty() {
            println!("\n== path {p} ==  only on one side: not compared");
            continue;
        }
        path::report(p, b, a, &plan);
    }
    text::context(&checks::fields(&before_refs, &after_refs), &mismatches);
    Ok(())
}
