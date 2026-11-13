//! `run` (design §3, §5, §8): paced connects to the targets and the own loopback
//! control, cut into cycles; around each cycle the main thread takes snapshots and
//! decides its validity (§3.6). One result file at the end. Exit 0 if every cycle is
//! valid, 4 otherwise, 3 if the CPU checks refuse the start (§3.7), `128 + signal`
//! after SIGTERM/SIGINT with the file marked `interrupted`.

mod cycle;
mod file;
mod session;
mod setup;
mod snapshot;
mod worker;

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;

use netmeasure_cli::RunArgs;
use netmeasure_core::schema::result::{Clock, Config, RunStatus};
use netmeasure_util::time::{as_nanos_u64, rfc3339_ns};

use self::session::Session;
use self::worker::Worker;
use crate::diag::context::{self, Declared};
use crate::exit;
use crate::listener::Listener;
use crate::shutdown::{self, Shutdown};
use crate::sys::time;

pub fn run(args: &RunArgs, sources: &BTreeMap<String, &'static str>) -> ExitCode {
    let setup = match setup::prepare(args) {
        Ok(s) => s,
        Err(e) => return refuse(exit::USAGE, "invalid run configuration", &e),
    };
    let dir = args.output.parent().filter(|p| !p.as_os_str().is_empty());
    if dir.is_some_and(|d| !d.is_dir()) {
        return refuse(
            exit::REFUSED,
            "cannot write --output",
            "the directory does not exist",
        );
    }
    let cgroup = match snapshot::cgroup_check(args.no_cgroup_check) {
        Ok(c) => c,
        Err(e) => return refuse(exit::REFUSED, "CPU check failed", &e),
    };
    let shutdown = match Shutdown::install() {
        Ok(s) => s,
        Err(e) => {
            return refuse(
                exit::INTERNAL,
                "cannot set up signal handling",
                &e.to_string(),
            );
        }
    };
    let lo = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let listener = match Listener::start(lo, args.backlog) {
        Ok(l) => l,
        Err(e) => {
            return refuse(
                exit::REFUSED,
                "cannot bind the loopback control listener",
                &e.to_string(),
            );
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
        cycles = setup.cycle_slots.len(), slots = setup.cycle_slots.iter().sum::<u64>(),
        quota_millicores = cgroup.quota_millicores, ancestor_cpu_throttling = %cgroup.ancestor_cpu_throttling,
        "run started");

    let worker = Worker {
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
    let measured = session::measure(
        &Session {
            setup: &setup,
            targets: &targets,
            path: &path,
            outlier_ns: as_nanos_u64(args.outlier_threshold),
            cgroup_unchecked: args.no_cgroup_check,
            listener: listener.handle(),
            shutdown: &shutdown,
            origin_ns,
            wall: &wall,
        },
        worker,
    );
    drop(listener);

    let signal = shutdown.requested();
    let all_valid = measured.cycles.len() == setup.cycle_slots.len()
        && measured.cycles.iter().all(|c| c.validity.valid);
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
        },
        labels: setup.labels.clone(),
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
            no_cgroup_check: args.no_cgroup_check,
        },
        config_source: sources
            .iter()
            .map(|(k, v)| (k.clone(), (*v).to_owned()))
            .collect(),
        cgroup,
        context_after: Some(context::read_context(&declared)),
        context_before,
        targets,
        cycles: measured.cycles,
        samples: measured.samples,
        diagnostics: measured.diagnostics,
    };
    if let Err(e) = file::write(&args.output, finished) {
        return refuse(
            exit::INTERNAL,
            "cannot write the result file",
            &e.to_string(),
        );
    }
    tracing::info!(run_id = %run_id, output = %args.output.display(), all_valid, "run finished");
    match signal {
        Some(s) => exit::code(u8::try_from(shutdown::exit_status(s)).unwrap_or(exit::INTERNAL)),
        None if all_valid => exit::code(exit::OK),
        None => exit::code(exit::INVALID_CYCLES),
    }
}

fn refuse(code: u8, what: &str, why: &str) -> ExitCode {
    tracing::error!(error = %why, "{what}");
    exit::code(code)
}
