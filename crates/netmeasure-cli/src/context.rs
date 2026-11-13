//! Declared node context (design §5): Phase 1 has no API access, so node name, zone
//! and instance type come from the operator or stay `unknown`.

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
}
