//! cgroup v2 `cpu.max`, `cpu.stat` and `/proc/self/cgroup` (design §3.7).
//!
//! Own code on purpose (checked in milestone 2): `procfs` parses `/proc/<pid>/cgroup`
//! but not the cgroup v2 files `cpu.max`/`cpu.stat`, and would add several
//! dependencies for a one-line format; `cgroups-rs` manages cgroups, is v1-oriented
//! and pulls in `nix` and `zbus`. The formats are small and documented in the kernel's
//! cgroup-v2 documentation.

use serde::{Deserialize, Serialize};

/// `cpu.max`: `max <period>` or `<quota> <period>` (µs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuMax {
    /// `None` = unlimited (`max`).
    pub quota_us: Option<u64>,
    pub period_us: u64,
}

impl CpuMax {
    /// The quota in millicores, `None` if unlimited.
    pub fn millicores(&self) -> Option<u64> {
        let q = self.quota_us?;
        Some(q.saturating_mul(1000) / self.period_us.max(1))
    }
}

pub fn parse_cpu_max(text: &str) -> Result<CpuMax, String> {
    let mut parts = text.split_whitespace();
    let (Some(quota), Some(period), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!(
            "cpu.max: expected `<quota|max> <period>`, got `{}`",
            text.trim()
        ));
    };
    let period_us = period
        .parse()
        .map_err(|_| format!("cpu.max: bad period `{period}`"))?;
    let quota_us = match quota {
        "max" => None,
        q => Some(q.parse().map_err(|_| format!("cpu.max: bad quota `{q}`"))?),
    };
    Ok(CpuMax {
        quota_us,
        period_us,
    })
}

/// The throttling counters of `cpu.stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuStat {
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}

pub fn parse_cpu_stat(text: &str) -> CpuStat {
    let mut stat = CpuStat::default();
    for line in text.lines() {
        let mut kv = line.split_whitespace();
        let (Some(k), Some(v)) = (kv.next(), kv.next().and_then(|v| v.parse().ok())) else {
            continue;
        };
        match k {
            "nr_periods" => stat.nr_periods = v,
            "nr_throttled" => stat.nr_throttled = v,
            "throttled_usec" => stat.throttled_usec = v,
            _ => {}
        }
    }
    stat
}

/// CPU quota floor in millicores (§3.7): 500m, plus 500m per load thread.
pub fn floor_millicores(load_threads: u64) -> u64 {
    500 + 500 * load_threads
}

/// How throttling of ancestor cgroups is known (§3.7): `observed` if ancestors are
/// visible (each with a quota is checked per cycle); without visible ancestors,
/// `inferred_impossible` if the own cgroup has a limit (the pod quota is then the sum
/// of all container limits), else `unknown`. The inference ignores pod-level
/// resources, hence the note.
pub fn ancestor_throttling(
    visible_ancestors: usize,
    own_limit: bool,
) -> (&'static str, Option<&'static str>) {
    match (visible_ancestors, own_limit) {
        (n, _) if n > 0 => ("observed", None),
        (_, true) => (
            "inferred_impossible",
            Some("unverified: pod-level resources not checked"),
        ),
        _ => ("unknown", None),
    }
}

/// The cgroup v2 path from `/proc/self/cgroup` (`0::<path>`); `None` on cgroup v1.
pub fn parse_self_cgroup(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("0::"))
        .map(|p| p.trim().to_owned())
}

/// Ancestor paths of `path` below the root, nearest first: `/a/b/c` → `/a/b`, `/a`.
pub fn ancestors(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    (1..parts.len())
        .rev()
        .map(|n| format!("/{}", parts[..n].join("/")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_and_ancestor_classification() {
        assert_eq!(floor_millicores(0), 500);
        assert_eq!(floor_millicores(2), 1500);
        assert_eq!(ancestor_throttling(3, false), ("observed", None));
        assert_eq!(ancestor_throttling(0, true).0, "inferred_impossible");
        assert!(
            ancestor_throttling(0, true)
                .1
                .unwrap()
                .contains("unverified")
        );
        assert_eq!(ancestor_throttling(0, false), ("unknown", None));
    }

    #[test]
    fn cpu_max() {
        let m = parse_cpu_max("50000 100000\n").unwrap();
        assert_eq!(
            m,
            CpuMax {
                quota_us: Some(50000),
                period_us: 100000
            }
        );
        assert_eq!(m.millicores(), Some(500));
        assert_eq!(parse_cpu_max("max 100000").unwrap().millicores(), None);
        assert!(parse_cpu_max("max").is_err());
        assert!(parse_cpu_max("x 100").is_err());
        assert!(parse_cpu_max("1 y").is_err());
    }

    #[test]
    fn cpu_stat() {
        let s = parse_cpu_stat(
            "usage_usec 9\nnr_periods 10\nnr_throttled 2\nthrottled_usec 345\nbad\n",
        );
        assert_eq!(
            s,
            CpuStat {
                nr_periods: 10,
                nr_throttled: 2,
                throttled_usec: 345
            }
        );
    }

    #[test]
    fn self_cgroup_and_ancestors() {
        assert_eq!(
            parse_self_cgroup("0::/kubepods.slice/pod1/c1\n").as_deref(),
            Some("/kubepods.slice/pod1/c1")
        );
        assert_eq!(parse_self_cgroup("1:cpu:/x\n"), None);
        assert_eq!(ancestors("/a/b/c"), vec!["/a/b", "/a"]);
        assert!(ancestors("/").is_empty());
    }
}
