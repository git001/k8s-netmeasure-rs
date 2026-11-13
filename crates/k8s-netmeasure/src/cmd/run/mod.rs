//! `run` (design §3, §5, §8): paced connects to the targets and the own loopback
//! control, cut into cycles, each validated; one result file at the end. The measuring
//! thread gets one job per cycle, so the main thread can take snapshots between
//! cycles (milestone 3c2). Exit 0 if every cycle is valid, 4 otherwise, `128 + signal`
//! after SIGTERM/SIGINT with the file marked `interrupted`.

mod cycle;
mod file;
mod setup;
mod worker;

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;
use std::sync::mpsc;

use netmeasure_cli::RunArgs;
use netmeasure_core::schema::result::{Clock, Config, RunStatus};
use netmeasure_util::time::{as_nanos_u64, rfc3339_ns};

use self::worker::{CycleRun, Job, Worker};
use crate::diag::context::{self, Declared};
use crate::exit;
use crate::listener::Listener;
use crate::measure::thread;
use crate::shutdown::{self, Shutdown};
use crate::sys::time;

pub fn run(args: &RunArgs, sources: &BTreeMap<String, &'static str>) -> ExitCode {
    let setup = match setup::prepare(args) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "invalid run configuration");
            return exit::code(exit::USAGE);
        }
    };
    let dir = args.output.parent().filter(|p| !p.as_os_str().is_empty());
    if dir.is_some_and(|d| !d.is_dir()) {
        tracing::error!(output = %args.output.display(), "the directory of --output does not exist");
        return exit::code(exit::REFUSED);
    }
    let shutdown = match Shutdown::install() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "cannot set up signal handling");
            return exit::code(exit::INTERNAL);
        }
    };
    let lo = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let listener = match Listener::start(lo, args.backlog) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "cannot bind the loopback control listener");
            return exit::code(exit::REFUSED);
        }
    };
    let declared = Declared::from_args(&args.context);
    let context_before = context::read_context(&declared);
    let (path, path_source) = setup::path(args, context_before.nic.value.is_some());
    let targets = setup::targets(args, &listener.local.to_string());
    let run_id = ulid::Ulid::generate().to_string();
    let (origin_ns, origin_wall) = (time::monotonic_ns(), time::realtime_ns());
    let wall = |mono: u64| rfc3339_ns(origin_wall + i128::from(mono.saturating_sub(origin_ns)));
    tracing::info!(run_id = %run_id, targets = args.target.len(), path = %path,
        cycles = setup.cycle_slots.len(), slots = setup.cycle_slots.iter().sum::<u64>(), "run started");

    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (done_tx, done_rx) = mpsc::channel::<CycleRun>();
    let mut worker = Worker {
        plan: setup.plan,
        addrs: std::iter::once(listener.local)
            .chain(args.target.iter().copied())
            .collect(),
        interval_ns: as_nanos_u64(args.interval),
        timeout_ns: as_nanos_u64(args.timeout),
        warmup: args.warmup,
        seen: vec![0; args.target.len() + 1],
        shutdown: shutdown.clone(),
    };
    let measuring = thread::spawn(move |_setup| {
        for job in job_rx {
            if done_tx.send(worker.run_cycle(job)).is_err() {
                break;
            }
        }
    });

    let (mut cycles, mut samples, mut first_slot) = (Vec::new(), Vec::new(), 0);
    for (index, &slots) in setup.cycle_slots.iter().enumerate() {
        if shutdown.requested().is_some() {
            break;
        }
        let started = time::monotonic_ns();
        let Ok(run) = job_tx
            .send(Job { first_slot, slots })
            .map(|()| done_rx.recv())
        else {
            break;
        };
        let Ok(run) = run else { break };
        let c = cycle::record(cycle::Done {
            index: index as u64,
            first_sample: samples.len() as u64,
            planned_slots: slots,
            run_origin_ns: origin_ns,
            started_at: wall(started),
            ended_at: wall(time::monotonic_ns()),
            run: &run,
        });
        cycle::log(
            &c,
            &run,
            &targets,
            &path,
            as_nanos_u64(args.outlier_threshold),
        );
        first_slot += slots;
        samples.extend(run.samples);
        cycles.push(c);
        if run.interrupted {
            break;
        }
    }
    drop(job_tx);
    let _ = measuring.join();
    drop(listener);

    let signal = shutdown.requested();
    let all_valid =
        cycles.len() == setup.cycle_slots.len() && cycles.iter().all(|c| c.validity.valid);
    let finished = file::Finished {
        run_id: run_id.clone(),
        status: if signal.is_some() {
            RunStatus::Interrupted
        } else {
            RunStatus::Complete
        },
        started_at: wall(origin_ns),
        ended_at: wall(time::monotonic_ns()),
        clock: Clock {
            realtime_at_origin: rfc3339_ns(origin_wall),
            boot_id: context_before.boot_id.value.clone(),
            synchronized: None,
        },
        labels: setup.labels,
        config: Config {
            samples: args.samples,
            warmup: args.warmup,
            interval_ns: as_nanos_u64(args.interval),
            loopback_every: args.loopback_every,
            cycles: setup.cycle_slots.len() as u64,
            cycle_slots: setup.cycle_slots.clone(),
            timeout_ns: as_nanos_u64(args.timeout),
            outlier_threshold_ns: as_nanos_u64(args.outlier_threshold),
            listener: targets[0].addr.clone(),
            path,
            path_source,
            topology: if args.target.len() == 1 {
                "pair"
            } else {
                "sampled"
            }
            .into(),
        },
        config_source: sources
            .iter()
            .map(|(k, v)| (k.clone(), (*v).to_owned()))
            .collect(),
        context_after: Some(context::read_context(&declared)),
        context_before,
        targets,
        cycles,
        samples,
    };
    if let Err(e) = file::write(&args.output, finished) {
        tracing::error!(output = %args.output.display(), error = %e, "cannot write the result file");
        return exit::code(exit::INTERNAL);
    }
    tracing::info!(run_id = %run_id, output = %args.output.display(), all_valid, "run finished");
    match signal {
        Some(s) => exit::code(u8::try_from(shutdown::exit_status(s)).unwrap_or(exit::INTERNAL)),
        None if all_valid => exit::code(exit::OK),
        None => exit::code(exit::INVALID_CYCLES),
    }
}
