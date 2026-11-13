//! Everything `run` decides before measuring (design §3.4, §3.5): slot plan, effective
//! cycles and their slot counts, labels, targets and the recorded configuration.

use std::collections::BTreeMap;

use netmeasure_cli::{PathKind, RunArgs, labels};
use netmeasure_core::measure::cycles::{self, Request};
use netmeasure_core::measure::plan::{CycleInput, SlotPlan, cycle_slots};
use netmeasure_core::schema::result::{Config, Target};
use netmeasure_util::time::as_nanos_u64;

use super::remote::Remote;

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

/// Index 0 is the loopback control. Targets with a control port carry its address,
/// identity and the context read at start; the node name is the target's declared
/// node name when it has one, else its address.
pub fn targets(args: &RunArgs, loopback: &str, remotes: &[Remote]) -> Vec<Target> {
    let lo = Target {
        index: 0,
        name: "loopback".into(),
        addr: loopback.into(),
        kind: "loopback".into(),
        control: None,
        listener_instance: None,
        context_before: None,
        context_after: None,
    };
    std::iter::once(lo)
        .chain(args.target.iter().enumerate().map(|(i, a)| {
            let r = remotes.iter().find(|r| usize::from(r.index) == i + 1);
            Target {
                index: i as u16 + 1,
                name: r
                    .and_then(|r| r.context.node_name.value.clone())
                    .unwrap_or_else(|| a.to_string()),
                addr: a.to_string(),
                kind: "listener".into(),
                control: r.map(|r| r.control.to_string()),
                listener_instance: r.map(|r| r.listener_instance.clone()),
                context_before: r.map(|r| r.context.clone()),
                context_after: None,
            }
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

/// The recorded configuration (§5).
pub fn config(
    args: &RunArgs,
    setup: &Setup,
    listener: &str,
    path: String,
    path_source: String,
) -> Config {
    Config {
        samples: args.samples,
        warmup: args.warmup,
        interval_ns: as_nanos_u64(args.interval),
        loopback_every: args.loopback_every,
        cycles: setup.cycle_slots.len() as u64,
        cycle_slots: setup.cycle_slots.clone(),
        timeout_ns: as_nanos_u64(args.timeout),
        outlier_threshold_ns: as_nanos_u64(args.outlier_threshold),
        listener: listener.to_owned(),
        path,
        path_source,
        topology: if args.target.len() == 1 {
            "pair"
        } else {
            "sampled"
        }
        .into(),
        no_cgroup_check: args.no_cgroup_check,
        target_control_port: (!args.no_target_control).then_some(args.target_control_port),
    }
}
