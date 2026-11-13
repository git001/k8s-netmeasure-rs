//! Node facts needed by `selftest` (the full node context of §5 follows in
//! milestone 3): kernel, boot id, CPU model and microcode, TCP sysctls, netstat
//! counters, and per physical interface the driver and ring sizes.

use std::collections::BTreeMap;
use std::path::Path;

use netmeasure_core::diag::cpuinfo::{self, CpuInfo};
use netmeasure_core::diag::netstat;
use serde::Serialize;

use crate::sys::ethtool;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeFacts {
    pub kernel_release: Option<String>,
    pub kernel_version: Option<String>,
    pub boot_id: Option<String>,
    /// `/proc/cpuinfo` of the first processor; both fields `None` if unreadable.
    pub cpu: CpuInfo,
    pub tcp_syn_retries: Option<u32>,
    /// `None` if the sysctl does not exist on this kernel.
    pub tcp_syn_linear_timeouts: Option<u32>,
    pub somaxconn: Option<u32>,
    /// Physical interfaces visible in this network namespace (anything with
    /// `/sys/class/net/<if>/device`). Empty in a pod network namespace: there only
    /// veth devices exist; the underlay view needs `hostNetwork`.
    pub interfaces: Vec<Interface>,
}

/// One physical interface (design §5: driver, ring sizes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Interface {
    pub name: String,
    /// Bound driver (`/sys/class/net/<if>/device/driver`), `None` if unbound.
    pub driver: Option<String>,
    /// `ethtool -g`; `None` with `rings_errno` set if the driver does not report them.
    pub rings: Option<RingSizes>,
    pub rings_errno: Option<i32>,
}

/// Current and maximum ring sizes. Hosts behind the same instance type can differ
/// here, e.g. a smaller TX ring on one NIC family, so they belong to the context.
/// A direction whose maximum the driver reports as 0 is not reported (`ethtool -g`
/// prints `n/a`) and is `None`, never a misleading 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RingSizes {
    pub rx: Option<u32>,
    pub tx: Option<u32>,
    pub rx_max: Option<u32>,
    pub tx_max: Option<u32>,
}

impl RingSizes {
    fn from_ethtool(r: ethtool::Rings) -> RingSizes {
        let reported = |max: u32, current: u32| (max > 0).then_some((current, max));
        let rx = reported(r.rx_max, r.rx);
        let tx = reported(r.tx_max, r.tx);
        RingSizes {
            rx: rx.map(|(c, _)| c),
            tx: tx.map(|(c, _)| c),
            rx_max: rx.map(|(_, m)| m),
            tx_max: tx.map(|(_, m)| m),
        }
    }
}

pub fn read() -> NodeFacts {
    NodeFacts {
        kernel_release: text("/proc/sys/kernel/osrelease"),
        kernel_version: text("/proc/sys/kernel/version"),
        boot_id: text("/proc/sys/kernel/random/boot_id"),
        cpu: cpuinfo::parse(&super::read("/proc/cpuinfo").unwrap_or_default()),
        tcp_syn_retries: number("/proc/sys/net/ipv4/tcp_syn_retries"),
        tcp_syn_linear_timeouts: number("/proc/sys/net/ipv4/tcp_syn_linear_timeouts"),
        somaxconn: number("/proc/sys/net/core/somaxconn"),
        interfaces: interfaces(Path::new("/sys/class/net")),
    }
}

/// Physical interfaces under `sys_class_net`, sorted by name.
fn interfaces(sys_class_net: &Path) -> Vec<Interface> {
    let Ok(entries) = std::fs::read_dir(sys_class_net) else {
        return Vec::new();
    };
    let mut out: Vec<Interface> = entries
        .flatten()
        .filter(|e| e.path().join("device").exists())
        .map(|e| interface(&e.file_name().to_string_lossy(), &e.path()))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn interface(name: &str, dir: &Path) -> Interface {
    let driver = std::fs::read_link(dir.join("device/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let (rings, rings_errno) = match ethtool::rings(name) {
        Ok(r) => (Some(RingSizes::from_ethtool(r)), None),
        Err(e) => (None, Some(e)),
    };
    Interface {
        name: name.to_owned(),
        driver,
        rings,
        rings_errno,
    }
}

/// `TcpExt.*` and friends of the own network namespace.
pub fn netstat() -> BTreeMap<String, u64> {
    super::read("/proc/net/netstat")
        .map(|t| netstat::parse(&t))
        .unwrap_or_default()
}

fn text(path: &str) -> Option<String> {
    super::read(path).map(|s| s.trim().to_owned())
}

fn number(path: &str) -> Option<u32> {
    text(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_interfaces_with_a_device_are_physical() {
        let root = std::env::temp_dir().join(format!("nm-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nm-phys0/device")).unwrap();
        std::fs::create_dir_all(root.join("nm-veth0")).unwrap();
        let found = interfaces(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.len(), 1);
        let i = &found[0];
        assert_eq!(i.name, "nm-phys0");
        assert_eq!(i.driver, None, "no driver link");
        assert_eq!((i.rings, i.rings_errno), (None, Some(libc::ENODEV)));
    }

    #[test]
    fn zero_maximum_means_not_reported() {
        let r = RingSizes::from_ethtool(ethtool::Rings {
            rx: 1024,
            tx: 0,
            rx_max: 8192,
            tx_max: 0,
        });
        assert_eq!((r.rx, r.rx_max), (Some(1024), Some(8192)));
        assert_eq!((r.tx, r.tx_max), (None, None));
    }

    #[test]
    fn missing_sysfs_gives_no_interfaces() {
        assert!(interfaces(Path::new("/nonexistent/sys/class/net")).is_empty());
    }
}
