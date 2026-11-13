//! After a cycle (design §3.5, §3.6, §9.3): validity, quality figures, the cycle record
//! and its log lines. Runs on the main thread, never inside the timed section.

use netmeasure_core::diag::netstat;
use netmeasure_core::measure::outcome::Outcome;
use netmeasure_core::measure::record::flag;
use netmeasure_core::measure::validity::{self, CycleFacts, delta};
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::result::{
    CpuDelta, Cycle, CycleDiagnostics, ListenerDrops, Quality, Quantiles, SourceDiagnostics, Target,
};
use netmeasure_core::stats::quantile::type1;

use super::remote::{self, Remote};
use super::snapshot::Snap;
use super::worker::CycleRun;

pub struct Done<'a> {
    pub index: u64,
    pub first_sample: u64,
    pub planned_slots: u64,
    pub run_origin_ns: u64,
    pub started_at: String,
    pub ended_at: String,
    pub run: &'a CycleRun,
    pub before: &'a Snap,
    pub after: &'a Snap,
    pub nic_first: Option<&'a NicPath>,
    pub cgroup_unchecked: bool,
    pub remotes: &'a [Remote],
    /// NIC path of every remote in the run's first cycle.
    pub remote_nic_first: &'a [Option<NicPath>],
    pub targets_unchecked: bool,
}

/// The cycle record and its diagnostics entry.
pub fn record(d: Done<'_>) -> (Cycle, CycleDiagnostics) {
    let mut late: Vec<u64> = d.run.samples.iter().map(|s| s.late_ns).collect();
    late.sort_unstable();
    let invalid = d
        .run
        .samples
        .iter()
        .filter(|s| s.outcome.invalidates_cycle())
        .count();
    let validity = validity::evaluate(&CycleFacts {
        planned_slots: d.planned_slots,
        recorded: d.run.samples.len() as u64,
        invalid_samples: invalid as u64,
        overruns: d.run.overruns,
        lateness_sorted: late.clone(),
        interrupted: d.run.interrupted,
        before: d.before.counters,
        after: d.after.counters,
        cgroup_unchecked: d.cgroup_unchecked,
        nic_before: d.before.nic.clone(),
        nic_after: d.after.nic.clone(),
        nic_first: d.nic_first.cloned(),
        targets: d
            .remotes
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (b, a) = (d.before.targets[i].as_ref(), d.after.targets[i].as_ref());
                remote::cycle(r, b, a, d.remote_nic_first[i].as_ref())
            })
            .collect(),
        targets_unchecked: d.targets_unchecked,
        node_listener: [d.before.node_listener, d.after.node_listener],
    });
    let (target_diag, target_drops): (Vec<_>, Vec<_>) = (0..d.remotes.len())
        .map(|i| remote::diagnostics(d.before.targets[i].as_ref(), d.after.targets[i].as_ref()))
        .unzip();
    let (b, a) = (&d.before.counters, &d.after.counters);
    let cycle_id = ulid::Ulid::generate().to_string();
    let diagnostics = CycleDiagnostics {
        cycle_id: cycle_id.clone(),
        source: SourceDiagnostics {
            netstat: netstat::delta(&d.before.netstat, &d.after.netstat),
        },
        targets: remote::by_index(d.remotes, target_diag),
    };
    let cycle = Cycle {
        cycle_id,
        index: d.index,
        first_sample: d.first_sample,
        end_sample: d.first_sample + d.run.samples.len() as u64,
        start_offset_ns: d.run.origin_ns.saturating_sub(d.run_origin_ns),
        started_at: d.started_at,
        ended_at: d.ended_at,
        validity,
        quality: Quality {
            wake_lateness_ns: quantiles(&late),
            overruns: d.run.overruns,
            cpu: CpuDelta {
                throttled_usec_delta: delta(b.throttled_usec, a.throttled_usec),
                ancestors_throttled_usec_delta: delta(
                    b.ancestors_throttled_usec,
                    a.ancestors_throttled_usec,
                ),
            },
            listener_drops: ListenerDrops {
                source: delta(b.sk_drops, a.sk_drops),
                targets: remote::by_index(d.remotes, target_drops),
            },
        },
    };
    (cycle, diagnostics)
}

/// One INFO line per target (§9.3), WARN per outlier and for an invalid cycle.
pub fn log(cycle: &Cycle, run: &CycleRun, targets: &[Target], path: &str, outlier_ns: u64) {
    for t in targets {
        let mut ok: Vec<u64> = run
            .samples
            .iter()
            .filter(|s| {
                s.target == t.index && s.outcome == Outcome::Ok && s.flags & flag::WARMUP == 0
            })
            .map(|s| s.duration_ns)
            .collect();
        ok.sort_unstable();
        let total = run.samples.iter().filter(|s| s.target == t.index).count();
        tracing::info!(
            cycle_id = %cycle.cycle_id, cycle = cycle.index, target_node = %t.name, path,
            samples = total, ok_counted = ok.len(),
            p50_ns = type1(&ok, 0.5), p99_ns = type1(&ok, 0.99),
            "cycle"
        );
    }
    for s in run.samples.iter().filter(|s| s.duration_ns > outlier_ns) {
        let name = targets
            .get(usize::from(s.target))
            .map_or("?", |t| t.name.as_str());
        tracing::warn!(cycle_id = %cycle.cycle_id, target_node = %name, path, duration_ns = s.duration_ns, result = ?s.outcome, "outlier");
    }
    if !cycle.validity.valid {
        tracing::warn!(cycle_id = %cycle.cycle_id, cycle = cycle.index, reasons = ?cycle.validity.reasons, "invalid cycle");
    }
}

fn quantiles(sorted: &[u64]) -> Quantiles {
    let q = |p| type1(sorted, p).unwrap_or(0);
    Quantiles {
        p50: q(0.5),
        p90: q(0.9),
        p99: q(0.99),
        max: sorted.last().copied().unwrap_or(0),
    }
}
