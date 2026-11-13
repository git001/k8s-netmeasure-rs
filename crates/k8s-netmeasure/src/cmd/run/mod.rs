//! `run` (design §3, §5, §8): paced connects to the targets and the own loopback
//! control, cut into cycles; around each cycle the main thread takes snapshots and
//! decides its validity (§3.6). One result file at the end. Exit 0 if every cycle is
//! valid, 4 otherwise, 3 if the CPU checks refuse the start (§3.7), `128 + signal`
//! after SIGTERM/SIGINT with the file marked `interrupted`.

mod cycle;
mod file;
mod remote;
mod session;
mod setup;
mod snapshot;
mod worker;

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;

use netmeasure_cli::RunArgs;
use netmeasure_core::schema::result::{Clock, RunStatus};
use netmeasure_util::time::{as_nanos_u64, rfc3339_ns};

use self::session::Session;
use self::worker::Worker;
use crate::control::client::Client;
use crate::diag::context::{self, Declared};
use crate::diag::node_listener;
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
    let mut client = match Client::new() {
        Ok(c) => c,
        Err(e) => {
            return refuse(
                exit::INTERNAL,
                "cannot start the control client",
                &e.to_string(),
            );
        }
    };
    let remote::Peers { remotes, node } = match remote::connect_peers(args, &mut client) {
        Ok(p) => p,
        Err(e) => return refuse(exit::REFUSED, "peer check failed", &e),
    };
    let (mut node, node_context) = node.unzip();
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
    let own = context::read_context(&declared);
    let (path, path_source) = setup::path(args, own.nic.value.is_some());
    let context_before = match &node_context {
        Some(n) => node_listener::with_node_nic(own, n),
        None => own,
    };
    let mut targets = setup::targets(args, &listener.local.to_string(), &remotes);
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
            remotes: &remotes,
            targets_unchecked: args.no_target_control,
            shutdown: &shutdown,
            origin_ns,
            wall: &wall,
        },
        &mut client,
        node.as_mut(),
        worker,
    );
    for r in &remotes {
        targets[usize::from(r.index)].context_after = remote::context_after(&mut client, r);
    }
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
        config: setup::config(args, &setup, &targets[0].addr, path, path_source),
        config_source: sources
            .iter()
            .map(|(k, v)| (k.clone(), (*v).to_owned()))
            .collect(),
        cgroup,
        context_after: Some(match node.as_mut() {
            Some(n) => n.context_after(&mut client, context::read_context(&declared)),
            None => context::read_context(&declared),
        }),
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
