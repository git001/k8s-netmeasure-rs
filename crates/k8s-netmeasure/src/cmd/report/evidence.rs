//! Evidence label of the sequential design (design §10.5) and the source-node check
//! behind it (§10.6: the sequential design measures the same nodes before and after).

use std::collections::{BTreeMap, BTreeSet};

use super::load::RunData;

/// Source nodes on both sides, only before, only after (§10.6: the sequential design
/// measures the same nodes before and after).
pub fn node_sets(before: &[&RunData], after: &[&RunData]) -> (usize, usize, usize) {
    let set = |rs: &[&RunData]| -> BTreeSet<String> { rs.iter().map(|r| r.node.clone()).collect() };
    let (b, a) = (set(before), set(after));
    let common = b.intersection(&a).count();
    (common, b.len() - common, a.len() - common)
}

/// Evidence label of the sequential design (§10.5) and the reason. Without a common
/// source node the comparison is observational: at most `exploratory`.
pub fn evidence(before: &[&RunData], after: &[&RunData]) -> (&'static str, String) {
    if node_sets(before, after).0 == 0 {
        return (
            "exploratory",
            "no source node on both sides: observational, not before/after on the same \
             nodes"
                .into(),
        );
    }
    let runs = |rs: &[&RunData]| {
        let mut per: BTreeMap<&str, usize> = BTreeMap::new();
        rs.iter()
            .for_each(|r| *per.entry(r.node.as_str()).or_default() += 1);
        per.values().copied().min().unwrap_or(0)
    };
    let (rb, ra) = (runs(before), runs(after));
    let mut order: Vec<(i128, bool)> = before.iter().map(|r| (r.start_ns, false)).collect();
    order.extend(after.iter().map(|r| (r.start_ns, true)));
    order.sort();
    let switches = order.windows(2).filter(|w| w[0].1 != w[1].1).count();
    let detail = format!("{rb} vs {ra} runs per node, {switches} side switch(es) in time");
    if rb.min(ra) <= 1 {
        ("exploratory", detail)
    } else if rb.min(ra) >= 5 && switches >= 3 {
        ("strong", detail)
    } else {
        ("preliminary", detail)
    }
}
