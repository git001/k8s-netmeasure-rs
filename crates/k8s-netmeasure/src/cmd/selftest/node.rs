//! The node-listener check of `selftest` (design §3.8, §5): reachable, on this node
//! (same `boot_id` across network namespaces) and able to see the NIC path.

use std::net::SocketAddrV4;

use netmeasure_core::schema::control::NodeContext;
use serde_json::json;

use super::report::{Check, Status};
use crate::control::client::Client;
use crate::diag::context;

pub fn check(addr: SocketAddrV4) -> Check {
    let own = context::boot_id();
    let fetched = Client::new()
        .map_err(|e| e.to_string())
        .and_then(|mut c| c.get::<NodeContext>(addr, "/v1/context"));
    let ctx = match fetched {
        Ok(c) => c,
        Err(e) => {
            return result(
                Status::Inconclusive,
                format!("{addr} not reachable: {e}"),
                json!({ "error": e }),
            );
        }
    };
    let theirs = ctx.boot_id.value.clone();
    let interfaces = ctx.nic.value.as_ref().map(|n| n.interfaces.len());
    let details = json!({ "boot_id_here": own, "boot_id_there": theirs, "nic": ctx.nic });
    match (own == theirs && own.is_some(), interfaces) {
        (false, _) => result(
            Status::Unreliable,
            format!("{addr} is on another node (boot id differs)"),
            details,
        ),
        (true, None) => result(
            Status::Unreliable,
            format!("{addr} is on this node but sees no NIC (not hostNetwork?)"),
            details,
        ),
        (true, Some(n)) => result(
            Status::Confirmed,
            format!("{addr} is on this node, {n} physical interfaces"),
            details,
        ),
    }
}

fn result(status: Status, summary: String, details: serde_json::Value) -> Check {
    Check {
        name: "node-listener",
        status,
        summary,
        details,
    }
}
