//! After a cycle (design §3.5, §3.6, §9.3): validity, quality figures, the cycle record
//! and its log lines. Runs on the main thread, never inside the timed section.

use netmeasure_core::measure::outcome::Outcome;
use netmeasure_core::measure::record::flag;
use netmeasure_core::measure::validity::{self, CycleFacts};
use netmeasure_core::schema::result::{Cycle, Quality, Quantiles, Target};
use netmeasure_core::stats::quantile::type1;

use super::worker::CycleRun;

pub struct Done<'a> {
    pub index: u64,
    pub first_sample: u64,
    pub planned_slots: u64,
    pub run_origin_ns: u64,
    pub started_at: String,
    pub ended_at: String,
    pub run: &'a CycleRun,
}

pub fn record(d: Done<'_>) -> Cycle {
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
    });
    Cycle {
        cycle_id: ulid::Ulid::generate().to_string(),
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
        },
    }
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
