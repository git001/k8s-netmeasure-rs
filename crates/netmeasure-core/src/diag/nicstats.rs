//! NIC statistics (`ethtool -S`) as diagnostics (design §5, milestone 6): only the
//! counters whose name points at trouble are kept, by driver-neutral name patterns, so
//! no vendor list is needed. Per cycle the increases are recorded; they explain a
//! change but never decide one.

use std::collections::BTreeMap;

/// Interface → counter → value.
pub type NicCounters = BTreeMap<String, BTreeMap<String, u64>>;

/// Substrings (case-insensitive) of the kept counters: drops, discards, errors,
/// misses, overruns/overflows, timeouts, a full or busy queue and the queue being
/// stopped and woken (a full TX ring holds packets back).
pub const PATTERNS: [&str; 10] = [
    "drop", "discard", "err", "miss", "over", "timeout", "full", "busy", "stop", "wake",
];

pub fn selected(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    PATTERNS.iter().any(|p| n.contains(p))
}

/// The selected counters of one interface. `u64::MAX` is left out: some drivers
/// report it for a value they do not have (not a counter).
pub fn select(stats: impl IntoIterator<Item = (String, u64)>) -> BTreeMap<String, u64> {
    stats
        .into_iter()
        .filter(|(n, v)| *v != u64::MAX && selected(n))
        .collect()
}

/// Counters that increased between two reads, per interface; a counter or interface
/// missing on either side or going backwards (driver reset) is left out.
pub fn delta(before: &NicCounters, after: &NicCounters) -> NicCounters {
    after
        .iter()
        .filter_map(|(iface, a)| {
            let b = before.get(iface)?;
            let d: BTreeMap<String, u64> = a
                .iter()
                .filter_map(|(k, v)| {
                    let d = v.checked_sub(*b.get(k)?)?;
                    (d > 0).then(|| (k.clone(), d))
                })
                .collect();
            (!d.is_empty()).then(|| (iface.clone(), d))
        })
        .collect()
}

/// The delta when both reads saw at least one interface, else `None` (not read).
pub fn delta_if_read(before: &NicCounters, after: &NicCounters) -> Option<NicCounters> {
    (!before.is_empty() && !after.is_empty()).then(|| delta(before, after))
}

/// Per counter name, summed over interfaces (names differ between nodes): for the
/// report, which adds up many runs.
pub fn by_counter<'a>(deltas: impl IntoIterator<Item = &'a NicCounters>) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    for d in deltas {
        for counters in d.values() {
            for (k, v) in counters {
                *out.entry(k.clone()).or_default() += v;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(iface: &str, kv: &[(&str, u64)]) -> NicCounters {
        let m = kv.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
        [(iface.to_owned(), m)].into_iter().collect()
    }

    #[test]
    fn only_trouble_counters_are_kept() {
        let kept = select(
            [
                ("tx_packets", 9),
                ("stop_queue", 1),
                ("wake_queue", 1),
                ("rx_dropped", 2),
                ("hc_tx_pkt_drop_phy", 0),
                ("rx_crc_errors", 3),
                ("rx_fifo_overruns", 0),
                ("tx_timeout", 0),
                ("rx_missed", 4),
                ("tx_send_full", 0),
                ("tx_busy", 0),
                ("RX_Discards", 5),
                ("rx_bytes", 7),
                ("ch_time_busy", u64::MAX),
            ]
            .map(|(k, v)| (k.to_owned(), v)),
        );
        assert_eq!(kept.len(), 11);
        assert!(!kept.contains_key("ch_time_busy"));
        assert!(!kept.contains_key("tx_packets") && !kept.contains_key("rx_bytes"));
    }

    #[test]
    fn deltas_keep_increases_only() {
        let b = counters("vf0", &[("stop_queue", 10), ("rx_dropped", 5), ("gone", 1)]);
        let a = counters("vf0", &[("stop_queue", 13), ("rx_dropped", 4), ("new", 2)]);
        assert_eq!(delta(&b, &a), counters("vf0", &[("stop_queue", 3)]));
        assert!(delta(&b, &b).is_empty());
        assert!(delta(&NicCounters::new(), &a).is_empty());
        assert_eq!(delta_if_read(&b, &b), Some(NicCounters::new()));
        assert_eq!(delta_if_read(&NicCounters::new(), &a), None);
    }

    #[test]
    fn sums_by_counter_name_across_interfaces() {
        let mut x = counters("vf0", &[("stop_queue", 3)]);
        x.extend(counters("vf1", &[("stop_queue", 2), ("rx_dropped", 1)]));
        let y = counters("other", &[("stop_queue", 5)]);
        let s = by_counter([&x, &y]);
        assert_eq!(s.get("stop_queue"), Some(&10));
        assert_eq!(s.get("rx_dropped"), Some(&1));
    }
}
