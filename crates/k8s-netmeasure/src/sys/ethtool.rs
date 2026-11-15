//! `ioctl(SIOCETHTOOL)` for one interface (design §2). Only GET commands, which the
//! kernel allows without `CAP_NET_ADMIN`. `libc` has `ifreq` and `SIOCETHTOOL` but not
//! the ethtool structs; they are declared here from `<linux/ethtool.h>`.

use std::os::fd::AsRawFd;

use socket2::{Domain, Socket, Type};

/// `ETHTOOL_GRINGPARAM` from `<linux/ethtool.h>`.
const ETHTOOL_GRINGPARAM: u32 = 0x10;

/// `struct ethtool_ringparam` (UAPI, 9 × `__u32`).
#[repr(C)]
#[derive(Debug, Default)]
struct RingParam {
    cmd: u32,
    rx_max_pending: u32,
    rx_mini_max_pending: u32,
    rx_jumbo_max_pending: u32,
    tx_max_pending: u32,
    rx_pending: u32,
    rx_mini_pending: u32,
    rx_jumbo_pending: u32,
    tx_pending: u32,
}

/// Ring sizes of an interface, as `ethtool -g` shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rings {
    pub rx: u32,
    pub tx: u32,
    pub rx_max: u32,
    pub tx_max: u32,
}

/// Current and maximum RX/TX ring sizes of `ifname`, or the errno (`EOPNOTSUPP` for
/// interfaces without rings, e.g. `lo` or veth; `ENODEV` if it does not exist).
pub fn rings(ifname: &str) -> Result<Rings, i32> {
    let mut param = RingParam {
        cmd: ETHTOOL_GRINGPARAM,
        ..RingParam::default()
    };
    ioctl(ifname, (&raw mut param).cast())?;
    Ok(Rings {
        rx: param.rx_pending,
        tx: param.tx_pending,
        rx_max: param.rx_max_pending,
        tx_max: param.tx_max_pending,
    })
}

/// `SIOCETHTOOL` on `ifname` with `data` pointing to an ethtool command struct.
pub(super) fn ioctl(ifname: &str, data: *mut libc::c_char) -> Result<(), i32> {
    let name = ifname.as_bytes();
    if name.is_empty() || name.len() >= libc::IFNAMSIZ || name.contains(&0) {
        return Err(libc::EINVAL);
    }
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)
        .map_err(|e| e.raw_os_error().unwrap_or(libc::EIO))?;
    // SAFETY: `ifreq` is plain data (name array and a union of plain fields); all
    // zeroes is a valid value.
    let mut req: libc::ifreq = unsafe { std::mem::zeroed() };
    for (dst, src) in req.ifr_name.iter_mut().zip(name) {
        *dst = *src as libc::c_char;
    }
    req.ifr_ifru.ifru_data = data;
    // SAFETY: open socket; `req` holds a NUL-terminated name (zeroed tail) and points
    // to a live, correctly sized ethtool struct that the kernel fills.
    let rc = unsafe { libc::ioctl(socket.as_raw_fd(), libc::SIOCETHTOOL as _, &mut req) };
    if rc == 0 {
        Ok(())
    } else {
        Err(super::last_errno())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_matches_the_uapi_header() {
        assert_eq!(size_of::<RingParam>(), 36);
    }

    #[test]
    fn errors_without_rings_or_interface() {
        assert_eq!(rings("lo"), Err(libc::EOPNOTSUPP));
        assert_eq!(rings("nm-no-such-if"), Err(libc::ENODEV));
        assert_eq!(rings(""), Err(libc::EINVAL));
        assert_eq!(rings("a-name-longer-than-15"), Err(libc::EINVAL));
    }
}
