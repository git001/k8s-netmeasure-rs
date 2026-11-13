//! Reading diagnostics from `/proc` and `/sys` (design §2). Parsing is in
//! `netmeasure_core::diag`.

pub mod cgroup;
pub mod net;
pub mod node;

/// Reads a small text file; `None` if it does not exist or is unreadable.
pub fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
