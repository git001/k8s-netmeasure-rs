//! `listen` (design §4, §7): the measurement target. Start order (§4.1): bind the
//! measured port and start the accept thread, then bind the control port; only then
//! do `/startup` and `/ready` report 200. On SIGTERM or SIGINT `/ready` turns 503 at
//! once, the listener stops accepting and closes, the control server stops, and the
//! process exits with `128 + signal`.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use netmeasure_cli::{ControlBind, ListenArgs};
use serde_json::json;

use crate::build_info::BUILD_INFO;
use crate::control::Server;
use crate::control::state::{Heartbeat, State};
use crate::diag::net;
use crate::exit;
use crate::listener::Listener;
use crate::shutdown::{self, Shutdown};
use crate::sys::time;

/// How often the main thread checks for a shutdown signal.
const TICK: Duration = Duration::from_millis(100);
/// Liveness bounds (§7): accept thread 5 s (it beats at least every second); the main
/// thread of `listen` only waits for signals, so the same bound applies.
const ACCEPT_BOUND_NS: u64 = 5_000_000_000;
const MAIN_BOUND_NS: u64 = 5_000_000_000;

pub fn run(args: &ListenArgs) -> ExitCode {
    if args.control_port == args.port {
        tracing::error!(port = args.port, "--control-port must differ from --port");
        return exit::code(exit::USAGE);
    }
    let shutdown = match Shutdown::install() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "cannot set up signal handling");
            return exit::code(exit::INTERNAL);
        }
    };
    let Some(control_ip) = control_ip(args.control_bind) else {
        tracing::error!(
            "--control-bind auto: no interface with an IPv4 default route; set --control-bind"
        );
        return exit::code(exit::REFUSED);
    };
    let addr = SocketAddrV4::new(args.bind, args.port);
    let listener = match Listener::start(addr, args.backlog) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(addr = %addr, error = %e, "cannot bind the measured port");
            return exit::code(exit::REFUSED);
        }
    };
    let main_beat = Arc::new(AtomicU64::new(time::monotonic_ns()));
    let state = Arc::new(State::new(
        info(args, &listener),
        heartbeats(&listener, &main_beat),
    ));
    let control_addr = SocketAddrV4::new(control_ip, args.control_port);
    let server = match Server::start(control_addr, Arc::clone(&state)) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(addr = %control_addr, error = %e, "cannot bind the control port");
            return exit::code(exit::REFUSED);
        }
    };
    state.mark_started();
    tracing::info!(
        addr = %listener.local,
        control = %server.local,
        backlog = args.backlog,
        effective_backlog = listener.effective_backlog,
        "listening"
    );
    let signal = loop {
        main_beat.store(time::monotonic_ns(), Ordering::Relaxed);
        if let Some(s) = shutdown.requested() {
            break s;
        }
        std::thread::sleep(TICK);
    };
    state.mark_stopping();
    let drops = listener.drops();
    let stats = listener.stop();
    server.stop();
    tracing::info!(
        signal,
        accepted = stats.accepted,
        aborted = stats.aborted,
        errors = stats.errors,
        listener_drops = drops.known(),
        "shut down"
    );
    exit::code(u8::try_from(shutdown::exit_status(signal)).unwrap_or(exit::INTERNAL))
}

fn control_ip(bind: ControlBind) -> Option<Ipv4Addr> {
    match bind {
        ControlBind::Addr(ip) => Some(ip),
        ControlBind::Auto => net::default_route_ipv4(),
    }
}

/// The `/v1/info` body (§7).
fn info(args: &ListenArgs, listener: &Listener) -> serde_json::Value {
    json!({
        "schema": "k8s-netmeasure/control", "schema_version": 1,
        "build_info": BUILD_INFO.to_json(),
        "listener_instance": ulid::Ulid::generate().to_string(),
        "measured": listener.local.to_string(),
        "backlog": args.backlog,
        "effective_backlog": listener.effective_backlog,
    })
}

fn heartbeats(listener: &Listener, main_beat: &Arc<AtomicU64>) -> Vec<Heartbeat> {
    let counters = listener.counters();
    let main = Arc::clone(main_beat);
    vec![
        Heartbeat {
            name: "accept",
            read: Box::new(move || counters.heartbeat_ns.load(Ordering::Relaxed)),
            bound_ns: ACCEPT_BOUND_NS,
        },
        Heartbeat {
            name: "main",
            read: Box::new(move || main.load(Ordering::Relaxed)),
            bound_ns: MAIN_BOUND_NS,
        },
    ]
}
