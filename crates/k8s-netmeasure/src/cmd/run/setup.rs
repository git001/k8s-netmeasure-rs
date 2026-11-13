//! Everything `run` decides before measuring (design §3.4, §3.5): slot plan, effective
//! cycles and their slot counts, labels, targets and the recorded configuration.

use std::collections::BTreeMap;

use netmeasure_cli::{PathKind, RunArgs, labels};
use netmeasure_core::measure::cycles::{self, Request};
use netmeasure_core::measure::plan::{CycleInput, SlotPlan, cycle_slots};
use netmeasure_core::schema::result::Target;
use netmeasure_util::time::as_nanos_u64;

pub struct Setup {
    pub plan: SlotPlan,
    pub cycle_slots: Vec<u64>,
    pub labels: BTreeMap<String, String>,
}

/// Default `--cycles` (§3.5).
const DEFAULT_CYCLES: u64 = 10;

pub fn prepare(args: &RunArgs) -> Result<Setup, String> {
    if args.target.len() >= usize::from(u16::MAX) {
        return Err(format!("at most {} targets", u16::MAX - 1));
    }
    let plan = SlotPlan::new(args.target.len() as u32, args.loopback_every)?;
    let input = |cycles| CycleInput {
        samples: args.samples,
        warmup: args.warmup,
        cycles,
    };
    let total: u64 = cycle_slots(&plan, &input(1))?.iter().sum();
    let request = match args.cycle {
        Some(len) => Request::Length(as_nanos_u64(len)),
        None => Request::Count(args.cycles.unwrap_or(DEFAULT_CYCLES)),
    };
    let run_ns = total.saturating_mul(as_nanos_u64(args.interval));
    let effective = cycles::effective(request, run_ns, args.samples);
    Ok(Setup {
        plan,
        cycle_slots: cycle_slots(&plan, &input(effective))?,
        labels: labels::normalise(&args.label)?,
    })
}

/// Index 0 is the loopback control.
pub fn targets(args: &RunArgs, loopback: &str) -> Vec<Target> {
    let lo = Target {
        index: 0,
        name: "loopback".into(),
        addr: loopback.into(),
        kind: "loopback".into(),
    };
    std::iter::once(lo)
        .chain(args.target.iter().enumerate().map(|(i, a)| Target {
            index: i as u16 + 1,
            name: a.to_string(),
            addr: a.to_string(),
            kind: "listener".into(),
        }))
        .collect()
}

/// `(path, path_source)`: declared by `--path`, else detected from whether physical
/// interfaces are visible.
pub fn path(args: &RunArgs, nic_visible: bool) -> (String, String) {
    let name = |p| match p {
        PathKind::Overlay => "overlay",
        PathKind::Underlay => "underlay",
    };
    match args.path {
        Some(p) => {
            if (p == PathKind::Underlay) != nic_visible {
                tracing::warn!(
                    declared = name(p),
                    nic_visible,
                    "--path differs from what this network namespace shows"
                );
            }
            (name(p).into(), "declared".into())
        }
        None if nic_visible => ("underlay".into(), "detected".into()),
        None => ("overlay".into(), "detected".into()),
    }
}
