//! The own cgroup v2 and every visible ancestor (design §3.7).

use netmeasure_core::diag::cgroup::{self, CpuMax, CpuStat};
use serde::Serialize;

const ROOT: &str = "/sys/fs/cgroup";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Level {
    pub path: String,
    pub cpu_max: Option<CpuMax>,
    pub cpu_stat: Option<CpuStat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CgroupReport {
    /// From `/proc/self/cgroup`; `None` without cgroup v2.
    pub path: Option<String>,
    /// The own cgroup's files are readable.
    pub readable: bool,
    pub own: Option<Level>,
    /// Ancestors whose `cpu.max` is visible, nearest first. Empty with a private
    /// cgroup namespace; that is reported, never read as "no throttling".
    pub visible_ancestors: Vec<Level>,
}

pub fn read() -> CgroupReport {
    let path = super::read("/proc/self/cgroup").and_then(|t| cgroup::parse_self_cgroup(&t));
    let Some(path) = path else {
        return CgroupReport {
            path: None,
            readable: false,
            own: None,
            visible_ancestors: vec![],
        };
    };
    // With a private cgroup namespace the path is "/" and the own cgroup is the
    // mount root; no ancestor is visible. Without one, the own cgroup and its
    // ancestors are directories below the root.
    let own = level(&path, dir_of(&path));
    let visible_ancestors = cgroup::ancestors(&path)
        .into_iter()
        .filter_map(|a| level(&a, dir_of(&a)))
        .collect();
    CgroupReport {
        readable: own.is_some(),
        path: Some(path),
        own,
        visible_ancestors,
    }
}

fn dir_of(path: &str) -> String {
    if path == "/" {
        ROOT.to_owned()
    } else {
        format!("{ROOT}{path}")
    }
}

fn level(path: &str, dir: String) -> Option<Level> {
    let cpu_max =
        super::read(&format!("{dir}/cpu.max")).and_then(|t| cgroup::parse_cpu_max(&t).ok());
    let cpu_stat = super::read(&format!("{dir}/cpu.stat")).map(|t| cgroup::parse_cpu_stat(&t));
    (cpu_max.is_some() || cpu_stat.is_some()).then(|| Level {
        path: path.to_owned(),
        cpu_max,
        cpu_stat,
    })
}
