//! Decoding of `TCP_INFO` and `SO_MEMINFO` buffers (design §3.3). The kernel fills
//! only as much of `struct tcp_info` as its version knows and returns that length in
//! `optlen`; a field outside the filled part is `Unknown`, never a misleading 0. No
//! crate offers that distinction, so the decoding is ours (checked in milestone 2:
//! `libc` has the struct but no fill check, and its musl `tcp_info` lacks
//! `tcpi_total_rto`; `socket2` and `nix` have no `TCP_INFO`/`SO_MEMINFO` support).
//! The offsets themselves come from the caller (the binary takes them from `libc`
//! with `offset_of!`); this crate has no `libc` dependency and stays testable
//! without a kernel.

use serde::Serialize;

/// Byte offsets of the `struct tcp_info` fields that are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpInfoLayout {
    /// `tcpi_state`, `u8`.
    pub state: usize,
    /// `tcpi_total_retrans`, `u32`.
    pub total_retrans: usize,
    /// `tcpi_total_rto`, `u16` (newer kernels only).
    pub total_rto: usize,
}

/// A counter that the kernel may or may not provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Field<T> {
    Known(T),
    #[serde(serialize_with = "unknown")]
    Unknown,
}

fn unknown<S: serde::Serializer>(s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str("unknown")
}

impl<T: Copy> Field<T> {
    pub fn known(self) -> Option<T> {
        match self {
            Field::Known(v) => Some(v),
            Field::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpInfo {
    pub state: Option<u8>,
    pub total_retrans: Field<u32>,
    pub total_rto: Field<u16>,
}

impl TcpInfo {
    pub const UNAVAILABLE: TcpInfo = TcpInfo {
        state: None,
        total_retrans: Field::Unknown,
        total_rto: Field::Unknown,
    };
}

/// `buf[..len]` is what the kernel filled (`optlen` after `getsockopt`). A field the
/// kernel did not fill completely is `Unknown`.
pub fn decode_tcp_info(layout: TcpInfoLayout, buf: &[u8], len: usize) -> TcpInfo {
    let filled = &buf[..len.min(buf.len())];
    TcpInfo {
        state: filled.get(layout.state).copied(),
        total_retrans: read_u32(filled, layout.total_retrans).map_or(Field::Unknown, Field::Known),
        total_rto: read_u16(filled, layout.total_rto).map_or(Field::Unknown, Field::Known),
    }
}

/// Element `drops_index` (`SK_MEMINFO_DROPS`) of the `u32` array in a `SO_MEMINFO`
/// buffer of `len` filled bytes.
pub fn decode_meminfo_drops(drops_index: usize, buf: &[u8], len: usize) -> Field<u32> {
    let filled = &buf[..len.min(buf.len())];
    read_u32(filled, drops_index * 4).map_or(Field::Unknown, Field::Known)
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_ne_bytes(bytes))
}

fn read_u16(b: &[u8], at: usize) -> Option<u16> {
    let bytes: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
    Some(u16::from_ne_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Linux UAPI layout (x86_64), as the binary gets it from `libc`.
    const L: TcpInfoLayout = TcpInfoLayout {
        state: 0,
        total_retrans: 100,
        total_rto: 240,
    };
    const DROPS: usize = 8;

    fn tcp_info_buf(state: u8, retrans: u32, rto: u16) -> [u8; 256] {
        let mut b = [0u8; 256];
        b[L.state] = state;
        b[L.total_retrans..L.total_retrans + 4].copy_from_slice(&retrans.to_ne_bytes());
        b[L.total_rto..L.total_rto + 2].copy_from_slice(&rto.to_ne_bytes());
        b
    }

    #[test]
    fn full_struct_has_all_fields() {
        let b = tcp_info_buf(1, 2, 3);
        let info = decode_tcp_info(L, &b, 280);
        assert_eq!(info.state, Some(1));
        assert_eq!(info.total_retrans, Field::Known(2));
        assert_eq!(info.total_rto, Field::Known(3));
    }

    #[test]
    fn older_kernel_without_total_rto() {
        let info = decode_tcp_info(L, &tcp_info_buf(1, 1, 9), 232);
        assert_eq!(info.total_retrans, Field::Known(1));
        assert_eq!(info.total_rto, Field::Unknown);
    }

    #[test]
    fn short_or_empty_buffers() {
        let b = tcp_info_buf(1, 1, 1);
        assert_eq!(decode_tcp_info(L, &b, 103).total_retrans, Field::Unknown);
        assert_eq!(decode_tcp_info(L, &b, 0), TcpInfo::UNAVAILABLE);
    }

    #[test]
    fn meminfo_drops() {
        let mut b = [0u8; (DROPS + 1) * 4];
        b[32..36].copy_from_slice(&7u32.to_ne_bytes());
        assert_eq!(decode_meminfo_drops(DROPS, &b, b.len()), Field::Known(7));
        assert_eq!(decode_meminfo_drops(DROPS, &b, 32), Field::Unknown);
        assert_eq!(Field::Known(7).known(), Some(7));
        assert_eq!(Field::<u32>::Unknown.known(), None);
    }
}
