//! `run` (design §3, §5, §8): paced connects to the targets and the own loopback
//! control, cut into cycles; around each cycle the main thread takes snapshots and
//! decides its validity (§3.6). One result file at the end. Exit 0 if every cycle is
//! valid, 4 otherwise, 3 if a start check refuses (`start.rs`), `128 + signal` after
//! SIGTERM/SIGINT with the file marked `interrupted`.

mod control;
mod cycle;
mod file;
mod outpath;
mod remote;
mod session;
mod setup;
mod snapshot;
mod start;
mod worker;

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::Arc;

use netmeasure_cli::RunArgs;
use netmeasure_core::schema::result::{Clock, RunStatus};
use netmeasure_util::time::{as_nanos_u64, rfc3339_ns};

use self::session::Session;
use self::worker::Worker;
use crate::diag::context;
use crate::exit;
use crate::shutdown;
use crate::sys::time;

pub fn run(args: &RunArgs, sources: &BTreeMap<String, &'static str>) -> ExitCode {
    let s = match start::start(args) {
        Ok(s) => s,
        Err((code, message)) => return refuse(code, &message),
    };
    let start::Started {
        setup,
        cgroup,
        shutdown,
        mut client,
        remotes,
        mut node,
        listener,
        declared,
        context_before,
        path: (path, path_source),
        run_id,
        output,
        beats,
        control,
    } = s;
    let mut targets = setup::targets(args, &listener.local.to_string(), &remotes);
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
        shutdown_deadline_ns: as_nanos_u64(args.shutdown_deadline),
        heartbeat: Arc::clone(&beats.measure),
    };
    let session = Session {
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
        main_beat: &beats.main,
        wall: &wall,
    };
    let measured = session::measure(&session, &mut client, node.as_mut(), worker);
    if let Some(c) = &control {
        c.stopping();
    }
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
        config: setup::config(
            args,
            &setup,
            &targets[0].addr,
            (path, path_source),
            control.as_ref().map(control::RunControl::addr),
        ),
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
    let written = file::write(&output, finished);
    if let Some(c) = control {
        c.stop();
    }
    if let Err(e) = written {
        return refuse(
            exit::INTERNAL,
            &format!("cannot write the result file: {e}"),
        );
    }
    tracing::info!(run_id = %run_id, output = %output.display(), all_valid, "run finished");
    match signal {
        Some(s) => exit::code(u8::try_from(shutdown::exit_status(s)).unwrap_or(exit::INTERNAL)),
        None if all_valid => exit::code(exit::OK),
        None => exit::code(exit::INVALID_CYCLES),
    }
}

fn refuse(code: u8, message: &str) -> ExitCode {
    tracing::error!("{message}");
    exit::code(code)
}
