//! `/proc/net/route`: the interface that carries the IPv4 default route (design §3.6,
//! §4.1). Columns are tab-separated, the first line is a header; destination and mask
//! are hex. The default route has destination and mask 0 and the `RTF_UP` flag.

/// `RTF_UP` (`include/uapi/linux/route.h`).
const RTF_UP: u32 = 0x1;

/// The interface of the default route with the lowest metric, if any.
pub fn default_route_interface(text: &str) -> Option<String> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let [iface, dest, _gw, flags, _refcnt, _use, metric, mask, ..] = cols[..] else {
                return None;
            };
            let flags = u32::from_str_radix(flags, 16).ok()?;
            let is_default = dest == "00000000" && mask == "00000000";
            (is_default && flags & RTF_UP != 0).then(|| (metric.parse::<u32>().ok(), iface))
        })
        .min_by_key(|(metric, _)| metric.unwrap_or(u32::MAX))
        .map(|(_, iface)| iface.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n";

    #[test]
    fn lowest_metric_default_route_wins() {
        let text = format!(
            "{HEADER}eth1\t00000000\t0101000A\t0003\t0\t0\t200\t00000000\t0\t0\t0\n\
             eth0\t00000000\t0100000A\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
             eth0\t0000000A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n"
        );
        assert_eq!(default_route_interface(&text).as_deref(), Some("eth0"));
    }

    #[test]
    fn no_default_route_or_route_down() {
        let down = format!("{HEADER}eth0\t00000000\t0100000A\t0002\t0\t0\t0\t00000000\t0\t0\t0\n");
        assert_eq!(default_route_interface(&down), None);
        let none = format!("{HEADER}eth0\t0000000A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n");
        assert_eq!(default_route_interface(&none), None);
        assert_eq!(default_route_interface(""), None);
        assert_eq!(default_route_interface(&format!("{HEADER}garbage\n")), None);
    }
}
