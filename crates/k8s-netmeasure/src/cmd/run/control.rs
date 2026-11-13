//! `run --control-port` (design §4.1, §7): probes and `/v1/*` for a long `run` Job.
//! Off by default, so a measurement never fails because a port is taken; a port that
//! is set and cannot be bound is exit 3. `/v1/info` carries the `run_id` as
//! `listener_instance`; the measured listener is the loopback control.

use std::net::SocketAddrV4;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use netmeasure_cli::{ControlBind, RunArgs};
use netmeasure_util::time::as_nanos_u64;

use crate::control::Server;
use crate::control::endpoints::{self, NicCache};
use crate::control::state::{Heartbeat, State};
use crate::diag::net;
use crate::listener::Listener;

/// Slack for snapshots around a cycle (control-port requests with 2 s timeouts) and
/// for writing the file, on top of the computed bounds (§7: + 30 s for main).
const SLACK_NS: u64 = 30_000_000_000;

pub struct RunControl {
    server: Server,
    state: Arc<State>,
}

pub struct Beats {
    pub main: Arc<AtomicU64>,
    pub measure: Arc<AtomicU64>,
}

/// `Ok(None)` without `--control-port`.
pub fn start(
    args: &RunArgs,
    listener: &Listener,
    run_id: &str,
    longest_cycle_slots: u64,
    beats: &Beats,
) -> Result<Option<RunControl>, String> {
    let Some(port) = args.control_port else {
        return Ok(None);
    };
    let ip = match args.control_bind {
        ControlBind::Addr(ip) => ip,
        ControlBind::Auto => net::default_route_ipv4().ok_or(
            "--control-bind auto: no interface with an IPv4 default route; set --control-bind",
        )?,
    };
    let (interval, timeout) = (as_nanos_u64(args.interval), as_nanos_u64(args.timeout));
    let watched = |name, beat: &Arc<AtomicU64>, bound_ns| {
        let b = Arc::clone(beat);
        Heartbeat {
            name,
            read: Box::new(move || b.load(Ordering::Relaxed)),
            bound_ns,
        }
    };
    let heartbeats = vec![
        // §7: measuring thread 2 × timeout + interval; it waits between cycles.
        watched("measure", &beats.measure, 2 * timeout + interval + SLACK_NS),
        // §7: main thread cycle duration + snapshot timeouts + 30 s.
        watched(
            "main",
            &beats.main,
            longest_cycle_slots.saturating_mul(interval) + SLACK_NS,
        ),
    ];
    let nic: NicCache = None;
    let body = endpoints::endpoints(
        args.backlog,
        &args.context,
        listener,
        nic,
        run_id.to_owned(),
    );
    let state = Arc::new(State::new(body, heartbeats));
    let addr = SocketAddrV4::new(ip, port);
    let server = Server::start(addr, Arc::clone(&state))
        .map_err(|e| format!("cannot bind the control port {addr}: {e}"))?;
    state.mark_started();
    tracing::info!(control = %server.local, "control port serving");
    Ok(Some(RunControl { server, state }))
}

impl RunControl {
    pub fn addr(&self) -> String {
        self.server.local.to_string()
    }

    /// `/ready` turns 503 (§8).
    pub fn stopping(&self) {
        self.state.mark_stopping();
    }

    pub fn stop(self) {
        self.server.stop();
    }
}
