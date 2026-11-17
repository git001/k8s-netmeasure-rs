//! One line per node for the NIC context fields (design §10.6): ring sizes, channels,
//! driver version and firmware, each as `driver:value` per physical interface, sorted.
//! `unknown` when the NIC path was not observed or a field was not recorded yet
//! (result files written before it existed).

use netmeasure_core::schema::control::NodeContext;
use netmeasure_core::schema::nic::Interface;

const UNKNOWN: &str = "unknown";

/// `driver:<value>` per interface; `None` from `value` marks "not recorded".
fn per_interface(c: &NodeContext, value: fn(&Interface) -> Option<String>) -> String {
    let Some(p) = &c.nic.value else {
        return UNKNOWN.into();
    };
    let mut v = Vec::new();
    for i in &p.interfaces {
        let Some(x) = value(i) else {
            return UNKNOWN.into();
        };
        v.push(format!("{}:{x}", i.driver.as_deref().unwrap_or("-")));
    }
    v.sort();
    v.join(" ")
}

fn opt(v: Option<u32>) -> String {
    v.map_or("-".into(), |x| x.to_string())
}

/// Current RX/TX ring sizes, e.g. `drv-a:1024/1024 drv-b:-`.
pub fn rings(c: &NodeContext) -> String {
    per_interface(c, |i| {
        Some(match &i.rings {
            Some(r) => format!("{}/{}", opt(r.rx), opt(r.tx)),
            None => "-".into(),
        })
    })
}

/// Channels current/max: combined if the driver has them, else `rx a/b tx c/d`.
pub fn channels(c: &NodeContext) -> String {
    per_interface(c, |i| match (&i.channels, i.channels_errno) {
        (None, None) => None,
        (None, Some(_)) => Some("-".into()),
        (Some(ch), _) => Some(match ch.combined {
            Some(n) => format!("{n}/{}", opt(ch.combined_max)),
            None => format!(
                "rx {}/{} tx {}/{}",
                opt(ch.rx),
                opt(ch.rx_max),
                opt(ch.tx),
                opt(ch.tx_max)
            ),
        }),
    })
}

/// Driver version and firmware, e.g. `drv-a:1.0 fw 2.3`.
pub fn driver(c: &NodeContext) -> String {
    per_interface(c, |i| match (&i.driver_info, i.driver_info_errno) {
        (None, None) => None,
        (None, Some(_)) => Some("-".into()),
        (Some(d), _) => Some(format!(
            "{} fw {}",
            d.version.as_deref().unwrap_or("-"),
            d.firmware.as_deref().unwrap_or("-")
        )),
    })
}
