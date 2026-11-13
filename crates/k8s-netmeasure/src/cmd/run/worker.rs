//! The measuring-thread side of `run` (design §3.4, §3.5): one cycle per job, paced
//! with absolute deadlines, round-robin over the targets with every n-th slot on the
//! loopback control. Nothing here logs or allocates per sample beyond the
//! preallocated vectors; the shutdown flag is checked between slots.

use std::net::SocketAddrV4;

use netmeasure_core::measure::outcome::peer_closed;
use netmeasure_core::measure::pacing::{Next, Pacer};
use netmeasure_core::measure::plan::{SlotPlan, SlotTarget};
use netmeasure_core::measure::record::{Sample, flag};

use crate::measure::connect;
use crate::shutdown::Shutdown;
use crate::sys::time;

/// One cycle: global slots `first_slot .. first_slot + slots`.
#[derive(Debug, Clone, Copy)]
pub struct Job {
    pub first_slot: u64,
    pub slots: u64,
}

pub struct CycleRun {
    pub samples: Vec<Sample>,
    /// Monotonic ns of the cycle's first scheduled slot.
    pub origin_ns: u64,
    pub overruns: u64,
    pub interrupted: bool,
}

pub struct Worker {
    pub plan: SlotPlan,
    /// Index 0 = loopback control, `i + 1` = real target `i`.
    pub addrs: Vec<SocketAddrV4>,
    pub interval_ns: u64,
    pub timeout_ns: u64,
    pub warmup: u64,
    /// Samples taken so far per target index, for the warm-up flag.
    pub seen: Vec<u64>,
    pub shutdown: Shutdown,
}

impl Worker {
    pub fn run_cycle(&mut self, job: Job) -> CycleRun {
        let origin_ns = time::monotonic_ns();
        let mut pacer = Pacer::new(origin_ns, self.interval_ns);
        let mut out = CycleRun {
            samples: Vec::with_capacity(job.slots as usize),
            origin_ns,
            overruns: 0,
            interrupted: false,
        };
        for k in 0..job.slots {
            if self.shutdown.requested().is_some() {
                out.interrupted = true;
                break;
            }
            if let Next::At(deadline) = pacer.next() {
                time::sleep_until(deadline);
            }
            let timing = pacer.begin(time::monotonic_ns());
            let index = match self.plan.target(job.first_slot + k) {
                SlotTarget::Loopback => 0,
                SlotTarget::Real(i) => i as usize + 1,
            };
            let a = connect::attempt(self.addrs[index], self.timeout_ns);
            let overran = pacer.end(time::monotonic_ns());
            out.overruns += u64::from(overran);
            let warm = self.seen[index] < self.warmup;
            self.seen[index] += 1;
            let flags = [
                (a.sync_connect, flag::SYNC_CONNECT),
                (warm, flag::WARMUP),
                (timing.after_overrun, flag::AFTER_OVERRUN),
                (a.eintr > 0, flag::EINTR),
                (peer_closed(a.tcp_info.state), flag::PEER_CLOSED),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .fold(0, |acc, (_, bit)| acc | bit);
            out.samples.push(Sample {
                target: index as u16,
                late_ns: timing.late_ns,
                duration_ns: a.duration_ns,
                outcome: a.outcome,
                syn_retrans: a.tcp_info.total_retrans.known(),
                flags,
                errno: a.errno,
                reanchored_at_ns: timing
                    .after_overrun
                    .then(|| timing.scheduled_ns - origin_ns),
            });
        }
        out
    }
}
