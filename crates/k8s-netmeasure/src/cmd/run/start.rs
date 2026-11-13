//! Everything `run` does before the first cycle (design §3.7, §4.1, §5, §8), in this
//! order: configuration (exit 2), output directory and CPU checks (exit 3), signal
//! handling before any other thread, peers (targets' control ports, node listener;
//! exit 3), the loopback control listener, the own context, and the optional control
//! port (exit 3 if set and not bindable).

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use netmeasure_cli::RunArgs;
use netmeasure_core::schema::control::NodeContext;
use netmeasure_core::schema::result::CgroupCheck;

use super::control::{self, Beats, RunControl};
use super::remote::{self, Remote};
use super::setup::{self, Setup};
use super::snapshot;
use crate::control::client::Client;
use crate::diag::context::{self, Declared};
use crate::diag::node_listener::{self, NodeListener};
use crate::exit;
use crate::listener::Listener;
use crate::shutdown::Shutdown;
use crate::sys::time;

pub struct Started {
    pub setup: Setup,
    pub cgroup: CgroupCheck,
    pub shutdown: Shutdown,
    pub client: Client,
    pub remotes: Vec<Remote>,
    pub node: Option<NodeListener>,
    pub listener: Listener,
    pub declared: Declared,
    pub context_before: NodeContext,
    /// `(path, path_source)` from the own network namespace.
    pub path: (String, String),
    pub run_id: String,
    pub beats: Beats,
    pub control: Option<RunControl>,
}

/// `Err((exit code, message))` refuses the start.
pub fn start(args: &RunArgs) -> Result<Started, (u8, String)> {
    let setup = setup::prepare(args)
        .map_err(|e| (exit::USAGE, format!("invalid run configuration: {e}")))?;
    let dir = args.output.parent().filter(|p| !p.as_os_str().is_empty());
    if dir.is_some_and(|d| !d.is_dir()) {
        return Err((
            exit::REFUSED,
            "cannot write --output: the directory does not exist".into(),
        ));
    }
    let cgroup = snapshot::cgroup_check(args.no_cgroup_check)
        .map_err(|e| (exit::REFUSED, format!("CPU check failed: {e}")))?;
    let internal = |what: &str, e: std::io::Error| (exit::INTERNAL, format!("{what}: {e}"));
    let shutdown = Shutdown::install().map_err(|e| internal("cannot set up signal handling", e))?;
    let mut client = Client::new().map_err(|e| internal("cannot start the control client", e))?;
    let peers = remote::connect_peers(args, &mut client).map_err(|e| (exit::REFUSED, e))?;
    let (node, node_context) = peers.node.unzip();
    let lo = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let listener = Listener::start(lo, args.backlog).map_err(|e| {
        (
            exit::REFUSED,
            format!("cannot bind the loopback control listener: {e}"),
        )
    })?;
    let declared = Declared::from_args(&args.context);
    let own = context::read_context(&declared);
    let path = setup::path(args, own.nic.value.is_some());
    let context_before = match &node_context {
        Some(n) => node_listener::with_node_nic(own, n),
        None => own,
    };
    let run_id = ulid::Ulid::generate().to_string();
    let now = time::monotonic_ns();
    let beats = Beats {
        main: Arc::new(AtomicU64::new(now)),
        measure: Arc::new(AtomicU64::new(now)),
    };
    let longest = setup.cycle_slots.iter().copied().max().unwrap_or(0);
    let control = control::start(args, &listener, &run_id, longest, &beats)
        .map_err(|e| (exit::REFUSED, e))?;
    Ok(Started {
        setup,
        cgroup,
        shutdown,
        client,
        remotes: peers.remotes,
        node,
        listener,
        declared,
        context_before,
        path,
        run_id,
        beats,
        control,
    })
}
