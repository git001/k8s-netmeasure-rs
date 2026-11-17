//! The only module allowed to contain `unsafe` (design §2): thin wrappers over
//! syscalls, through the safe `nix` API wherever it covers the call. `unsafe` remains
//! only where `nix` has no wrapper or the call is unsafe by nature: installing a
//! signal handler (`sigaction`), `getsockopt` for partially filled buffers
//! (`sockopt`) and `ioctl(SIOCETHTOOL)` (`ethtool`, shared by `ethtool_stats` and `ethtool_info`). Each wrapper returns plain values
//! or an errno; no policy lives here.
#![allow(unsafe_code)]

pub mod ethtool;
pub mod ethtool_info;
pub mod ethtool_stats;
pub mod ifaddr;
pub mod poll;
pub mod sigaction;
pub mod signals;
pub mod sockopt;
pub mod time;

/// The calling thread's current `errno`.
pub fn last_errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}
