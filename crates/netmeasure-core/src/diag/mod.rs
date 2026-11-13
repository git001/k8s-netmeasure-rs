//! Pure parsers for diagnostics read by the binary (cgroup v2, `/proc/cpuinfo`,
//! `/proc/net/netstat`, `/proc/net/route`).

pub mod cgroup;
pub mod cpuinfo;
pub mod netstat;
pub mod route;
