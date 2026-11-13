//! The node context (design §5) and the NIC path (§3.6) as observed by this process.
//! Declared values (node name, zone, instance type) come from flags; everything else
//! is read here or `unknown`.

use std::path::Path;

use netmeasure_core::diag::cgroup as cg;
use netmeasure_core::diag::cpuinfo;
use netmeasure_core::schema::control::NodeContext;
use netmeasure_core::schema::nic::{Interface, NicPath, RingSizes};
use netmeasure_core::schema::sourced::Sourced;

use super::{cgroup, net, read};
use crate::sys::ethtool;

/// Values the operator declares (Phase 1 has no API access, §5).
#[derive(Debug, Clone, Default)]
pub struct Declared {
    pub node_name: Option<String>,
    pub zone: Option<String>,
    pub instance_type: Option<String>,
}

impl Declared {
    /// `--node-name`, else `$NODE_NAME` (Downward API), else unknown.
    pub fn from_args(c: &netmeasure_cli::ContextArgs) -> Declared {
        Declared {
            node_name: c
                .node_name
                .clone()
                .or_else(|| std::env::var("NODE_NAME").ok()),
            zone: c.zone.clone(),
            instance_type: c.instance_type.clone(),
        }
    }
}

pub fn read_context(declared: &Declared) -> NodeContext {
    let cpu = cpuinfo::parse(&read("/proc/cpuinfo").unwrap_or_default());
    let mtu =
        net::default_route_interface().and_then(|i| number(&format!("/sys/class/net/{i}/mtu")));
    NodeContext {
        node_name: Sourced::declared(declared.node_name.clone()),
        zone: Sourced::declared(declared.zone.clone()),
        instance_type: Sourced::declared(declared.instance_type.clone()),
        kernel_release: Sourced::observed(text("/proc/sys/kernel/osrelease")),
        kernel_version: Sourced::observed(text("/proc/sys/kernel/version")),
        boot_id: Sourced::observed(boot_id()),
        uptime_s: Sourced::observed(uptime_s()),
        cpu_max: Sourced::observed(
            cgroup::own_file("cpu.max").and_then(|t| cg::parse_cpu_max(&t).ok()),
        ),
        cpu_model: Sourced::observed(cpu.model),
        cpu_microcode: Sourced::observed(cpu.microcode),
        somaxconn: Sourced::observed(number("/proc/sys/net/core/somaxconn")),
        tcp_syn_retries: Sourced::observed(number("/proc/sys/net/ipv4/tcp_syn_retries")),
        tcp_syn_linear_timeouts: Sourced::observed(number(
            "/proc/sys/net/ipv4/tcp_syn_linear_timeouts",
        )),
        mtu: Sourced::observed(mtu),
        nic: nic_path(),
    }
}

/// The NIC path; `unknown` where no physical interface is visible, as in a pod's
/// network namespace (the overlay gets it from a node listener, §5).
pub fn nic_path() -> Sourced<NicPath> {
    let interfaces = interfaces(Path::new("/sys/class/net"));
    Sourced::observed((!interfaces.is_empty()).then(|| NicPath {
        default_route_interface: net::default_route_interface(),
        interfaces,
    }))
}

pub fn boot_id() -> Option<String> {
    text("/proc/sys/kernel/random/boot_id")
}

fn uptime_s() -> Option<u64> {
    let t = read("/proc/uptime")?;
    let secs: f64 = t.split_whitespace().next()?.parse().ok()?;
    Some(secs as u64)
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
        Ok(r) => (
            Some(RingSizes::from_raw(r.rx, r.tx, r.rx_max, r.tx_max)),
            None,
        ),
        Err(e) => (None, Some(e)),
    };
    Interface {
        name: name.to_owned(),
        driver,
        rings,
        rings_errno,
    }
}

fn text(path: &str) -> Option<String> {
    read(path).map(|s| s.trim().to_owned())
}

fn number<T: std::str::FromStr>(path: &str) -> Option<T> {
    text(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use netmeasure_core::schema::sourced::Source;

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
    fn missing_sysfs_gives_no_interfaces() {
        assert!(interfaces(Path::new("/nonexistent/sys/class/net")).is_empty());
    }

    #[test]
    fn context_carries_provenance() {
        let c = read_context(&Declared {
            zone: Some("z1".into()),
            ..Declared::default()
        });
        assert_eq!(c.zone.source, Source::Declared);
        assert_eq!(c.node_name.source, Source::Unknown);
        assert_eq!(c.kernel_release.source, Source::Observed);
        assert_eq!(c.boot_id.value, boot_id());
        assert!(c.uptime_s.value.is_some());
    }
}
