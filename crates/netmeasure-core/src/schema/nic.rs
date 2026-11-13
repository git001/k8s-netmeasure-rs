//! The NIC path (design §3.6, §5): physical interfaces with driver and ring sizes, and
//! the interface that carries the default route. Compared around every cycle.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NicPath {
    pub default_route_interface: Option<String>,
    /// Physical interfaces (anything with `/sys/class/net/<if>/device`), by name.
    pub interfaces: Vec<Interface>,
}

/// One physical interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    /// Bound driver (`device/driver` link), `None` if unbound.
    pub driver: Option<String>,
    /// `ethtool -g`; `None` with `rings_errno` set if the driver does not report them.
    pub rings: Option<RingSizes>,
    pub rings_errno: Option<i32>,
}

/// Current and maximum ring sizes. Hosts behind the same instance type can differ
/// here, e.g. a smaller TX ring on one NIC family. A direction whose maximum the
/// driver reports as 0 is not reported (`ethtool -g` prints `n/a`) and is `None`,
/// never a misleading 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RingSizes {
    pub rx: Option<u32>,
    pub tx: Option<u32>,
    pub rx_max: Option<u32>,
    pub tx_max: Option<u32>,
}

impl RingSizes {
    /// From the raw `ETHTOOL_GRINGPARAM` values.
    pub fn from_raw(rx: u32, tx: u32, rx_max: u32, tx_max: u32) -> RingSizes {
        let reported = |max: u32, current: u32| (max > 0).then_some((current, max));
        let (rx, tx) = (reported(rx_max, rx), reported(tx_max, tx));
        RingSizes {
            rx: rx.map(|(c, _)| c),
            tx: tx.map(|(c, _)| c),
            rx_max: rx.map(|(_, m)| m),
            tx_max: tx.map(|(_, m)| m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_maximum_means_not_reported() {
        let r = RingSizes::from_raw(1024, 0, 8192, 0);
        assert_eq!((r.rx, r.rx_max), (Some(1024), Some(8192)));
        assert_eq!((r.tx, r.tx_max), (None, None));
    }
}
