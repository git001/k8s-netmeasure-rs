//! The control-port bodies of `listen` (design §7) and its watched threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use netmeasure_cli::ListenArgs;
use netmeasure_core::diag::cgroup;
use netmeasure_core::schema::control::{self, Info, ListenerCounters, NodeContext, Snapshot};

use super::node::{self, NicCache};
use crate::build_info::BUILD_INFO;
use crate::control::state::{Endpoints, Heartbeat};
use crate::diag::context::{self, Declared};
use crate::diag::net;
use crate::listener::{Handle, Listener};
use crate::sys::time;

/// Liveness bounds (§7): accept thread 5 s (it beats at least every second); the main
/// thread of `listen` only waits for signals, so the same bound applies.
const ACCEPT_BOUND_NS: u64 = 5_000_000_000;
const MAIN_BOUND_NS: u64 = 5_000_000_000;

pub fn endpoints(args: &ListenArgs, listener: &Listener, nic: NicCache) -> Endpoints {
    let info = Info {
        schema: control::SCHEMA.into(),
        schema_version: control::SCHEMA_VERSION,
        build_info: BUILD_INFO.schema(),
        listener_instance: ulid::Ulid::generate().to_string(),
        measured: listener.local.to_string(),
        backlog: args.backlog,
        effective_backlog: listener.effective_backlog,
    };
    let instance = info.listener_instance.clone();
    let declared = declared(args);
    let handle = listener.handle();
    let nic_for_context = nic.clone();
    Endpoints {
        info: json(&info),
        context: Box::new(move || {
            let mut c = context::read_context(&declared);
            c.nic = node::nic(&nic_for_context);
            json(&c)
        }),
        snapshot: Box::new(move || json(&snapshot(&handle, &instance, &nic))),
    }
}

/// The context once at startup, for the log (§4.1 step 3).
pub fn initial_context(args: &ListenArgs) -> NodeContext {
    context::read_context(&declared(args))
}

pub fn heartbeats(listener: &Listener, main_beat: &Arc<AtomicU64>) -> Vec<Heartbeat> {
    let handle = listener.handle();
    let main = Arc::clone(main_beat);
    vec![
        Heartbeat {
            name: "accept",
            read: Box::new(move || handle.stats().heartbeat_ns),
            bound_ns: ACCEPT_BOUND_NS,
        },
        Heartbeat {
            name: "main",
            read: Box::new(move || main.load(Ordering::Relaxed)),
            bound_ns: MAIN_BOUND_NS,
        },
    ]
}

fn snapshot(handle: &Handle, instance: &str, nic: &NicCache) -> Snapshot {
    let stats = handle.stats();
    let netstat = net::netstat();
    Snapshot {
        monotonic_ns: time::monotonic_ns(),
        boot_id: context::boot_id(),
        listener_instance: instance.to_owned(),
        listener: ListenerCounters {
            accepted: stats.accepted,
            aborted: stats.aborted,
            errors: stats.errors,
            sk_drops: handle.drops().known(),
        },
        netns_listen_overflows: netstat.get("TcpExt.ListenOverflows").copied(),
        netns_listen_drops: netstat.get("TcpExt.ListenDrops").copied(),
        cpu_stat: crate::diag::cgroup::own_file("cpu.stat").map(|t| cgroup::parse_cpu_stat(&t)),
        nic: node::nic(nic),
    }
}

fn declared(args: &ListenArgs) -> Declared {
    Declared::from_args(&args.context)
}

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}
