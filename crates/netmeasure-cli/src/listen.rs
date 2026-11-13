//! `listen` arguments (design §4).

use std::fmt;
use std::net::Ipv4Addr;

use clap::Args;

/// Default measured port (design §4.1).
pub const DEFAULT_PORT: u16 = 17700;
/// Default control port (design §4.1).
pub const DEFAULT_CONTROL_PORT: u16 = 17701;
/// Default and maximum `listen()` backlog; the kernel caps it at `somaxconn`.
pub const DEFAULT_BACKLOG: u32 = 4096;
pub const MAX_BACKLOG: u32 = 65535;

#[derive(Debug, Clone, Args)]
pub struct ListenArgs {
    /// Measured port: raw TCP, connections are accepted and nothing is read or written.
    #[arg(
        long,
        env = "K8S_NETMEASURE_PORT",
        default_value_t = DEFAULT_PORT,
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub port: u16,

    /// IPv4 address the measured port binds to.
    #[arg(long, env = "K8S_NETMEASURE_BIND", default_value_t = Ipv4Addr::UNSPECIFIED)]
    pub bind: Ipv4Addr,

    /// listen() backlog (1–65535); the effective value is min(backlog, somaxconn).
    #[arg(
        long,
        env = "K8S_NETMEASURE_BACKLOG",
        default_value_t = DEFAULT_BACKLOG,
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_BACKLOG))
    )]
    pub backlog: u32,

    /// Control port: HTTP/1.1 + JSON for Kubernetes probes and /v1/*; must differ from
    /// --port.
    #[arg(
        long,
        env = "K8S_NETMEASURE_CONTROL_PORT",
        default_value_t = DEFAULT_CONTROL_PORT,
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub control_port: u16,

    /// IPv4 address of the control port; `auto` = the address of the interface with
    /// the default route (the pod IP, or the node IP with hostNetwork). `oc
    /// port-forward` needs 0.0.0.0.
    #[arg(
        long,
        env = "K8S_NETMEASURE_CONTROL_BIND",
        default_value_t = ControlBind::Auto,
        value_parser = control_bind
    )]
    pub control_bind: ControlBind,

    #[command(flatten)]
    pub log: crate::LogArgs,
}

/// `--control-bind`: `auto` or an IPv4 address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlBind {
    Auto,
    Addr(Ipv4Addr),
}

impl fmt::Display for ControlBind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlBind::Auto => f.write_str("auto"),
            ControlBind::Addr(a) => write!(f, "{a}"),
        }
    }
}

fn control_bind(text: &str) -> Result<ControlBind, String> {
    if text == "auto" {
        return Ok(ControlBind::Auto);
    }
    text.parse()
        .map(ControlBind::Addr)
        .map_err(|_| format!("`{text}` is neither `auto` nor an IPv4 address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_bind_values() {
        assert_eq!(control_bind("auto"), Ok(ControlBind::Auto));
        let a = control_bind("10.0.0.5").unwrap();
        assert_eq!(a, ControlBind::Addr(Ipv4Addr::new(10, 0, 0, 5)));
        assert_eq!(a.to_string(), "10.0.0.5");
        assert_eq!(ControlBind::Auto.to_string(), "auto");
        assert!(control_bind("::1").unwrap_err().contains("neither"));
    }
}
