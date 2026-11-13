//! Pure parsers for diagnostics read by the binary (cgroup v2, `/proc/cpuinfo`,
//! `/proc/net/netstat`).

pub mod cgroup;
pub mod cpuinfo;
pub mod netstat;
