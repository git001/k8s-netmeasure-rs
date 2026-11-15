//! The node listener (design §5): an underlay `listen` on the same node whose NIC path
//! stands in for the one a pod network namespace cannot observe. The same node is
//! proven by `boot_id`, which belongs to the kernel, not to a network namespace.

use std::net::SocketAddrV4;

use netmeasure_core::diag::nicstats::NicCounters;
use netmeasure_core::measure::validity::NodeListener as State;
use netmeasure_core::schema::control::{NodeContext, Snapshot};
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::sourced::Sourced;

use crate::control::client::Client;

pub struct NodeListener {
    pub addr: SocketAddrV4,
    boot_id: String,
    /// Set once it reports another boot id; it is not asked again.
    mismatched: bool,
}

impl NodeListener {
    /// Reads its `/v1/context` and checks that it is on this node and sees a NIC path.
    /// `Err` refuses the start (exit 3).
    pub fn connect(
        client: &mut Client,
        addr: SocketAddrV4,
        own_boot_id: Option<&str>,
    ) -> Result<(NodeListener, NodeContext), String> {
        let context: NodeContext = client.get(addr, "/v1/context")?;
        let theirs = context.boot_id.value.clone();
        match (own_boot_id, theirs.as_deref()) {
            (Some(own), Some(t)) if own == t => {}
            (own, t) => {
                return Err(format!(
                    "node listener {addr} is not on this node (boot id {} here, {} there)",
                    own.unwrap_or("unknown"),
                    t.unwrap_or("unknown")
                ));
            }
        }
        if context.nic.value.is_none() {
            return Err(format!(
                "node listener {addr} sees no physical interface; it must run with hostNetwork"
            ));
        }
        let boot_id = theirs.unwrap_or_default();
        Ok((
            NodeListener {
                addr,
                boot_id,
                mismatched: false,
            },
            context,
        ))
    }

    /// The node's NIC path and selected NIC statistics now, from its `/v1/snapshot`.
    pub fn nic(&mut self, client: &mut Client) -> (Sourced<NicPath>, NicCounters, State) {
        let none = |state| (Sourced::unknown(), NicCounters::new(), state);
        if self.mismatched {
            return none(State::Mismatch);
        }
        match client.get::<Snapshot>(self.addr, "/v1/snapshot") {
            Err(e) => {
                tracing::warn!(node_listener = %self.addr, error = %e, "node listener unreachable; NIC path unknown");
                none(State::Unavailable)
            }
            Ok(s) if s.boot_id.as_deref() != Some(self.boot_id.as_str()) => {
                tracing::warn!(node_listener = %self.addr, "node listener reports another boot id; no longer used");
                self.mismatched = true;
                none(State::Mismatch)
            }
            Ok(s) => (
                Sourced::via_node_listener(s.nic.value),
                s.nic_stats,
                State::Ok,
            ),
        }
    }
}

impl NodeListener {
    /// `own` with the node's NIC path at the end of the run; unknown if the node
    /// listener is gone or no longer on this node.
    pub fn context_after(&mut self, client: &mut Client, own: NodeContext) -> NodeContext {
        let node = (!self.mismatched)
            .then(|| client.get::<NodeContext>(self.addr, "/v1/context").ok())
            .flatten()
            .filter(|c| c.boot_id.value.as_deref() == Some(self.boot_id.as_str()));
        match node {
            Some(n) => with_node_nic(own, &n),
            None => NodeContext {
                nic: Sourced::unknown(),
                ..own
            },
        }
    }
}

/// `context` with its NIC path taken from the node listener's context.
pub fn with_node_nic(mut context: NodeContext, node: &NodeContext) -> NodeContext {
    context.nic = Sourced::via_node_listener(node.nic.value.clone());
    context
}
