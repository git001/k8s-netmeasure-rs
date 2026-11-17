//! The NIC path (design §3.6, §5): physical interfaces with driver, driver version,
//! firmware, ring sizes and channels, and the interface that carries the default
//! route. Compared around every cycle.

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
    /// `ethtool -i`; both absent in files written before they were recorded.
    #[serde(default)]
    pub driver_info: Option<DriverInfo>,
    #[serde(default)]
    pub driver_info_errno: Option<i32>,
    /// `ethtool -l`; both absent in files written before they were recorded.
    #[serde(default)]
    pub channels: Option<Channels>,
    #[serde(default)]
    pub channels_errno: Option<i32>,
}

/// Driver version, firmware and bus address as the driver reports them; an empty
/// string from the driver is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverInfo {
    pub version: Option<String>,
    pub firmware: Option<String>,
    pub bus: Option<String>,
}

impl DriverInfo {
    pub fn from_raw(version: &str, firmware: &str, bus: &str) -> DriverInfo {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        DriverInfo {
            version: some(version),
            firmware: some(firmware),
            bus: some(bus),
        }
    }
}

/// Current and maximum channel (queue) counts. A kind whose maximum the driver
/// reports as 0 does not exist on it (`ethtool -l`: `n/a`) and is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channels {
    pub rx: Option<u32>,
    pub tx: Option<u32>,
    pub other: Option<u32>,
    pub combined: Option<u32>,
    pub rx_max: Option<u32>,
    pub tx_max: Option<u32>,
    pub other_max: Option<u32>,
    pub combined_max: Option<u32>,
}

impl Channels {
    /// From the raw `ETHTOOL_GCHANNELS` values, `(current, max)` per kind in the
    /// order rx, tx, other, combined.
    pub fn from_raw(raw: [(u32, u32); 4]) -> Channels {
        let r = raw.map(|(c, m)| (m > 0).then_some((c, m)));
        let cur = |i: usize| r[i].map(|(c, _)| c);
        let max = |i: usize| r[i].map(|(_, m)| m);
        Channels {
            rx: cur(0),
            tx: cur(1),
            other: cur(2),
            combined: cur(3),
            rx_max: max(0),
            tx_max: max(1),
            other_max: max(2),
            combined_max: max(3),
        }
    }
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

    #[test]
    fn channels_without_a_maximum_do_not_exist() {
        let c = Channels::from_raw([(0, 0), (0, 0), (1, 1), (16, 32)]);
        assert_eq!((c.combined, c.combined_max), (Some(16), Some(32)));
        assert_eq!((c.other, c.other_max), (Some(1), Some(1)));
        assert_eq!((c.rx, c.tx, c.rx_max), (None, None, None));
    }

    #[test]
    fn empty_driver_strings_are_none() {
        let d = DriverInfo::from_raw("", "1.2.3", "");
        assert_eq!(
            (d.version, d.firmware.as_deref(), d.bus),
            (None, Some("1.2.3"), None)
        );
    }

    #[test]
    fn files_without_driver_info_and_channels_still_parse() {
        let old = r#"{"name": "eth0", "driver": "d", "rings": null, "rings_errno": 95}"#;
        let i: Interface = serde_json::from_str(old).unwrap();
        assert_eq!((i.driver_info, i.channels), (None, None));
    }
}
