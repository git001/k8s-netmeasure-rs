//! The targets' control ports in `run` (design §3.6, §7): identity and context at the
//! start, a `/v1/snapshot` before and after every cycle, and what the validity rules
//! and diagnostics need from them.

use std::collections::BTreeMap;
use std::net::SocketAddrV4;

use netmeasure_core::measure::validity::{SideCounters, TargetCycle, delta};
use netmeasure_core::schema::control::{self, Info, NodeContext, Snapshot};
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::result::TargetDiagnostics;
use netmeasure_core::schema::sourced::Sourced;

use crate::control::client::Client;

/// A target with a control port, as seen at the start of the run.
pub struct Remote {
    /// Value in the `target` column.
    pub index: u16,
    pub control: SocketAddrV4,
    pub listener_instance: String,
    pub boot_id: Option<String>,
    pub context: NodeContext,
}

/// `/v1/info` and `/v1/context` of every target; any failure refuses the start.
pub fn connect(
    client: &mut Client,
    targets: &[SocketAddrV4],
    port: u16,
) -> Result<Vec<Remote>, String> {
    targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let control = SocketAddrV4::new(*t.ip(), port);
            let info: Info = client.get(control, "/v1/info")?;
            if info.schema != control::SCHEMA || info.schema_version != control::SCHEMA_VERSION {
                return Err(format!(
                    "{control}: unsupported control schema {} v{}",
                    info.schema, info.schema_version
                ));
            }
            let context: NodeContext = client.get(control, "/v1/context")?;
            Ok(Remote {
                index: i as u16 + 1,
                control,
                listener_instance: info.listener_instance,
                boot_id: context.boot_id.value.clone(),
                context,
            })
        })
        .collect()
}

/// One snapshot per target; `None` where the control port did not answer.
pub fn snapshots(client: &mut Client, remotes: &[Remote]) -> Vec<Option<Snapshot>> {
    remotes
        .iter()
        .map(|r| {
            client
                .get(r.control, "/v1/snapshot")
                .inspect_err(|e| tracing::warn!(target_node = %r.control, error = %e, "target control port unreachable"))
                .ok()
        })
        .collect()
}

/// The context at the end of the run; `None` if unreachable.
pub fn context_after(client: &mut Client, r: &Remote) -> Option<NodeContext> {
    client.get(r.control, "/v1/context").ok()
}

/// What the validity rules see of one target in one cycle.
pub fn cycle(
    r: &Remote,
    before: Option<&Snapshot>,
    after: Option<&Snapshot>,
    nic_first: Option<&NicPath>,
) -> TargetCycle {
    let same = |s: &Snapshot| s.listener_instance == r.listener_instance && s.boot_id == r.boot_id;
    let nic = |s: Option<&Snapshot>| s.map_or_else(Sourced::unknown, |s| s.nic.clone());
    TargetCycle {
        before: before.map(counters),
        after: after.map(counters),
        same_identity: before.is_none_or(same) && after.is_none_or(same),
        nic_before: nic(before),
        nic_after: nic(after),
        nic_first: nic_first.cloned(),
    }
}

/// Deltas for the diagnostics and the per-cycle `listener_drops`.
pub fn diagnostics(
    before: Option<&Snapshot>,
    after: Option<&Snapshot>,
) -> (TargetDiagnostics, Option<u64>) {
    let get = |f: fn(&Snapshot) -> Option<u64>| delta(before.and_then(f), after.and_then(f));
    (
        TargetDiagnostics {
            accepted: get(|s| Some(s.listener.accepted)),
            aborted: get(|s| Some(s.listener.aborted)),
            netns_listen_drops: get(|s| s.netns_listen_drops),
            netns_listen_overflows: get(|s| s.netns_listen_overflows),
        },
        get(|s| s.listener.sk_drops.map(u64::from)),
    )
}

/// Index → value, as the result file keys targets (`"1"`, `"2"`, …).
pub fn by_index<T>(remotes: &[Remote], values: impl IntoIterator<Item = T>) -> BTreeMap<String, T> {
    remotes
        .iter()
        .map(|r| r.index.to_string())
        .zip(values)
        .collect()
}

fn counters(s: &Snapshot) -> SideCounters {
    SideCounters {
        throttled_usec: None,
        ancestors_throttled_usec: None,
        sk_drops: s.listener.sk_drops,
        netns_listen_drops: s.netns_listen_drops,
        netns_listen_overflows: s.netns_listen_overflows,
    }
}
