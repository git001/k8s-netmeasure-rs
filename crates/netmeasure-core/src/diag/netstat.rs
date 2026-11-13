//! `/proc/net/netstat` and `/proc/net/snmp`: pairs of header and value lines per
//! section, e.g. `TcpExt: ListenDrops …` / `TcpExt: 0 …`.
//!
//! Own code on purpose (checked in milestone 2): `procfs` parses `/proc/net/snmp` but
//! not `/proc/net/netstat` (no `TcpExt` counters such as `ListenDrops`); the
//! `netstat*` crates list sockets, not these counters.

use std::collections::BTreeMap;

/// `Section.Name` → value, e.g. `TcpExt.ListenDrops`.
pub fn parse(text: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let lines: Vec<&str> = text.lines().collect();
    for pair in lines.chunks(2) {
        let [names, values] = pair else { continue };
        let (Some((section, names)), Some((section2, values))) =
            (names.split_once(':'), values.split_once(':'))
        else {
            continue;
        };
        if section != section2 {
            continue;
        }
        for (n, v) in names.split_whitespace().zip(values.split_whitespace()) {
            if let Ok(v) = v.parse() {
                out.insert(format!("{section}.{n}"), v);
            }
        }
    }
    out
}

/// Counters that changed between two reads (`after − before`, only increases), for the
/// per-cycle diagnostics (§5). A counter missing on either side or going backwards is
/// left out.
pub fn delta(
    before: &BTreeMap<String, u64>,
    after: &BTreeMap<String, u64>,
) -> BTreeMap<String, u64> {
    after
        .iter()
        .filter_map(|(k, a)| {
            let d = a.checked_sub(*before.get(k)?)?;
            (d > 0).then(|| (k.clone(), d))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_keeps_increases_only() {
        let m = |v: &[(&str, u64)]| v.iter().map(|(k, n)| ((*k).to_owned(), *n)).collect();
        let d = delta(
            &m(&[("a", 1), ("b", 5), ("c", 2)]),
            &m(&[("a", 4), ("b", 5), ("c", 1), ("d", 9)]),
        );
        assert_eq!(d, m(&[("a", 3)]));
    }

    #[test]
    fn sections_are_prefixed() {
        let text = "TcpExt: SyncookiesSent ListenOverflows ListenDrops\nTcpExt: 0 3 4\nIpExt: InNoRoutes\nIpExt: 1\n";
        let m = parse(text);
        assert_eq!(m["TcpExt.ListenDrops"], 4);
        assert_eq!(m["TcpExt.ListenOverflows"], 3);
        assert_eq!(m["IpExt.InNoRoutes"], 1);
    }

    #[test]
    fn mismatched_or_odd_lines_are_skipped() {
        assert!(parse("TcpExt: A\nIpExt: 1\nTcpExt: B\n").is_empty());
    }
}
