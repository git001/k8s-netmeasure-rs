//! Declared node context (design §5): Phase 1 has no API access, so node name, zone
//! and instance type come from the operator or stay `unknown`.

use std::net::SocketAddrV4;

use clap::Args;

#[derive(Debug, Clone, Default, Args)]
pub struct ContextArgs {
    /// Node name for the context; if not set, the environment variable NODE_NAME
    /// (Downward API `spec.nodeName`) is used, else it is unknown.
    #[arg(long, env = "K8S_NETMEASURE_NODE_NAME")]
    pub node_name: Option<String>,

    /// Availability zone for the context (unknown if not set).
    #[arg(long, env = "K8S_NETMEASURE_ZONE")]
    pub zone: Option<String>,

    /// Instance type for the context (unknown if not set). Never used to infer the
    /// NIC family, which is always observed.
    #[arg(long, env = "K8S_NETMEASURE_INSTANCE_TYPE")]
    pub instance_type: Option<String>,

    /// Control port (IPv4:port) of an underlay `listen` (hostNetwork) on the same node.
    /// Its NIC path is used as this process's NIC context, which a pod network
    /// namespace cannot observe. The node listener's boot id must equal this one's.
    #[arg(long, env = "K8S_NETMEASURE_NODE_CONTEXT_FROM", value_parser = crate::parse::ipv4_target)]
    pub node_context_from: Option<SocketAddrV4>,
}
