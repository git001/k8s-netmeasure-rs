//! `ethtool -i` and `ethtool -l` for one interface (design §5, milestone 6):
//! `ETHTOOL_GDRVINFO` for driver version, firmware and bus address,
//! `ETHTOOL_GCHANNELS` for the channel (queue) counts. GET commands only, allowed
//! without `CAP_NET_ADMIN`. Structs from `<linux/ethtool.h>`.

use super::ethtool::ioctl;

const ETHTOOL_GDRVINFO: u32 = 0x03;
const ETHTOOL_GCHANNELS: u32 = 0x3c;

/// `struct ethtool_drvinfo` (UAPI, 196 bytes).
#[repr(C)]
#[derive(Debug, Default)]
struct DrvInfo {
    cmd: u32,
    driver: [u8; 32],
    version: [u8; 32],
    fw_version: [u8; 32],
    bus_info: [u8; 32],
    erom_version: [u8; 32],
    reserved2: [u8; 12],
    n_priv_flags: u32,
    n_stats: u32,
    testinfo_len: u32,
    eedump_len: u32,
    regdump_len: u32,
}

/// `struct ethtool_channels` (UAPI, 9 × `__u32`).
#[repr(C)]
#[derive(Debug, Default)]
struct ChannelsRaw {
    cmd: u32,
    max_rx: u32,
    max_tx: u32,
    max_other: u32,
    max_combined: u32,
    rx_count: u32,
    tx_count: u32,
    other_count: u32,
    combined_count: u32,
}

/// What `ethtool -i` shows besides the driver name (that comes from sysfs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverInfo {
    pub version: String,
    pub firmware: String,
    pub bus: String,
}

/// `(current, max)` per kind: rx, tx, other, combined.
pub type Channels = [(u32, u32); 4];

/// The NUL-terminated string in `b`, lossy UTF-8, trimmed.
fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_owned()
}

/// Driver version, firmware and bus address of `ifname`, or the errno.
pub fn driver_info(ifname: &str) -> Result<DriverInfo, i32> {
    let mut d = DrvInfo {
        cmd: ETHTOOL_GDRVINFO,
        ..DrvInfo::default()
    };
    ioctl(ifname, (&raw mut d).cast())?;
    Ok(DriverInfo {
        version: text(&d.version),
        firmware: text(&d.fw_version),
        bus: text(&d.bus_info),
    })
}

/// Channel counts of `ifname`, or the errno (`EOPNOTSUPP` without channels).
pub fn channels(ifname: &str) -> Result<Channels, i32> {
    let mut c = ChannelsRaw {
        cmd: ETHTOOL_GCHANNELS,
        ..ChannelsRaw::default()
    };
    ioctl(ifname, (&raw mut c).cast())?;
    Ok([
        (c.rx_count, c.max_rx),
        (c.tx_count, c.max_tx),
        (c.other_count, c.max_other),
        (c.combined_count, c.max_combined),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structs_match_the_uapi_header() {
        assert_eq!(size_of::<DrvInfo>(), 196);
        assert_eq!(size_of::<ChannelsRaw>(), 36);
    }

    #[test]
    fn strings_end_at_the_first_nul() {
        assert_eq!(text(b"1.2.3\0junk"), "1.2.3");
        assert_eq!(text(b"\0"), "");
        assert_eq!(text(b"no-nul"), "no-nul");
    }

    #[test]
    fn loopback_and_missing_interfaces() {
        // `lo` has no channels; a missing interface is ENODEV for both.
        assert_eq!(channels("lo"), Err(libc::EOPNOTSUPP));
        assert_eq!(channels("nm-no-such-if"), Err(libc::ENODEV));
        assert_eq!(driver_info("nm-no-such-if"), Err(libc::ENODEV));
        assert_eq!(driver_info(""), Err(libc::EINVAL));
    }
}
