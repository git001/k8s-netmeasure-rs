//! `report` (design §10.4–§10.7), first step: the sequential design with the factor
//! `kernel`. Per path and NIC stratum (source family → target family) one comparison,
//! never pooled across strata, the source nodes as the top resampling level; the
//! loopback control on its median per path; the context of both sides is listed and
//! a changed NIC family or CPU model per node refuses (exit 5). The verdict never
//! sets the exit code.

mod block;
mod checks;
mod evidence;
mod fmt;
mod load;
mod nic;
mod nic_compare;
mod params;
mod path;
mod strata;
mod text;

use std::process::ExitCode;

use netmeasure_cli::ReportArgs;
use netmeasure_cli::report::{Design, Factor, MarginArg};
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

/// The declared plan with `k` verdicts in the family.
fn plan(args: &ReportArgs, k: usize) -> Plan {
    let mut quantiles = args.quantiles.clone();
    quantiles.push(args.primary);
    quantiles.sort_by(f64::total_cmp);
    quantiles.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    Plan {
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
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
    }
}

/// The declared design, factor and inputs must fit together (exit 2 otherwise).
fn declared(args: &ReportArgs) -> Result<(), String> {
    match (args.factor, args.design) {
        (Factor::Nic, Design::Concurrent) if !args.runs.is_empty() => Ok(()),
        (Factor::Nic, _) => Err("--factor nic needs --design concurrent and --runs".into()),
        (_, Design::Sequential) if !args.before.is_empty() && !args.after.is_empty() => Ok(()),
        (f, _) => Err(format!(
            "--factor {} needs --design sequential, --before and --after",
            f.name()
        )),
    }
}

fn report(args: &ReportArgs) -> Result<(), (u8, String)> {
    let usage = |e: String| (exit::USAGE, e);
    declared(args).map_err(usage)?;
    if args.factor == Factor::Nic {
        return nic::report(args);
    }
    let mut before = load::load(&args.before).map_err(usage)?;
    let mut after = load::load(&args.after).map_err(usage)?;
    if before.is_empty() || after.is_empty() {
        return Err(usage("no result files on one side".into()));
    }
    for side in [&mut before, &mut after] {
        let families = strata::node_families(&side.iter().collect::<Vec<_>>());
        side.iter_mut().for_each(|r| strata::assign(r, &families));
    }
    let before_refs: Vec<&RunData> = before.iter().collect();
    let after_refs: Vec<&RunData> = after.iter().collect();

    let overlap =
        checks::sequential(&before_refs, &after_refs).map_err(|e| (exit::COMPARISON_REFUSED, e))?;
    let mismatches = checks::node_mismatches(&before_refs, &after_refs, args.factor);
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
                "context changed on a node, which measures the host, not the {}: {} \
                 (accept with --allow-context-mismatch)",
                args.factor.name(),
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
    let paths: Vec<(&String, path::Prepared, path::Prepared)> = groups
        .iter()
        .map(|(p, (bi, ai))| {
            (
                p,
                path::Prepared::new(&before, bi),
                path::Prepared::new(&after, ai),
            )
        })
        .collect();
    // Every stratum verdict can stand alone as a finding: k = paths × strata (§10.7).
    let k: usize = paths
        .iter()
        .map(|(_, b, a)| path::strata(b, a).0.len())
        .sum();
    let plan = plan(args, k);
    let validated = format!("the sides overlap by {:.1} s", overlap as f64 / 1e9);
    text::header(args, &plan, &validated);
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
    text::context(
        &checks::fields(&before_refs, &after_refs, args.factor),
        &mismatches,
    );
    Ok(())
}
