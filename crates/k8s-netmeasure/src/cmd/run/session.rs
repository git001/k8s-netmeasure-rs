//! The cycle loop of `run` (design §3.5): before-snapshot → the measuring thread runs
//! the cycle → after-snapshot → validity. The measuring thread lives for the whole run
//! and gets one job per cycle.

use std::sync::mpsc;

use netmeasure_core::measure::record::Sample;
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::result::{Cycle, CycleDiagnostics, Target};

use super::remote::Remote;
use super::snapshot::{self, Snap};
use super::worker::{CycleRun, Job, Worker};
use super::{cycle, setup::Setup};
use crate::control::client::Client;
use crate::listener::Handle;
use crate::measure::thread;
use crate::shutdown::Shutdown;
use crate::sys::time;

pub struct Session<'a> {
    pub setup: &'a Setup,
    pub targets: &'a [Target],
    pub path: &'a str,
    pub outlier_ns: u64,
    pub cgroup_unchecked: bool,
    pub listener: Handle,
    pub remotes: &'a [Remote],
    pub targets_unchecked: bool,
    pub shutdown: &'a Shutdown,
    pub origin_ns: u64,
    /// Monotonic ns → RFC 3339.
    pub wall: &'a dyn Fn(u64) -> String,
}

#[derive(Default)]
pub struct Outcome {
    pub cycles: Vec<Cycle>,
    pub diagnostics: Vec<CycleDiagnostics>,
    pub samples: Vec<Sample>,
}

pub fn measure(s: &Session<'_>, client: &mut Client, worker: Worker) -> Outcome {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (done_tx, done_rx) = mpsc::channel::<CycleRun>();
    let mut worker = worker;
    let measuring = thread::spawn(move |_setup| {
        for job in job_rx {
            if done_tx.send(worker.run_cycle(job)).is_err() {
                break;
            }
        }
    });
    let mut out = Outcome::default();
    let (mut first_slot, mut nic_first) = (0, None);
    let mut remote_nic_first = vec![None; s.remotes.len()];
    for (index, &slots) in s.setup.cycle_slots.iter().enumerate() {
        if s.shutdown.requested().is_some() {
            break;
        }
        let before = snapshot::take(&s.listener, client, s.remotes);
        let started = time::monotonic_ns();
        if job_tx.send(Job { first_slot, slots }).is_err() {
            break;
        }
        let Ok(run) = done_rx.recv() else { break };
        let ended = time::monotonic_ns();
        let after = snapshot::take(&s.listener, client, s.remotes);
        if index == 0 {
            nic_first = before.nic.value.clone();
            for (first, snap) in remote_nic_first.iter_mut().zip(&before.targets) {
                *first = snap.as_ref().and_then(|t| t.nic.value.clone());
            }
        }
        let (c, diag) = record(
            s,
            &CycleCtx {
                index,
                slots,
                started,
                ended,
                first_sample: out.samples.len(),
            },
            &run,
            &before,
            &after,
            (nic_first.as_ref(), &remote_nic_first),
        );
        cycle::log(&c, &run, s.targets, s.path, s.outlier_ns);
        first_slot += slots;
        out.samples.extend(run.samples);
        out.cycles.push(c);
        out.diagnostics.push(diag);
        if run.interrupted {
            break;
        }
    }
    drop(job_tx);
    let _ = measuring.join();
    out
}

struct CycleCtx {
    index: usize,
    slots: u64,
    started: u64,
    ended: u64,
    first_sample: usize,
}

fn record(
    s: &Session<'_>,
    c: &CycleCtx,
    run: &CycleRun,
    before: &Snap,
    after: &Snap,
    (nic_first, remote_nic_first): (Option<&NicPath>, &[Option<NicPath>]),
) -> (Cycle, CycleDiagnostics) {
    cycle::record(cycle::Done {
        index: c.index as u64,
        first_sample: c.first_sample as u64,
        planned_slots: c.slots,
        run_origin_ns: s.origin_ns,
        started_at: (s.wall)(c.started),
        ended_at: (s.wall)(c.ended),
        run,
        before,
        after,
        nic_first,
        cgroup_unchecked: s.cgroup_unchecked,
        remotes: s.remotes,
        remote_nic_first,
        targets_unchecked: s.targets_unchecked,
    })
}
