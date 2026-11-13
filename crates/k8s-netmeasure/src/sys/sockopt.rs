//! `getsockopt` for buffers the kernel fills partially (`TCP_INFO`, `SO_MEMINFO`).
//! Decoding is in `netmeasure_core::measure::sockinfo`; the layout comes from `libc`.
//! Raw `libc` on purpose: `nix` has neither option and does not return the filled
//! length of a partial buffer.

use std::mem::offset_of;
use std::os::fd::{AsRawFd, BorrowedFd};

use netmeasure_core::measure::sockinfo::TcpInfoLayout;

/// `tcpi_total_rto` (Linux 6.7+). `libc` declares it only in the glibc `tcp_info`, not
/// in the musl one, so the offset is a constant here; the test below checks it against
/// `libc` on glibc targets.
const TCPI_TOTAL_RTO: usize = 240;

/// Offsets of the `struct tcp_info` fields that are read.
pub const TCP_INFO_LAYOUT: TcpInfoLayout = TcpInfoLayout {
    state: offset_of!(libc::tcp_info, tcpi_state),
    total_retrans: offset_of!(libc::tcp_info, tcpi_total_retrans),
    total_rto: TCPI_TOTAL_RTO,
};

/// Bytes requested: up to and including the last field that is read.
const TCP_INFO_BUF: usize = TCPI_TOTAL_RTO + size_of::<u16>();
const _: () = assert!(TCP_INFO_LAYOUT.total_retrans + size_of::<u32>() <= TCP_INFO_BUF);

/// Index of `SK_MEMINFO_DROPS` in the `SO_MEMINFO` `u32` array.
pub const SK_MEMINFO_DROPS: usize = libc::SK_MEMINFO_DROPS as usize;
const MEMINFO_BUF: usize = (SK_MEMINFO_DROPS + 1) * size_of::<u32>();

/// `TCP_INFO`: the buffer and the number of bytes the kernel filled.
pub fn tcp_info(fd: BorrowedFd<'_>) -> Result<([u8; TCP_INFO_BUF], usize), i32> {
    let mut buf = [0u8; TCP_INFO_BUF];
    let len = get(fd, libc::IPPROTO_TCP, libc::TCP_INFO, &mut buf)?;
    Ok((buf, len))
}

/// `SO_MEMINFO` up to `SK_MEMINFO_DROPS`, as bytes, and the filled length.
pub fn meminfo(fd: BorrowedFd<'_>) -> Result<([u8; MEMINFO_BUF], usize), i32> {
    let mut buf = [0u8; MEMINFO_BUF];
    let len = get(fd, libc::SOL_SOCKET, libc::SO_MEMINFO, &mut buf)?;
    Ok((buf, len))
}

fn get(
    fd: BorrowedFd<'_>,
    level: libc::c_int,
    name: libc::c_int,
    buf: &mut [u8],
) -> Result<usize, i32> {
    let mut len = buf.len() as libc::socklen_t;
    // SAFETY: `fd` is open for the borrow; `buf` is valid for `len` bytes; the kernel
    // writes at most `len` and stores the filled length back into `len`.
    let rc = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            level,
            name,
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(len as usize)
    } else {
        Err(super::last_errno())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_env = "gnu")]
    #[test]
    fn total_rto_offset_matches_libc() {
        assert_eq!(TCPI_TOTAL_RTO, offset_of!(libc::tcp_info, tcpi_total_rto));
    }

    #[test]
    fn layout_matches_linux_uapi() {
        // Same values as `offsetof` on the UAPI header; guards against a libc change.
        assert_eq!(TCP_INFO_LAYOUT.state, 0);
        assert_eq!(TCP_INFO_LAYOUT.total_retrans, 100);
        assert_eq!(SK_MEMINFO_DROPS, 8);
    }
}
