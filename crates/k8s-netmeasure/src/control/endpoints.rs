//! The control-port bodies (design §7), shared by `listen` and `run --control-port`:
//! `/v1/info` fixed at start, `/v1/context` and `/v1/snapshot` built per request from
//! `/proc`, `/sys`, the measured listener's counters and the cached node NIC path.

use std::sync::{Arc, Mutex};

use netmeasure_cli::ContextArgs;
use netmeasure_core::diag::cgroup;
use netmeasure_core::schema::control::{self, Info, ListenerCounters, Snapshot};
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::sourced::Sourced;

use super::state::Endpoints;
use crate::build_info::BUILD_INFO;
use crate::diag::context::{self, Declared};
use crate::diag::net;
use crate::listener::{Handle, Listener};
use crate::sys::time;

/// The NIC path to report; `None` without `--node-context-from` (observe directly).
pub type NicCache = Option<Arc<Mutex<Sourced<NicPath>>>>;

/// `instance` is the `listener_instance` (`listen`) or the `run_id` (`run`).
pub fn endpoints(
    backlog: u32,
    context_args: &ContextArgs,
    listener: &Listener,
    nic: NicCache,
    instance: String,
) -> Endpoints {
    let info = Info {
        schema: control::SCHEMA.into(),
        schema_version: control::SCHEMA_VERSION,
        build_info: BUILD_INFO.schema(),
        listener_instance: instance,
        measured: listener.local.to_string(),
        backlog,
        effective_backlog: listener.effective_backlog,
    };
    let instance = info.listener_instance.clone();
    let declared = Declared::from_args(context_args);
    let handle = listener.handle();
    let nic_for_context = nic.clone();
    Endpoints {
        info: json(&info),
        context: Box::new(move || {
            let mut c = context::read_context(&declared);
            c.nic = nic_now(&nic_for_context);
            json(&c)
        }),
        snapshot: Box::new(move || json(&snapshot(&handle, &instance, &nic))),
    }
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
        nic: nic_now(nic),
    }
}

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

/// The NIC path to report now.
pub fn nic_now(cache: &NicCache) -> Sourced<NicPath> {
    match cache {
        Some(c) => c.lock().map_or_else(|_| Sourced::unknown(), |g| g.clone()),
        None => context::nic_path(),
    }
}
