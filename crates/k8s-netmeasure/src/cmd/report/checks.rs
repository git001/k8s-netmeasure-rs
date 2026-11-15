//! Checks before anything is compared (design §10.5, §10.6): the declared design
//! against the wall-clock spans, the context of both sides per node, and the evidence
//! label of the sequential design.

use std::collections::{BTreeMap, BTreeSet};

use netmeasure_cli::report::Factor;
use netmeasure_core::schema::control::NodeContext;

use super::load::RunData;

/// Overlap allowed in the sequential design: a clock step after a reboot (§10.6).
const MAX_OVERLAP_NS: i128 = 1_000_000_000;

/// `Err` with the spans when the sides overlap in time (exit 5).
pub fn sequential(before: &[&RunData], after: &[&RunData]) -> Result<i128, String> {
    let span = |rs: &[&RunData]| {
        let s = rs.iter().map(|r| r.start_ns).min().unwrap_or(0);
        let e = rs.iter().map(|r| r.end_ns).max().unwrap_or(0);
        (s, e)
    };
    let ((bs, be), (as_, ae)) = (span(before), span(after));
    let overlap = (be.min(ae) - bs.max(as_)).max(0);
    if overlap > MAX_OVERLAP_NS {
        return Err(format!(
            "the sides overlap by {:.1} s in wall-clock time: not a sequential design \
             (or a node clock is off; the NTP state is not recorded)",
            overlap as f64 / 1e9
        ));
    }
    Ok(overlap)
}

/// One context field across both sides.
pub struct Field {
    pub name: &'static str,
    pub before: BTreeSet<String>,
    pub after: BTreeSet<String>,
    /// Part of the declared factor: shown without a warning marker.
    pub factor: bool,
}

impl Field {
    pub fn differs(&self) -> bool {
        self.before != self.after
    }
}

/// The fields every report lists, over the source contexts of all runs; `factor`
/// marks the fields the declared factor is about (§10.7 table).
pub fn fields(before: &[&RunData], after: &[&RunData], declared: Factor) -> Vec<Field> {
    type Get = fn(&NodeContext) -> Option<String>;
    let (kernel, nic) = (declared == Factor::Kernel, declared == Factor::NicSettings);
    let list: [(&str, bool, Get); 7] = [
        ("kernel_release", kernel, |c| c.kernel_release.value.clone()),
        ("kernel_version", kernel, |c| c.kernel_version.value.clone()),
        ("instance_type", false, |c| c.instance_type.value.clone()),
        ("cpu_model", false, |c| c.cpu_model.value.clone()),
        ("nic_family", false, |c| Some(family(c))),
        ("nic_rings", nic, |c| Some(rings(c))),
        ("mtu", false, |c| c.mtu.value.map(|m| m.to_string())),
    ];
    let set = |rs: &[&RunData], get: Get| -> BTreeSet<String> {
        rs.iter()
            .map(|r| get(&r.context).unwrap_or_else(|| "unknown".into()))
            .collect()
    };
    list.iter()
        .map(|&(name, factor, get)| Field {
            name,
            before: set(before, get),
            after: set(after, get),
            factor,
        })
        .collect()
}

/// Reads one context value of a node.
type NodeGet = fn(&NodeContext) -> String;

/// Per node: the NIC family (drivers of the physical interfaces) or CPU model must
/// not change between the sides (§10.6), with `nic-settings` not the kernel either.
/// Returns `(field, node, before, after)`.
pub fn node_mismatches(
    before: &[&RunData],
    after: &[&RunData],
    declared: Factor,
) -> Vec<(&'static str, String, String, String)> {
    let per_node = |rs: &[&RunData], get: NodeGet| {
        let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in rs {
            m.entry(r.node.clone()).or_default().insert(get(&r.context));
        }
        m
    };
    let mut out = Vec::new();
    let mut checks: Vec<(&'static str, NodeGet)> = vec![
        ("nic_family", family),
        ("cpu_model", |c| {
            c.cpu_model
                .value
                .clone()
                .unwrap_or_else(|| "unknown".into())
        }),
    ];
    if declared == Factor::NicSettings {
        checks.push(("kernel_release", |c| {
            c.kernel_release
                .value
                .clone()
                .unwrap_or_else(|| "unknown".into())
        }));
    }
    for (name, get) in checks {
        let (b, a) = (per_node(before, get), per_node(after, get));
        for (node, vb) in &b {
            if let Some(va) = a.get(node).filter(|va| *va != vb) {
                out.push((name, node.clone(), join(vb), join(va)));
            }
        }
    }
    out
}

fn join(s: &BTreeSet<String>) -> String {
    s.iter().cloned().collect::<Vec<_>>().join(" | ")
}

/// Sorted drivers of the physical interfaces, `unknown` if not observed.
pub fn family(c: &NodeContext) -> String {
    match &c.nic.value {
        None => "unknown".into(),
        Some(p) => {
            let d: BTreeSet<&str> = p
                .interfaces
                .iter()
                .filter_map(|i| i.driver.as_deref())
                .collect();
            d.into_iter().collect::<Vec<_>>().join("+")
        }
    }
}

fn rings(c: &NodeContext) -> String {
    match &c.nic.value {
        None => "unknown".into(),
        Some(p) => {
            let mut v: Vec<String> = p
                .interfaces
                .iter()
                .map(|i| match &i.rings {
                    Some(r) => format!(
                        "{}:{}/{}",
                        i.driver.as_deref().unwrap_or("-"),
                        opt(r.rx),
                        opt(r.tx)
                    ),
                    None => format!("{}:-", i.driver.as_deref().unwrap_or("-")),
                })
                .collect();
            v.sort();
            v.join(" ")
        }
    }
}

/// Evidence label of the sequential design (§10.5) and the reason.
pub fn evidence(before: &[&RunData], after: &[&RunData]) -> (&'static str, String) {
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

fn opt(v: Option<u32>) -> String {
    v.map_or("-".into(), |x| x.to_string())
}
