//! `listen --node-context-from` (design §5): an overlay `listen` reports the NIC path
//! of the underlay `listen` on its node. The main thread refreshes it every second, so
//! the control thread only reads a cached value and never waits for the network.

use std::sync::{Arc, Mutex};

use netmeasure_cli::ListenArgs;
use netmeasure_core::schema::nic::NicPath;
use netmeasure_core::schema::sourced::Sourced;

use crate::control::client::Client;
use crate::diag::context;
use crate::diag::node_listener::NodeListener;

pub struct Node {
    client: Client,
    listener: NodeListener,
    cache: Arc<Mutex<Sourced<NicPath>>>,
}

/// Connects to the node listener if configured. `Err` refuses the start (exit 3).
pub fn connect(args: &ListenArgs) -> Result<Option<Node>, String> {
    let Some(addr) = args.context.node_context_from else {
        return Ok(None);
    };
    let mut client = Client::new().map_err(|e| e.to_string())?;
    let (listener, ctx) = NodeListener::connect(&mut client, addr, context::boot_id().as_deref())?;
    let cache = Arc::new(Mutex::new(Sourced::via_node_listener(ctx.nic.value)));
    Ok(Some(Node {
        client,
        listener,
        cache,
    }))
}

impl Node {
    pub fn cache(&self) -> Arc<Mutex<Sourced<NicPath>>> {
        Arc::clone(&self.cache)
    }

    pub fn refresh(&mut self) {
        let (nic, _) = self.listener.nic(&mut self.client);
        if let Ok(mut c) = self.cache.lock() {
            *c = nic;
        }
    }
}
